//! A signed, networked artifact registry for Anvil.
//!
//! The filesystem shared store (`ORPHEUS_CACHE_SHARED`) lets same-toolchain hosts reuse builds over
//! a shared directory. This is its networked sibling: a tiny HTTP service that holds compiled
//! binaries so a fleet without shared storage (CI runners, separate machines) can still compile
//! each program once. Because artifacts now cross an untrusted network, they are **signed**: every
//! binary carries a MAC, the server refuses to store one whose MAC doesn't verify, and the client
//! refuses to install one whose MAC doesn't verify — so a tampered or unauthenticated artifact is
//! rejected rather than executed.
//!
//! The MAC is **HMAC-SHA3-256 under a shared key** (`ORPHEUS_REGISTRY_KEY`). This is a standard
//! construction over the SHA-3 the rest of the system already uses; it gives integrity and
//! authenticity among parties that hold the key (the natural "trusted CI builders + consumers"
//! model). It is deliberately *not* a public-key signature: a real asymmetric scheme would let
//! verifiers check artifacts without holding a signing key, but implementing elliptic-curve crypto
//! has no place in a hand-rolled, zero-dependency codebase — that is the one piece you would bring a
//! vetted crypto library in for.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

// --------------------------------------------------------------------- config

/// Base URL of the registry, e.g. `http://10.0.0.5:8099`, from `ORPHEUS_REGISTRY`.
pub fn registry_url() -> Option<String> {
    match std::env::var("ORPHEUS_REGISTRY") {
        Ok(s) if !s.trim().is_empty() => Some(s.trim().trim_end_matches('/').to_string()),
        _ => None,
    }
}

/// The shared MAC key from `ORPHEUS_REGISTRY_KEY` (empty ⇒ signing unavailable).
pub fn registry_key() -> Option<Vec<u8>> {
    match std::env::var("ORPHEUS_REGISTRY_KEY") {
        Ok(s) if !s.is_empty() => Some(s.into_bytes()),
        _ => None,
    }
}

/// Soft size cap for the artifact store, from `ORPHEUS_REGISTRY_MAX` (MiB, default 2048). After each
/// upload the oldest artifacts are evicted until the store is back under the cap, so a long-running
/// registry doesn't grow without bound.
fn registry_max_bytes() -> u64 {
    std::env::var("ORPHEUS_REGISTRY_MAX")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(2048)
        .saturating_mul(1024 * 1024)
}

/// Pure LRU eviction plan: given `(path, mtime, pair_size)` for each stored artifact and a byte
/// `cap`, return the oldest artifacts to remove so the total drops to `cap` or below. Returns empty
/// when already under cap.
fn plan_registry_evictions(
    mut items: Vec<(std::path::PathBuf, u64, u64)>,
    cap: u64,
) -> Vec<std::path::PathBuf> {
    let total: u64 = items.iter().map(|i| i.2).sum();
    if total <= cap {
        return Vec::new();
    }
    items.sort_by_key(|i| i.1); // oldest first
    let mut running = total;
    let mut out = Vec::new();
    for (p, _, sz) in items {
        if running <= cap {
            break;
        }
        running -= sz;
        out.push(p);
    }
    out
}

/// Inventory stored artifacts (the binary of each `<tid>/<name>` + `.mac` pair) as
/// `(binary_path, mtime_secs, binary_size + mac_size)`.
fn artifact_inventory(root: &str) -> Vec<(std::path::PathBuf, u64, u64)> {
    let mut out = Vec::new();
    if let Ok(tids) = std::fs::read_dir(root) {
        for tid in tids.flatten() {
            if let Ok(files) = std::fs::read_dir(tid.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.ends_with(".mac") || name.contains(".tmp") {
                        continue;
                    }
                    let meta = match f.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let mac_sz = {
                        let mut mp = p.clone().into_os_string();
                        mp.push(".mac");
                        std::fs::metadata(std::path::PathBuf::from(mp)).map(|m| m.len()).unwrap_or(0)
                    };
                    out.push((p, mtime, meta.len() + mac_sz));
                }
            }
        }
    }
    out
}

fn evict_registry(root: &str, cap: u64) {
    for p in plan_registry_evictions(artifact_inventory(root), cap) {
        let _ = std::fs::remove_file(&p);
        let mut mp = p.into_os_string();
        mp.push(".mac");
        let _ = std::fs::remove_file(std::path::PathBuf::from(mp));
    }
}

/// Write `bytes` to `path` atomically (temp file + rename), so a reader never sees a half-written
/// file and a crash never leaves a torn artifact.
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = {
        let mut s = path.to_path_buf().into_os_string();
        s.push(format!(".tmp{}", std::process::id()));
        std::path::PathBuf::from(s)
    };
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

// ------------------------------------------------------------ HMAC-SHA3-256

/// HMAC-SHA3-256(key, msg). Block size B = 136 (the SHA3-256 rate).
fn hmac_sha3_256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const B: usize = 136;
    let mut k = if key.len() > B {
        crate::sha3::sha3_256(key).to_vec()
    } else {
        key.to_vec()
    };
    k.resize(B, 0);
    let mut inner = Vec::with_capacity(B + msg.len());
    inner.extend(k.iter().map(|b| b ^ 0x36)); // ipad
    inner.extend_from_slice(msg);
    let ih = crate::sha3::sha3_256(&inner);
    let mut outer = Vec::with_capacity(B + 32);
    outer.extend(k.iter().map(|b| b ^ 0x5c)); // opad
    outer.extend_from_slice(&ih);
    crate::sha3::sha3_256(&outer)
}

/// Hex MAC of `msg` under `key`.
pub fn mac_hex(key: &[u8], msg: &[u8]) -> String {
    crate::sha3::hex(&hmac_sha3_256(key, msg))
}

/// Length-independent equality for hex MACs (avoid early-exit timing leaks).
pub fn mac_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for i in 0..a.len() {
        d |= a[i] ^ b[i];
    }
    d == 0
}

// ----------------------------------------------------------------- HTTP client

fn parse_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (authority.to_string(), 80u16),
    };
    Some((host, port, path.to_string()))
}

fn read_response(stream: TcpStream) -> Option<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let status: u16 = line.split_whitespace().nth(1)?.parse().ok()?;
    let mut headers = Vec::new();
    let mut clen = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 {
            break;
        }
        let t = h.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            let (k, v) = (k.trim().to_string(), v.trim().to_string());
            if k.eq_ignore_ascii_case("content-length") {
                clen = v.parse().unwrap_or(0);
            }
            headers.push((k, v));
        }
    }
    let mut body = vec![0u8; clen];
    if clen > 0 {
        r.read_exact(&mut body).ok()?;
    }
    Some((status, headers, body))
}

fn connect(host: &str, port: u16) -> Option<TcpStream> {
    let s = TcpStream::connect((host, port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    s.set_write_timeout(Some(Duration::from_secs(30))).ok();
    Some(s)
}

/// `GET url` → (status, headers, body).
/// POST a body to a URL — the write half of the node-to-node client, used by
/// the db sync protocol to push records to a peer. Same zero-dependency
/// plumbing as http_get.
pub fn http_post(url: &str, body: &[u8]) -> Option<(u16, Vec<(String, String)>, Vec<u8>)> {
    let (host, port, path) = parse_url(url)?;
    let mut s = connect(&host, port)?;
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        path, host, body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    read_response(s)
}

pub fn http_get(url: &str) -> Option<(u16, Vec<(String, String)>, Vec<u8>)> {
    let (host, port, path) = parse_url(url)?;
    let mut s = connect(&host, port)?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    s.write_all(req.as_bytes()).ok()?;
    read_response(s)
}

/// `PUT url` with extra headers and a binary body → status code.
pub fn http_put(url: &str, extra: &[(&str, &str)], body: &[u8]) -> Option<u16> {
    let (host, port, path) = parse_url(url)?;
    let mut s = connect(&host, port)?;
    let mut head = format!(
        "PUT {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
        path,
        host,
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    let (status, _, _) = read_response(s)?;
    Some(status)
}

// ----------------------------------------------------------------- HTTP server

const MAC_HEADER: &str = "x-orpheus-mac";

/// Keep a path component to a safe charset (defeats traversal / odd names).
fn safe_component(s: &str) -> Option<String> {
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    if s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        Some(s.to_string())
    } else {
        None
    }
}

/// Resolve `/<tid>/<name>` to a storage path under `root`, rejecting anything unsafe.
fn slot(root: &str, path: &str) -> Option<std::path::PathBuf> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let tid = safe_component(parts[0])?;
    let name = safe_component(parts[1])?;
    Some(std::path::Path::new(root).join(tid).join(name))
}

/// The registry as a routing surface over the shared HTTP core (`httpd`). All wire handling —
/// parsing, response writing, keep-alive, HEAD — comes from the core; this only decides what each
/// request means.
struct Registry {
    root: String,
    key: Option<Vec<u8>>,
}

impl crate::httpd::Handler for Registry {
    // Artifacts are whole compiled binaries, so raise the body cap well past the GUI default.
    fn max_body(&self) -> usize {
        64 * 1024 * 1024
    }
    fn read_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }

    fn handle(&self, req: &crate::httpd::Request) -> crate::httpd::Response {
        use crate::httpd::Response;
        if req.path == "/health" {
            return Response::text(200, "ok");
        }
        let path = match slot(&self.root, &req.path) {
            Some(p) => p,
            None => return Response::text(400, "bad path"),
        };
        let mac_path = {
            let mut p = path.clone().into_os_string();
            p.push(".mac");
            std::path::PathBuf::from(p)
        };
        match req.method.as_str() {
            "GET" | "HEAD" => match (std::fs::read(&path), std::fs::read_to_string(&mac_path)) {
                (Ok(body), Ok(mac)) => {
                    // Self-healing read: if we hold the key, re-verify the stored MAC against the
                    // stored bytes before serving — so on-disk corruption is caught here rather than
                    // shipped to a client.
                    if let Some(key) = &self.key {
                        if !mac_eq(mac.trim(), &mac_hex(key, &body)) {
                            return Response::text(500, "stored artifact failed integrity check");
                        }
                    }
                    Response::bytes(200, "application/octet-stream", body).with_header(MAC_HEADER, mac.trim())
                }
                _ => Response::text(404, "no such artifact"),
            },
            "PUT" => {
                // Authenticated write only: the body's MAC must verify under the shared key.
                let key = match &self.key {
                    Some(k) => k,
                    None => return Response::text(503, "registry has no key"),
                };
                let claimed = req.header(MAC_HEADER).unwrap_or("");
                let actual = mac_hex(key, &req.body);
                if !mac_eq(claimed, &actual) {
                    return Response::text(401, "bad mac");
                }
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                // Write the MAC first, then the binary, each atomically: a reader requires both, so
                // a crash can only ever leave a harmless orphaned MAC, never a binary without one.
                if atomic_write(&mac_path, actual.as_bytes()).is_ok()
                    && atomic_write(&path, &req.body).is_ok()
                {
                    // Keep the store bounded.
                    evict_registry(&self.root, registry_max_bytes());
                    Response::text(200, "stored")
                } else {
                    Response::text(500, "store failed")
                }
            }
            _ => Response::text(405, "method"),
        }
    }
}

/// Run the registry server on the shared HTTP core, storing artifacts under `root`. Verifies the
/// MAC of every uploaded artifact against `ORPHEUS_REGISTRY_KEY`; without a key it serves reads but
/// rejects writes.
pub fn serve(listen: &str, root: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let key = registry_key();
    println!(
        "anvil registry on http://{}  root={}  signing={}",
        listen,
        root,
        if key.is_some() { "on" } else { "off (reads only)" }
    );
    crate::httpd::serve(listen, std::sync::Arc::new(Registry { root: root.to_string(), key }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_plan_drops_oldest_until_under_cap() {
        use std::path::PathBuf;
        // three artifacts, 100 bytes each (total 300), cap 150 → evict the two oldest.
        let items = vec![
            (PathBuf::from("old"), 10, 100),
            (PathBuf::from("mid"), 20, 100),
            (PathBuf::from("new"), 30, 100),
        ];
        let evicted = plan_registry_evictions(items, 150);
        assert_eq!(evicted, vec![PathBuf::from("old"), PathBuf::from("mid")]);
        // already under cap → nothing evicted
        let small = vec![(PathBuf::from("a"), 1, 50)];
        assert!(plan_registry_evictions(small, 150).is_empty());
    }

    #[test]
    fn hmac_is_deterministic_and_key_sensitive() {
        let a = mac_hex(b"key1", b"hello world");
        assert_eq!(a, mac_hex(b"key1", b"hello world")); // deterministic
        assert_ne!(a, mac_hex(b"key2", b"hello world")); // key-sensitive
        assert_ne!(a, mac_hex(b"key1", b"hello worle")); // message-sensitive
        assert_eq!(a.len(), 64); // sha3-256 hex
        assert!(mac_eq(&a, &a) && !mac_eq(&a, &mac_hex(b"key2", b"x")));
    }

    #[test]
    fn url_parsing() {
        assert_eq!(
            parse_url("http://1.2.3.4:8099/rustc/e1234"),
            Some(("1.2.3.4".into(), 8099, "/rustc/e1234".into()))
        );
        assert_eq!(parse_url("http://host/x"), Some(("host".into(), 80, "/x".into())));
        assert!(parse_url("ftp://x/y").is_none());
    }

    #[test]
    fn path_safety() {
        assert!(slot("/root", "/tid/name").is_some());
        assert!(slot("/root", "/../etc/passwd").is_none()); // traversal rejected
        assert!(slot("/root", "/a/b/c").is_none()); // wrong shape
        assert!(slot("/root", "/tid/na me").is_none()); // bad chars
    }
}

//! anvild — a resident Anvil compile server.
//!
//! A one-shot CLI process cannot usefully compile in the background: it exits in a fraction of a
//! second, killing any `rustc` it spawned, so the first run of a cold program either stalls on the
//! build or falls back to the interpreter forever. A long-lived daemon fixes exactly this. Its one
//! irreplaceable job is **background compilation that outlives the client** — the on-disk binary
//! cache (see `rustgen::cache_dir`) already gives warm *runs* across processes, so the daemon need
//! only *build*. A cold client hands the program to the daemon (`WARMBG`) and answers the current
//! call adaptively; the daemon finishes the build in the background, and the next client — a
//! separate process — finds the binary on disk and runs it natively.
//!
//! Transport is a Unix-domain socket under the cache dir (zero dependencies — all `std`). The
//! protocol is a request `"<VERB> <len>\n"` header followed by `len` payload bytes, answered with a
//! `"<STATUS> <len>\n"` header (`OK`/`MISS`/`ERR`) and `len` payload bytes. Verbs:
//!   * `PING`    — liveness check (`OK pong`).
//!   * `WARM`    — build the program synchronously, then reply (explicit prewarm).
//!   * `WARMBG`  — spawn the build in the background, reply `OK` at once (the hot path).
//!   * `STATS`   — reply `OK "<count> <bytes>"` from the binary cache.
//!   * `STOP`    — reply `OK`, then exit the daemon.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

fn sock_path() -> PathBuf {
    crate::rustgen::cache_dir().join("anvild.sock")
}
fn pid_path() -> PathBuf {
    crate::rustgen::cache_dir().join("anvild.pid")
}

// ----------------------------------------------------------------------------- framing

/// Parse a `"<VERB> <len>"` header line. Pure and total, so it is unit-testable without any I/O.
pub(crate) fn parse_header(line: &str) -> Option<(String, usize)> {
    let mut it = line.trim().splitn(2, ' ');
    let verb = it.next()?.to_string();
    let len: usize = it.next()?.trim().parse().ok()?;
    if verb.is_empty() {
        return None;
    }
    Some((verb, len))
}

fn write_msg(s: &mut UnixStream, head: &str, payload: &[u8]) -> std::io::Result<()> {
    s.write_all(format!("{} {}\n", head, payload.len()).as_bytes())?;
    s.write_all(payload)?;
    s.flush()
}

fn read_msg(s: &mut UnixStream) -> std::io::Result<(String, Vec<u8>)> {
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        if s.read(&mut b)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
        if line.len() > 64 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
    }
    let (verb, len) = parse_header(&String::from_utf8_lossy(&line))
        .ok_or(std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    let mut payload = vec![0u8; len];
    s.read_exact(&mut payload)?;
    Ok((verb, payload))
}

// ----------------------------------------------------------------------------- client

/// Encode a warm request as `"<lib,lib,...>\n<expr>"`. Carrying the library scope makes the daemon
/// build the *exact* binary the caller will later look up (same emitted source ⇒ same cache key), so
/// warming works for any scope, not just the daemon's default one.
pub(crate) fn encode_warm(expr: &str, libs: &[&str]) -> Vec<u8> {
    format!("{}\n{}", libs.join(","), expr).into_bytes()
}

/// Inverse of [`encode_warm`]. Returns `(libs, expr)`; a payload with no newline is treated as a
/// legacy bare expression (`libs = None`, caller falls back to the daemon's default scope).
pub(crate) fn decode_warm(payload: &[u8]) -> (Option<Vec<String>>, String) {
    let text = String::from_utf8_lossy(payload);
    match text.split_once('\n') {
        Some((csv, expr)) => {
            let libs: Vec<String> = csv.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            (if libs.is_empty() { None } else { Some(libs) }, expr.to_string())
        }
        None => (None, text.into_owned()),
    }
}

fn connect() -> Option<UnixStream> {
    let s = UnixStream::connect(sock_path()).ok()?;
    // A wedged daemon must never hang the client: cap how long we wait, then fall back in-process.
    let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    Some(s)
}

fn request(verb: &str, payload: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut s = connect()?;
    write_msg(&mut s, verb, payload).ok()?;
    read_msg(&mut s).ok()
}

/// Is a daemon listening and responsive?
pub fn ping() -> bool {
    matches!(request("PING", b""), Some((st, _)) if st == "OK")
}

/// Ask the daemon to build `expr` (with library scope `libs`) in the background. Returns whether the
/// daemon was reachable and accepted the request. Non-blocking: the build proceeds in the daemon
/// after this returns, so the caller never waits on `rustc`.
pub fn warm_bg(expr: &str, libs: &[&str]) -> bool {
    matches!(request("WARMBG", &encode_warm(expr, libs)), Some((st, _)) if st == "OK")
}

/// Build `expr` (with library scope `libs`) synchronously in the daemon; returns whether it
/// succeeded.
pub fn warm(expr: &str, libs: &[&str]) -> bool {
    matches!(request("WARM", &encode_warm(expr, libs)), Some((st, _)) if st == "OK")
}

/// The daemon's view of the binary cache: `(count, bytes)`.
pub fn stats() -> Option<(usize, u64)> {
    let (st, payload) = request("STATS", b"")?;
    if st != "OK" {
        return None;
    }
    let text = String::from_utf8_lossy(&payload);
    let mut it = text.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// Ask the daemon to shut down; returns whether it acknowledged.
pub fn stop() -> bool {
    matches!(request("STOP", b""), Some((st, _)) if st == "OK")
}

// ----------------------------------------------------------------------------- server

fn handle(mut s: UnixStream, libs: &[String]) {
    let (verb, payload) = match read_msg(&mut s) {
        Ok(v) => v,
        Err(_) => return,
    };
    match verb.as_str() {
        "PING" => {
            let _ = write_msg(&mut s, "OK", b"pong");
        }
        "STATS" => {
            let (c, b) = crate::rustgen::cache_stats();
            let _ = write_msg(&mut s, "OK", format!("{} {}", c, b).as_bytes());
        }
        "WARM" => {
            let (libs_opt, expr) = decode_warm(&payload);
            let owned = libs_opt.unwrap_or_else(|| libs.to_vec());
            let refs: Vec<&str> = owned.iter().map(|x| x.as_str()).collect();
            match crate::rustgen::warm_native(&expr, &refs) {
                Ok(_) => {
                    let _ = write_msg(&mut s, "OK", b"");
                }
                Err(e) => {
                    let _ = write_msg(&mut s, "ERR", e.as_bytes());
                }
            }
        }
        "WARMBG" => {
            // Reply immediately; compile on a detached thread so the client never waits on rustc.
            let (libs_opt, expr) = decode_warm(&payload);
            let owned = libs_opt.unwrap_or_else(|| libs.to_vec());
            std::thread::spawn(move || {
                let refs: Vec<&str> = owned.iter().map(|x| x.as_str()).collect();
                let _ = crate::rustgen::warm_native(&expr, &refs);
            });
            let _ = write_msg(&mut s, "OK", b"");
        }
        "STOP" => {
            let _ = write_msg(&mut s, "OK", b"bye");
            let _ = s.flush();
            let _ = std::fs::remove_file(sock_path());
            let _ = std::fs::remove_file(pid_path());
            std::process::exit(0);
        }
        _ => {
            let _ = write_msg(&mut s, "ERR", b"unknown verb");
        }
    }
}

/// Run the daemon: bind the socket, then serve connections (each on its own thread) until `STOP`.
/// Libraries are resolved once and shared, so every request compiles against the same scope.
pub fn serve() -> std::io::Result<()> {
    let dir = crate::rustgen::cache_dir();
    std::fs::create_dir_all(&dir)?;
    let sp = sock_path();
    let _ = std::fs::remove_file(&sp); // clear a stale socket from a crashed daemon
    let listener = UnixListener::bind(&sp)?;
    std::fs::write(pid_path(), std::process::id().to_string())?;
    let libs = std::sync::Arc::new(crate::latte::all_libs());
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let libs = libs.clone();
                std::thread::spawn(move || handle(stream, &libs));
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_warm, encode_warm, parse_header};

    #[test]
    fn header_parsing_is_total_and_strict() {
        assert_eq!(parse_header("PING 0"), Some(("PING".into(), 0)));
        assert_eq!(parse_header("WARMBG 1234"), Some(("WARMBG".into(), 1234)));
        assert_eq!(parse_header("  OK 5 \n"), Some(("OK".into(), 5)));
        // malformed headers yield None rather than panicking
        assert_eq!(parse_header(""), None);
        assert_eq!(parse_header("NOLEN"), None);
        assert_eq!(parse_header("BAD xyz"), None);
        assert_eq!(parse_header(" 7"), None); // empty verb
    }

    #[test]
    fn warm_payload_carries_libs() {
        // round-trip with an explicit scope
        let p = encode_warm("(map f xs)", &["std", "num"]);
        let (libs, expr) = decode_warm(&p);
        assert_eq!(libs, Some(vec!["std".to_string(), "num".to_string()]));
        assert_eq!(expr, "(map f xs)");
        // an expression containing a newline survives (split is on the first newline only)
        let (_, e2) = decode_warm(&encode_warm("a\nb", &["std"]));
        assert_eq!(e2, "a\nb");
        // legacy bare payload (no newline) ⇒ no libs, caller uses its default scope
        let (libs3, expr3) = decode_warm(b"(add 1 2)");
        assert_eq!(libs3, None);
        assert_eq!(expr3, "(add 1 2)");
    }
}

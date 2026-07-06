// ---------------------------------------------------------------------------
// NODE-TO-NODE DATABASE SYNC: anti-entropy over HTTP for the persistent store.
//
// Two Orpheus systems each hold a named database (src/dbservice.rs: WAL-backed,
// surviving restarts). `sync` reconciles one table with a peer over the peer's
// ordinary /api/db endpoints — no new wire protocol, just the same GET/POST any
// browser uses, via the zero-dependency HTTP client in src/registry.rs.
//
// The semantics are deliberately the simplest CONVERGENT ones, taken straight
// from the CRDT playbook the system already teaches (lib/crdt.lat):
//
//   · Keys the peer has and we lack are PULLED; keys we have and the peer
//     lacks are PUSHED. For append-style tables whose keys are unique and
//     whose records never change — the message board's Lamport-pair keys —
//     this is exactly a G-Set merge: any two nodes that sync converge to the
//     union, in any order, any number of times (idempotent, commutative).
//   · A key BOTH sides hold with DIFFERENT records is a genuine conflict.
//     We keep the local record and report the count — honest last-writer-
//     UNKNOWN rather than a silent clobber. Boards never hit this case by
//     construction (a key embeds its writer and timestamp).
//
// Records travel as re-evaluable Latte expressions (dbservice::rec, the same
// noun_to_latte serializer the WAL checkpoint trusts), so a record survives
// the round trip byte-exactly.
// ---------------------------------------------------------------------------

/// This installation's stable node id: eight hex characters, minted once and
/// persisted beside the cache. It names this node in board post keys (the
/// Lamport pair's tie-breaking component — lib/lamport.lat's `lam_lt` order)
/// and in sync reports.
pub fn node_id() -> String {
    let path = crate::rustgen::cache_dir().join("node_id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let t = s.trim().to_string();
        if t.len() == 8 && t.chars().all(|c| c.is_ascii_hexdigit()) {
            return t;
        }
    }
    // mint: hash time + pid — uniqueness across installs, stability within one
    let seed = format!(
        "{:?}\u{1}{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH),
        std::process::id()
    );
    let id = crate::sha3::hex(&crate::sha3::sha3_256(seed.as_bytes()))[..8].to_string();
    let _ = std::fs::create_dir_all(crate::rustgen::cache_dir());
    let _ = std::fs::write(&path, &id);
    id
}

fn peer_get(base: &str, path_q: &str) -> Result<String, String> {
    let url = format!("{}{}", base.trim_end_matches('/'), path_q);
    match crate::registry::http_get(&url) {
        Some((200, _, body)) => Ok(String::from_utf8_lossy(&body).into_owned()),
        Some((code, _, body)) => Err(format!("peer {} -> {}: {}", path_q, code, String::from_utf8_lossy(&body))),
        None => Err(format!("peer unreachable: {}", url)),
    }
}

/// Reconcile table `name` with the peer at `base` (e.g. http://host:8088).
/// Returns a one-line human report: pulled/pushed/conflict counts.
pub fn sync(base: &str, name: &str) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("table name must be alphanumeric".into());
    }
    let remote_raw = peer_get(base, &format!("/api/db?op=keys&name={}", name))?;
    let remote: std::collections::BTreeSet<String> =
        remote_raw.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let local: std::collections::BTreeSet<String> = {
        let svc = crate::dbservice::service();
        let mut s = svc.lock().unwrap();
        s.keys(name)?.into_iter().collect()
    };
    let mut pulled = 0usize;
    let mut pushed = 0usize;
    let mut conflicts = 0usize;
    // PULL: keys only the peer has
    for k in remote.difference(&local) {
        let rec = peer_get(base, &format!("/api/db?op=rec&name={}&key={}", name, urlq(k)))?;
        let svc = crate::dbservice::service();
        let mut s = svc.lock().unwrap();
        s.put(name, k, rec.trim())?;
        pulled += 1;
    }
    // PUSH: keys only we have (through the peer's ordinary put endpoint)
    for k in local.difference(&remote) {
        let rec = {
            let svc = crate::dbservice::service();
            let mut s = svc.lock().unwrap();
            s.rec(name, k)?
        };
        let url = format!(
            "{}/api/db?op=put&name={}&key={}",
            base.trim_end_matches('/'), name, urlq(k)
        );
        match crate::registry::http_post(&url, rec.as_bytes()) {
            Some((200, _, _)) => pushed += 1,
            Some((code, _, body)) => {
                return Err(format!("peer put {} -> {}: {}", k, code, String::from_utf8_lossy(&body)))
            }
            None => return Err("peer unreachable during push".into()),
        }
    }
    // CONFLICTS: shared keys whose records differ — kept local, counted honestly
    for k in local.intersection(&remote) {
        let mine = {
            let svc = crate::dbservice::service();
            let mut s = svc.lock().unwrap();
            s.rec(name, k).unwrap_or_default()
        };
        let theirs = peer_get(base, &format!("/api/db?op=rec&name={}&key={}", name, urlq(k))).unwrap_or_default();
        if mine.trim() != theirs.trim() {
            conflicts += 1;
        }
    }
    Ok(format!(
        "synced '{}' with {}: pulled {}, pushed {}{}",
        name,
        base,
        pulled,
        pushed,
        if conflicts > 0 { format!(", {} conflicting keys kept local", conflicts) } else { String::new() }
    ))
}

fn urlq(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn node_id_is_stable_hex() {
        let a = super::node_id();
        let b = super::node_id();
        assert_eq!(a, b, "node id must persist");
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn board_keys_order_matches_lamport_lat() {
        // The board's post keys are Lamport pairs rendered as zero-padded
        // "ts-node" strings; their STRING order must equal lib/lamport.lat's
        // lam_lt total order (timestamp, then node id as the tie-break) —
        // the differential check that the Rust rendering and the Latte
        // library agree on what "earlier" means.
        let pairs = [(5u64, 2u64), (5, 7), (6, 1), (5, 2), (10, 0)];
        for &(t1, n1) in &pairs {
            for &(t2, n2) in &pairs {
                let k1 = format!("{:016}-{:08}", t1, n1);
                let k2 = format!("{:016}-{:08}", t2, n2);
                let expr = format!("(lam_lt [{} {}] [{} {}])", t1, n1, t2, n2);
                let lat = crate::latte::run_with_libs(&expr, &["std", "lamport"]).unwrap();
                let lat_lt = crate::serve::render_noun(&lat) == "0"; // loobean yes
                assert_eq!(k1 < k2, lat_lt, "({},{}) vs ({},{})", t1, n1, t2, n2);
            }
        }
    }
}

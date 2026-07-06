//! The LEDGER — the GUI's persistent, shared, gossiped state.
//!
//! One kv-agent Node (src/net.rs + src/agent.rs) lives inside the GUI server.
//! Every `Kv.put` from a page or the System console becomes a durable event in
//! its append-only log, gossiped to every connected peer; two Orpheus
//! instances pointed at each other (over a LAN or the open Internet) converge
//! on byte-identical state — strong eventual consistency, surfaced as an
//! interactive page (`/network`) instead of a CLI.
//!
//! The ledger is deliberately the SAME machinery `latte node` runs: the same
//! agent, the same wire protocol, the same durable store. A GUI instance and
//! a bare CLI node are full peers of one another.

use crate::agent::Agent;
use crate::knot::{cell, cord, num, Knot, N};
use crate::net;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Ledger {
    pub node: net::NodeHandle,
    pub peers: net::Peers,
    pub cfg: Arc<net::Config>,
    pub listen: String,
    store: Option<String>,
    /// Peers dialled at runtime: address → the connector's alive flag
    /// (clearing it is how `forget` stops the retry loop).
    dyn_peers: Mutex<Vec<(String, Arc<std::sync::atomic::AtomicBool>)>>,
}

/// Runtime-connected peers persist here (one address per line), so a GUI
/// restart redials them — `Kv.connect` is a durable decision, `Kv.forget`
/// its undo.
fn peers_path() -> std::path::PathBuf {
    crate::rustgen::cache_dir().join("ledger-peers")
}

fn saved_peers() -> Vec<String> {
    std::fs::read_to_string(peers_path())
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn save_peers(addrs: &[String]) {
    let _ = std::fs::create_dir_all(crate::rustgen::cache_dir());
    let body = if addrs.is_empty() { String::new() } else { addrs.join("\n") + "\n" };
    let _ = std::fs::write(peers_path(), body);
}

static LEDGER: OnceLock<Ledger> = OnceLock::new();

/// Start the ledger (idempotent — the first call wins, later calls report the
/// existing one). `listen` empty means "do not listen" (still able to dial
/// out); peers are dialled with retry-forever connectors, so an offline peer
/// is adopted the moment it appears.
pub fn init(store: Option<&str>, listen: &str, peers: &[String], id: Option<u64>) -> Result<String, String> {
    if LEDGER.get().is_some() {
        return Ok(describe());
    }
    let agent = Agent::new_kv()?;
    let id = id.unwrap_or_else(|| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        t ^ (std::process::id() as u64).wrapping_mul(2654435761)
    });
    let node = match store {
        Some(dir) => net::Node::open(id, agent, dir, 16).map_err(|e| format!("ledger store {}: {}", dir, e))?,
        None => net::Node::new(id, agent),
    };
    let node: net::NodeHandle = Arc::new(Mutex::new(node));
    let cfg = Arc::new(net::Config {
        name: "ledger".into(),
        listen: listen.to_string(),
        peers: peers.to_vec(),
        verbose: false,
        compact_every: 0,
    });
    // net::start binds the listener (when given), launches the startup peer
    // connectors, and runs the anti-entropy sweep. With an empty listen it
    // logs a bind failure and the rest still runs; avoid the noise by only
    // starting the listener machinery when there is something to do.
    let peers_handle = if listen.is_empty() && peers.is_empty() {
        Arc::new(Mutex::new(Vec::new()))
    } else {
        net::start(node.clone(), cfg.clone())
    };
    let ledger = Ledger {
        node,
        peers: peers_handle,
        cfg,
        listen: listen.to_string(),
        store: store.map(|s| s.to_string()),
        dyn_peers: Mutex::new(Vec::new()),
    };
    let _ = LEDGER.set(ledger);
    // Redial the peers earlier sessions connected (only when this ledger is
    // networked — an in-memory, listen-less ledger, e.g. under test, stays
    // quiet). `connect` below persists new ones.
    if !listen.is_empty() {
        for addr in saved_peers() {
            if let Some(l) = LEDGER.get() {
                if !l.cfg.peers.iter().any(|x| *x == addr) {
                    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
                    l.dyn_peers.lock().unwrap().push((addr.clone(), alive.clone()));
                    net::connect_peer_cancellable(l.node.clone(), l.peers.clone(), l.cfg.clone(), addr, alive);
                }
            }
        }
    }
    Ok(describe())
}

/// A generation stamp that changes whenever the ledger's event set changes —
/// local pokes AND gossiped arrivals alike. Folded into Facet's render/eval
/// memo keys, so a page showing ledger state is re-rendered the moment a
/// peer's event lands, and never before.
pub fn generation() -> u64 {
    match LEDGER.get() {
        None => 0,
        Some(l) => {
            let n = l.node.lock().unwrap();
            (n.event_count() as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (n.lamport << 1) ^ n.id
        }
    }
}

fn describe() -> String {
    match LEDGER.get() {
        None => "no ledger".into(),
        Some(l) => {
            let n = l.node.lock().unwrap();
            format!(
                "ledger node id={:x} listen={} events={} store={}",
                n.id,
                if l.listen.is_empty() { "-" } else { &l.listen },
                n.event_count(),
                l.store.as_deref().unwrap_or("(memory)")
            )
        }
    }
}

// ------------------------------- values --------------------------------------

/// A page/console value → a TAGGED kv noun: `[%n 42]` for a decimal,
/// `[%t "hello"]` for text. Atoms alone cannot say whether they are numbers
/// or cords (42 and "*" are the same noun), so the ledger stores which was
/// meant — values describe themselves, and display is unambiguous.
pub fn text_to_noun(s: &str) -> N {
    match s.trim().parse::<u128>() {
        Ok(v) => cell(cord("n"), num(v)),
        Err(_) => cell(cord("t"), cord(s.trim())),
    }
}

/// Render a kv noun for people. The ledger's own tagged values (`[%n v]`,
/// `[%t c]`) render exactly as entered; raw values (a CLI node's `--do "put
/// k 5"` gossips untagged nouns) fall back to the printable-cord-else-decimal
/// heuristic; other cells use the system's noun notation.
pub fn show_noun(n: &N) -> String {
    if let Some((tag, v)) = n.as_cell() {
        match tag.as_atom().and_then(|a| a.as_cord()).as_deref() {
            Some("n") => {
                if let Some(x) = v.as_atom().and_then(|a| a.to_u128()) {
                    return x.to_string();
                }
            }
            Some("t") => {
                if let Some(s) = v.as_atom().and_then(|a| a.as_cord()) {
                    return s;
                }
                if v.as_atom().map(|a| a.is_zero()).unwrap_or(false) {
                    return String::new();
                }
            }
            _ => {}
        }
    }
    match &**n {
        Knot::Atom(a) => match a.as_cord() {
            Some(s) if !s.is_empty() => s,
            _ => a.to_u128().map(|v| v.to_string()).unwrap_or_else(|| format!("{:?}", a)),
        },
        Knot::Cell(_, _) => format!("{:?}", n),
    }
}

// ------------------------------- operations ----------------------------------

fn require() -> Result<&'static Ledger, String> {
    LEDGER
        .get()
        .ok_or_else(|| "no ledger node in this process — start the GUI (latte gui) to host one".into())
}

/// Durable, gossiped `put`: one event in the log, pushed to every peer.
pub fn put(key: &str, val: &str) -> Result<String, String> {
    let l = require()?;
    if key.trim().is_empty() {
        return Err("Kv.put: the key is empty".into());
    }
    let action = cell(cord("put"), cell(cord(key.trim()), text_to_noun(val)));
    net::submit(&l.node, &l.peers, action);
    let n = l.node.lock().unwrap();
    Ok(format!("put {} = {}  (event {}, gossiped to {} peer link(s))", key.trim(), val.trim(), n.event_count(), l.peers.lock().unwrap().len()))
}

/// Durable, gossiped `del`.
pub fn del(key: &str) -> Result<String, String> {
    let l = require()?;
    if key.trim().is_empty() {
        return Err("Kv.del: the key is empty".into());
    }
    let action = cell(cord("del"), cord(key.trim()));
    net::submit(&l.node, &l.peers, action);
    Ok(format!("deleted {}", key.trim()))
}

/// The current state as (key, value) rows, insertion-order newest first.
pub fn state_rows() -> Result<Vec<(String, String)>, String> {
    let l = require()?;
    let n = l.node.lock().unwrap();
    let st = n.state().map_err(|e| format!("{:?}", e))?;
    Ok(assoc_rows(&st))
}

/// TIME TRAVEL: the state as of the first `k` events in total order — event
/// sourcing gives this for free, and the GUI turns it into a slider.
pub fn state_at_rows(k: usize) -> Result<(usize, usize, Vec<(String, String)>), String> {
    let l = require()?;
    let n = l.node.lock().unwrap();
    let total = n.event_count();
    let k = k.min(total);
    let st = n.state_at(k).map_err(|e| format!("{:?}", e))?;
    Ok((k, total, assoc_rows(&st)))
}

fn assoc_rows(st: &N) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur = st.clone();
    while let Knot::Cell(pair, rest) = &*cur {
        if let Some((k, v)) = pair.as_cell() {
            out.push((show_noun(k), show_noun(v)));
        }
        cur = rest.clone();
    }
    out
}

/// The last `n` events (lamport, node, action) — the shared history itself.
pub fn log_rows(n: usize) -> Result<Vec<(u64, u64, String)>, String> {
    let l = require()?;
    let node = l.node.lock().unwrap();
    let mut rows: Vec<(u64, u64, String)> = node
        .events
        .iter()
        .map(|((lam, nid, _), ev)| {
            let act = net::Event::from_knot(ev).map(|e| show_action(&e.action)).unwrap_or_else(|| "?".into());
            (*lam, *nid, act)
        })
        .collect();
    let keep = rows.len().saturating_sub(n);
    rows.drain(..keep);
    rows.reverse(); // newest first
    Ok(rows)
}

fn show_action(a: &N) -> String {
    // kv actions: [%put [k v]] | [%del k] | [%clear 0]
    if let Some((tag, rest)) = a.as_cell() {
        if let Some(t) = tag.as_atom().and_then(|x| x.as_cord()) {
            return match t.as_str() {
                "put" => match rest.as_cell() {
                    Some((k, v)) => format!("put {} = {}", show_noun(k), show_noun(v)),
                    None => "put ?".into(),
                },
                "del" => format!("del {}", show_noun(rest)),
                other => format!("{} {}", other, show_noun(rest)),
            };
        }
    }
    format!("{:?}", a)
}

/// Connect to one more peer at runtime. The connector retries forever, so an
/// address that is offline now is adopted the moment it appears.
pub fn connect(addr: &str) -> Result<String, String> {
    let l = require()?;
    let addr = addr.trim();
    if addr.is_empty()
        || !addr.contains(':')
        || !addr.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
    {
        return Err("Kv.connect: give a peer as host:port (e.g. 203.0.113.7:9600)".into());
    }
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let mut d = l.dyn_peers.lock().unwrap();
        if d.iter().any(|(a, _)| a == addr) || l.cfg.peers.iter().any(|x| x == addr) {
            return Ok(format!("already connecting to {} (connectors retry forever)", addr));
        }
        d.push((addr.to_string(), alive.clone()));
        // persist (networked ledgers only): a restart redials this peer
        if !l.listen.is_empty() {
            save_peers(&d.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>());
        }
    }
    net::connect_peer_cancellable(l.node.clone(), l.peers.clone(), l.cfg.clone(), addr.to_string(), alive);
    Ok(format!(
        "connecting to {} — the link retries until the peer appears, then the logs reconcile automatically (persists across restarts; Kv.forget undoes it)",
        addr
    ))
}

/// Undo a `connect`: stop the retry loop and drop the address from the
/// persisted list. An already-open link lives until it drops on its own.
pub fn forget(addr: &str) -> Result<String, String> {
    let l = require()?;
    let addr = addr.trim();
    let mut d = l.dyn_peers.lock().unwrap();
    let before = d.len();
    for (a, alive) in d.iter() {
        if a == addr {
            alive.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
    d.retain(|(a, _)| a != addr);
    if !l.listen.is_empty() {
        save_peers(&d.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>());
    }
    if d.len() == before {
        if l.cfg.peers.iter().any(|x| x == addr) {
            return Ok(format!("{} was given at startup (--kv-peer) — restart without the flag to drop it", addr));
        }
        return Ok(format!("{} was not a dialled peer", addr));
    }
    Ok(format!("forgot {} — no more reconnection attempts (an open link, if any, lapses on its own)", addr))
}

/// The training-data reader: ledger keys beginning with `prefix`, each value
/// a point "x, y" (or "x y") — the shared, gossiped dataset any connected
/// instance can contribute to.
pub fn data_points(prefix: &str) -> Result<Vec<(String, String)>, String> {
    let rows = state_rows()?;
    Ok(rows.into_iter().filter(|(k, _)| k.starts_with(prefix)).collect())
}

/// Peer addresses this node dials (startup + runtime) and how many links are live.
pub fn peers_info() -> Result<(Vec<String>, usize), String> {
    let l = require()?;
    let mut addrs = l.cfg.peers.clone();
    addrs.extend(l.dyn_peers.lock().unwrap().iter().map(|(a, _)| a.clone()));
    let live = l.peers.lock().unwrap().len();
    Ok((addrs, live))
}

/// One-line identity + vitals for the info panel.
pub fn info_lines() -> Result<Vec<(String, String)>, String> {
    let l = require()?;
    let n = l.node.lock().unwrap();
    let (addrs, live) = {
        let mut a = l.cfg.peers.clone();
        a.extend(l.dyn_peers.lock().unwrap().iter().map(|(x, _)| x.clone()));
        (a, l.peers.lock().unwrap().len())
    };
    Ok(vec![
        ("node id".into(), format!("{:x}", n.id)),
        ("agent".into(), "kv (lib-defined key-value store, entirely in Latte)".into()),
        (
            "listening".into(),
            if l.listen.is_empty() { "no (dial-out only)".into() } else { l.listen.clone() },
        ),
        ("events".into(), n.event_count().to_string()),
        ("lamport".into(), n.lamport.to_string()),
        (
            "store".into(),
            l.store.clone().unwrap_or_else(|| "in-memory (pass --kv-store DIR for durability)".into()),
        ),
        ("peers dialled".into(), if addrs.is_empty() { "none".into() } else { addrs.join(", ") }),
        ("live links".into(), live.to_string()),
    ])
}

/// Ledger-touching tests share one process-wide node — they serialize on
/// this guard so history assertions see only their own events.
#[cfg(test)]
pub fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_put_state_timetravel_and_log() {
        // one shared global per test process: initialize in-memory, no
        // network — and serialize with the other ledger-touching tests
        let _g = test_guard();
        init(None, "", &[], Some(7)).unwrap();
        let t0 = state_at_rows(usize::MAX).unwrap().1; // events already present
        let g0 = generation();
        put("greeting", "hello").unwrap();
        put("answer", "42").unwrap();
        assert_ne!(g0, generation(), "events move the generation stamp");
        let rows = state_rows().unwrap();
        assert!(rows.iter().any(|(k, v)| k == "greeting" && v == "hello"));
        assert!(rows.iter().any(|(k, v)| k == "answer" && v == "42"));
        // time travel, RELATIVE to whatever history preceded this test:
        // after our first event, greeting exists and answer does not yet
        let (_k, total, at1) = state_at_rows(t0 + 1).unwrap();
        assert!(total >= t0 + 2);
        assert!(at1.iter().any(|(k, _)| k == "greeting"));
        assert!(!at1.iter().any(|(k, _)| k == "answer"));
        // the log shows both actions, newest first
        let log = log_rows(10).unwrap();
        assert!(log[0].2.contains("answer"), "newest first: {:?}", log);
        del("greeting").unwrap();
        let rows = state_rows().unwrap();
        assert!(!rows.iter().any(|(k, _)| k == "greeting"));
        // values render for people
        assert_eq!(show_noun(&text_to_noun("hello")), "hello");
        assert_eq!(show_noun(&text_to_noun("42")), "42");
        // dial-out bookkeeping: connect records, forget cancels — and with a
        // listen-less (test) ledger nothing is persisted to the cache
        let saved_before = super::saved_peers();
        connect("127.0.0.1:1").unwrap();
        let (addrs, _) = peers_info().unwrap();
        assert!(addrs.iter().any(|a| a == "127.0.0.1:1"));
        assert_eq!(super::saved_peers(), saved_before, "listen-less ledgers do not write the peers file");
        let msg = forget("127.0.0.1:1").unwrap();
        assert!(msg.contains("forgot"), "{}", msg);
        let (addrs, _) = peers_info().unwrap();
        assert!(!addrs.iter().any(|a| a == "127.0.0.1:1"));
        // the shared-dataset reader filters by prefix
        put("pt.a", "1, 3.0").unwrap();
        put("pt.b", "2, 5.1").unwrap();
        put("other", "9").unwrap();
        let pts = data_points("pt.").unwrap();
        assert_eq!(pts.len(), 2, "{:?}", pts);
        assert!(pts.iter().all(|(k, _)| k.starts_with("pt.")));
    }
}

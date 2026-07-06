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
    dyn_peers: Mutex<Vec<String>>,
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
    {
        let mut d = l.dyn_peers.lock().unwrap();
        if d.iter().any(|x| x == addr) || l.cfg.peers.iter().any(|x| x == addr) {
            return Ok(format!("already connecting to {} (connectors retry forever)", addr));
        }
        d.push(addr.to_string());
    }
    net::connect_peer(l.node.clone(), l.peers.clone(), l.cfg.clone(), addr.to_string());
    Ok(format!(
        "connecting to {} — the link retries until the peer appears, then the logs reconcile automatically",
        addr
    ))
}

/// Peer addresses this node dials (startup + runtime) and how many links are live.
pub fn peers_info() -> Result<(Vec<String>, usize), String> {
    let l = require()?;
    let mut addrs = l.cfg.peers.clone();
    addrs.extend(l.dyn_peers.lock().unwrap().iter().cloned());
    let live = l.peers.lock().unwrap().len();
    Ok((addrs, live))
}

/// One-line identity + vitals for the info panel.
pub fn info_lines() -> Result<Vec<(String, String)>, String> {
    let l = require()?;
    let n = l.node.lock().unwrap();
    let (addrs, live) = {
        let mut a = l.cfg.peers.clone();
        a.extend(l.dyn_peers.lock().unwrap().iter().cloned());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_put_state_timetravel_and_log() {
        // one shared global per test process: initialize in-memory, no network
        init(None, "", &[], Some(7)).unwrap();
        let g0 = generation();
        put("greeting", "hello").unwrap();
        put("answer", "42").unwrap();
        assert_ne!(g0, generation(), "events move the generation stamp");
        let rows = state_rows().unwrap();
        assert!(rows.iter().any(|(k, v)| k == "greeting" && v == "hello"));
        assert!(rows.iter().any(|(k, v)| k == "answer" && v == "42"));
        // time travel: after the first event only greeting exists
        let (_k, total, at1) = state_at_rows(1).unwrap();
        assert!(total >= 2);
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
    }
}

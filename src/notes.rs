//! COLLABORATIVE NOTES — the host side of lib/notes.lat.
//!
//! One notes-agent Node lives in the GUI process (a sibling of the ledger,
//! src/ledger.rs, on its own port): every operation — make a note, insert a
//! block after an anchor, replace a block's text, tombstone it — is a durable
//! event, gossiped to every connected peer and folded in the agreed total
//! order by the PURE Latte agent. Convergence is therefore the log's;
//! intention preservation (concurrent edits to different blocks both
//! survive; anchored insertion keeps concurrent runs contiguous; tombstones
//! keep anchors valid under concurrent deletion) is the agent's. This module
//! mints the globally unique block ids ([lamport node], shown to people as
//! "LAM-NODEHEX"), turns page/console text into agent actions, and reads the
//! folded state back out for the two interfaces: the Note.* tools
//! (src/facet.rs) and the live editor (lib/site/notes.html).

use crate::agent::Agent;
use crate::knot::{cell, cord, num, Knot, N};
use crate::net;
use std::sync::{Arc, Mutex, OnceLock};

pub struct NotesHost {
    pub node: net::NodeHandle,
    pub peers: net::Peers,
    pub cfg: Arc<net::Config>,
    pub listen: String,
    store: Option<String>,
    dyn_peers: Mutex<Vec<(String, Arc<std::sync::atomic::AtomicBool>)>>,
}

static NOTES: OnceLock<NotesHost> = OnceLock::new();

fn peers_path() -> std::path::PathBuf {
    crate::rustgen::cache_dir().join("notes-peers")
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

/// Start the notes node (idempotent — first call wins).
pub fn init(store: Option<&str>, listen: &str, peers: &[String], id: Option<u64>) -> Result<String, String> {
    if NOTES.get().is_some() {
        return Ok(describe());
    }
    let agent = Agent::new_notes()?;
    let id = id.unwrap_or_else(|| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        t ^ (std::process::id() as u64).wrapping_mul(0x9E3779B9)
    });
    let node = match store {
        Some(dir) => net::Node::open(id, agent, dir, 16).map_err(|e| format!("notes store {}: {}", dir, e))?,
        None => net::Node::new(id, agent),
    };
    let node: net::NodeHandle = Arc::new(Mutex::new(node));
    let cfg = Arc::new(net::Config {
        name: "notes".into(),
        listen: listen.to_string(),
        peers: peers.to_vec(),
        verbose: false,
        compact_every: 0,
    });
    let peers_handle = if listen.is_empty() && peers.is_empty() {
        Arc::new(Mutex::new(Vec::new()))
    } else {
        net::start(node.clone(), cfg.clone())
    };
    let host = NotesHost {
        node,
        peers: peers_handle,
        cfg,
        listen: listen.to_string(),
        store: store.map(|s| s.to_string()),
        dyn_peers: Mutex::new(Vec::new()),
    };
    let _ = NOTES.set(host);
    if !listen.is_empty() {
        for addr in saved_peers() {
            if let Some(h) = NOTES.get() {
                if !h.cfg.peers.iter().any(|x| *x == addr) {
                    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
                    h.dyn_peers.lock().unwrap().push((addr.clone(), alive.clone()));
                    net::connect_peer_cancellable(h.node.clone(), h.peers.clone(), h.cfg.clone(), addr, alive);
                }
            }
        }
    }
    Ok(describe())
}

fn require() -> Result<&'static NotesHost, String> {
    NOTES
        .get()
        .ok_or_else(|| "no notes node in this process — start the GUI (latte gui) to host one".into())
}

fn describe() -> String {
    match NOTES.get() {
        None => "no notes node".into(),
        Some(h) => {
            let n = h.node.lock().unwrap();
            format!(
                "notes node id={:x} listen={} events={} store={}",
                n.id,
                if h.listen.is_empty() { "-" } else { &h.listen },
                n.event_count(),
                h.store.as_deref().unwrap_or("(memory)")
            )
        }
    }
}

/// A generation stamp for Facet's memo keys: moves on every event, local or
/// gossiped.
pub fn generation() -> u64 {
    match NOTES.get() {
        None => 0,
        Some(h) => {
            let n = h.node.lock().unwrap();
            (n.event_count() as u64).wrapping_mul(0xA24B_AED4_963E_E407) ^ (n.lamport << 1) ^ n.id
        }
    }
}

// ------------------------------- block ids -----------------------------------

/// Mint a globally unique block id: this node's next lamport paired with its
/// node id. Distinct instances can never collide (the node id differs);
/// one instance never reuses a lamport for two mints (the submit that follows
/// each mint advances it).
fn mint_bid(h: &NotesHost) -> N {
    let n = h.node.lock().unwrap();
    cell(num((n.lamport + 1) as u128), num(n.id as u128))
}

/// "LAM-NODEHEX" — the human/wire form of a block id.
pub fn bid_string(bid: &N) -> String {
    match bid.as_cell() {
        Some((l, n)) => format!(
            "{}-{:x}",
            l.as_atom().and_then(|a| a.to_u128()).unwrap_or(0),
            n.as_atom().and_then(|a| a.to_u128()).unwrap_or(0)
        ),
        None => "0-0".into(),
    }
}

pub fn parse_bid(s: &str) -> Option<N> {
    let (l, n) = s.trim().split_once('-')?;
    Some(cell(num(l.parse::<u128>().ok()?), num(u128::from_str_radix(n, 16).ok()?)))
}

// ------------------------------- operations ----------------------------------

fn submit(h: &NotesHost, action: N) {
    net::submit(&h.node, &h.peers, action);
}

/// Document KINDS ride the one agent as id prefixes — the same blocks,
/// anchors, and tombstones carry prose, economy specs, ballots, and code:
///   n = note · p = economic plan spec · v = quadratic-vote ballots · c = code
pub const KINDS: &[(&str, &str)] =
    &[("n", "note"), ("p", "plan"), ("v", "votes"), ("c", "code"), ("s", "language"), ("d", "drawing")];

/// Create a document of the given kind; returns its id ("KLAM-NODEHEX").
pub fn create_kind(kind: &str, title: &str) -> Result<String, String> {
    let h = require()?;
    if title.trim().is_empty() {
        return Err("give the document a title".into());
    }
    let k = kind.trim();
    if !KINDS.iter().any(|(p, _)| *p == k) {
        return Err(format!("unknown document kind '{}' (n=note, p=plan, v=votes, c=code)", k));
    }
    let id = {
        let n = h.node.lock().unwrap();
        format!("{}{}-{:x}", k, n.lamport + 1, n.id)
    };
    submit(h, cell(cord("mknote"), cell(cord(&id), cord(title.trim()))));
    Ok(id)
}

/// Create a plain note (kind "n").
pub fn create(title: &str) -> Result<String, String> {
    create_kind("n", title)
}

/// The document's LIVE text: blocks in order, joined by newlines — what the
/// planners parse and the code tools compile. Tombstones excluded.
pub fn assemble(id: &str) -> Result<Option<String>, String> {
    Ok(read_note(id, 0)?.map(|(_, blocks)| {
        blocks
            .into_iter()
            .filter(|b| b.alive)
            .map(|b| b.text)
            .collect::<Vec<_>>()
            .join("\n")
    }))
}

pub fn retitle(id: &str, title: &str) -> Result<(), String> {
    let h = require()?;
    submit(h, cell(cord("title"), cell(cord(id.trim()), cord(title.trim()))));
    Ok(())
}

pub fn remove(id: &str) -> Result<(), String> {
    let h = require()?;
    submit(h, cell(cord("rmnote"), cord(id.trim())));
    Ok(())
}

/// Insert a block after `anchor` ("" or "0" = at the head); returns the new
/// block's id string.
pub fn insert_after(id: &str, anchor: &str, author: &str, text: &str) -> Result<String, String> {
    let h = require()?;
    let bid = mint_bid(h);
    let bs = bid_string(&bid);
    let anchor_n = match anchor.trim() {
        "" | "0" => num(0),
        a => parse_bid(a).ok_or_else(|| format!("'{}' is not a block id (LAM-NODEHEX)", a))?,
    };
    let action = cell(
        cord("ins"),
        cell(
            cord(id.trim()),
            cell(anchor_n, cell(bid, cell(cord(author.trim()), cord(text)))),
        ),
    );
    submit(h, action);
    Ok(bs)
}

/// Append a block at the end of a note (the common case): anchored to the
/// current last block, so concurrent appends from two instances form two
/// contiguous runs rather than an interleaved shuffle.
pub fn append_block(id: &str, author: &str, text: &str) -> Result<String, String> {
    let last = read_note(id, 0)?
        .map(|(_, blocks)| blocks.last().map(|b| b.bid.clone()))
        .flatten()
        .unwrap_or_default();
    insert_after(id, &last, author, text)
}

pub fn set_text(id: &str, bid: &str, author: &str, text: &str) -> Result<(), String> {
    let h = require()?;
    let b = parse_bid(bid).ok_or_else(|| format!("'{}' is not a block id (LAM-NODEHEX)", bid))?;
    submit(
        h,
        cell(cord("set"), cell(cord(id.trim()), cell(b, cell(cord(author.trim()), cord(text))))),
    );
    Ok(())
}

pub fn del_block(id: &str, bid: &str) -> Result<(), String> {
    let h = require()?;
    let b = parse_bid(bid).ok_or_else(|| format!("'{}' is not a block id (LAM-NODEHEX)", bid))?;
    submit(h, cell(cord("del"), cell(cord(id.trim()), b)));
    Ok(())
}

// ------------------------------- reading -------------------------------------

pub struct Block {
    pub bid: String,
    pub alive: bool,
    pub author: String,
    pub text: String,
}

fn cord_of(n: &N) -> String {
    n.as_atom().and_then(|a| a.as_cord()).unwrap_or_default()
}

fn state_of(h: &NotesHost, at: usize) -> Result<N, String> {
    let n = h.node.lock().unwrap();
    if at == 0 {
        n.state().map_err(|e| format!("{:?}", e))
    } else {
        n.state_at(at.min(n.event_count())).map_err(|e| format!("{:?}", e))
    }
}

/// All notes: (id, title, live-block count). `at` 0 = present, else the state
/// as of the first `at` events (time travel).
pub fn list_notes(at: usize) -> Result<Vec<(String, String, usize)>, String> {
    let h = require()?;
    let st = state_of(h, at)?;
    let mut out = Vec::new();
    let mut cur = st;
    while let Knot::Cell(pair, rest) = &*cur.clone() {
        if let Some((id, note)) = pair.as_cell() {
            if let Some((title, blocks)) = note.as_cell() {
                let live = blocks_of(blocks).iter().filter(|b| b.alive).count();
                out.push((cord_of(id), cord_of(title), live));
            }
        }
        cur = rest.clone();
    }
    Ok(out)
}

/// One note: (title, blocks in document order — tombstones included, marked).
pub fn read_note(id: &str, at: usize) -> Result<Option<(String, Vec<Block>)>, String> {
    let h = require()?;
    let st = state_of(h, at)?;
    let mut cur = st;
    while let Knot::Cell(pair, rest) = &*cur.clone() {
        if let Some((nid, note)) = pair.as_cell() {
            if cord_of(nid) == id.trim() {
                if let Some((title, blocks)) = note.as_cell() {
                    return Ok(Some((cord_of(title), blocks_of(blocks))));
                }
            }
        }
        cur = rest.clone();
    }
    Ok(None)
}

fn blocks_of(blocks: &N) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur = blocks.clone();
    while let Knot::Cell(blk, rest) = &*cur.clone() {
        // block = [ bid [ alive [ author text ] ] ]
        if let Some((bid, r1)) = blk.as_cell() {
            if let Some((alive, r2)) = r1.as_cell() {
                if let Some((author, text)) = r2.as_cell() {
                    out.push(Block {
                        bid: bid_string(bid),
                        alive: alive.as_atom().and_then(|a| a.to_u128()) == Some(1),
                        author: cord_of(author),
                        text: cord_of(text),
                    });
                }
            }
        }
        cur = rest.clone();
    }
    out
}

pub fn event_count() -> usize {
    NOTES.get().map(|h| h.node.lock().unwrap().event_count()).unwrap_or(0)
}

// ------------------------------- peers ----------------------------------------

pub fn connect(addr: &str) -> Result<String, String> {
    let h = require()?;
    let addr = addr.trim();
    if addr.is_empty()
        || !addr.contains(':')
        || !addr.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
    {
        return Err("Note.connect: give a peer as host:port (the peer's notes port, 9601 by default)".into());
    }
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let mut d = h.dyn_peers.lock().unwrap();
        if d.iter().any(|(a, _)| a == addr) || h.cfg.peers.iter().any(|x| x == addr) {
            return Ok(format!("already connecting to {} (connectors retry forever)", addr));
        }
        d.push((addr.to_string(), alive.clone()));
        if !h.listen.is_empty() {
            save_peers(&d.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>());
        }
    }
    net::connect_peer_cancellable(h.node.clone(), h.peers.clone(), h.cfg.clone(), addr.to_string(), alive);
    Ok(format!(
        "connecting notes to {} — the link retries until the peer appears (persists across restarts; Note.forget undoes it)",
        addr
    ))
}

pub fn forget(addr: &str) -> Result<String, String> {
    let h = require()?;
    let addr = addr.trim();
    let mut d = h.dyn_peers.lock().unwrap();
    let before = d.len();
    for (a, alive) in d.iter() {
        if a == addr {
            alive.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
    d.retain(|(a, _)| a != addr);
    if !h.listen.is_empty() {
        save_peers(&d.iter().map(|(a, _)| a.clone()).collect::<Vec<_>>());
    }
    if d.len() == before {
        if h.cfg.peers.iter().any(|x| x == addr) {
            return Ok(format!("{} was given at startup (--notes-peer) — restart without the flag to drop it", addr));
        }
        return Ok(format!("{} was not a dialled peer", addr));
    }
    Ok(format!("forgot {}", addr))
}

pub fn info_lines() -> Result<Vec<(String, String)>, String> {
    let h = require()?;
    let n = h.node.lock().unwrap();
    let (addrs, live) = {
        let mut a = h.cfg.peers.clone();
        a.extend(h.dyn_peers.lock().unwrap().iter().map(|(x, _)| x.clone()));
        (a, h.peers.lock().unwrap().len())
    };
    Ok(vec![
        ("node id".into(), format!("{:x}", n.id)),
        ("agent".into(), "notes (block-sequence documents, entirely in Latte — lib/notes.lat)".into()),
        (
            "listening".into(),
            if h.listen.is_empty() { "no (dial-out only)".into() } else { h.listen.clone() },
        ),
        ("events".into(), n.event_count().to_string()),
        (
            "store".into(),
            h.store.clone().unwrap_or_else(|| "in-memory (pass --notes-store DIR for durability)".into()),
        ),
        ("peers dialled".into(), if addrs.is_empty() { "none".into() } else { addrs.join(", ") }),
        ("live links".into(), live.to_string()),
    ])
}

/// Notes-touching tests share one process-wide node — they serialize here.
#[cfg(test)]
pub fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_edits_from_two_nodes_merge_with_intention_preserved() {
        // Two LIVE notes nodes over real TCP editing the same note
        // concurrently. This exercises the raw agent + log machinery (the
        // global host above stays untouched): both replicas must converge to
        // identical state, both writers' block runs must survive contiguously,
        // and an insert anchored to a concurrently-deleted block must stay in
        // place (the tombstone holds the position).
        let mk = |id: u64| Arc::new(Mutex::new(net::Node::new(id, Agent::new_notes().unwrap())));
        let a = mk(51);
        let b = mk(52);
        let la = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = la.local_addr().unwrap().to_string();
        drop(la);
        let cfg_a = Arc::new(net::Config { name: "NA".into(), listen: addr.clone(), peers: vec![], verbose: false, compact_every: 0 });
        let cfg_b = Arc::new(net::Config { name: "NB".into(), listen: String::new(), peers: vec![], verbose: false, compact_every: 0 });
        let pa = net::start(a.clone(), cfg_a);
        let pb: net::Peers = Arc::new(Mutex::new(Vec::new()));

        // helpers speaking directly to a chosen node
        let bid_of = |node: &net::NodeHandle| {
            let n = node.lock().unwrap();
            cell(num((n.lamport + 1) as u128), num(n.id as u128))
        };
        let ins = |node: &net::NodeHandle, peers: &net::Peers, anchor: &N, who: &str, t: &str| -> N {
            let bid = bid_of(node);
            net::submit(
                node,
                peers,
                cell(cord("ins"), cell(cord("doc"), cell(anchor.clone(), cell(bid.clone(), cell(cord(who), cord(t)))))),
            );
            bid
        };
        let texts = |node: &net::NodeHandle| -> Vec<String> {
            let st = node.lock().unwrap().state().unwrap();
            let mut cur = st;
            while let Knot::Cell(pair, rest) = &*cur.clone() {
                if let Some((nid, note)) = pair.as_cell() {
                    if cord_of(nid) == "doc" {
                        return blocks_of(&note.as_cell().unwrap().1)
                            .into_iter()
                            .filter(|x| x.alive)
                            .map(|x| x.text)
                            .collect();
                    }
                }
                cur = rest.clone();
            }
            Vec::new()
        };

        // A creates the note with one seed block, B syncs it
        net::submit(&a, &pa, cell(cord("mknote"), cell(cord("doc"), cord("shared doc"))));
        let seed = ins(&a, &pa, &num(0), "ada", "seed");
        net::connect_peer(b.clone(), pb.clone(), cfg_b, addr);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while texts(&b) != vec!["seed"] {
            assert!(std::time::Instant::now() < deadline, "B never synced the seed");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // CONCURRENT RUNS: each side chains two blocks off the seed while the
        // other does the same (no waiting in between — the ops race the wire)
        let a1 = ins(&a, &pa, &seed, "ada", "a-one");
        let _a2 = ins(&a, &pa, &a1, "ada", "a-two");
        let b1 = ins(&b, &pb, &seed, "bob", "b-one");
        let _b2 = ins(&b, &pb, &b1, "bob", "b-two");
        // CONCURRENT delete-vs-anchor: A tombstones the seed while B anchors to it
        net::submit(&a, &pa, cell(cord("del"), cell(cord("doc"), seed.clone())));
        let _b3 = ins(&b, &pb, &seed, "bob", "b-after-dead-anchor");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        loop {
            let (sa, sb) = (a.lock().unwrap().state().unwrap(), b.lock().unwrap().state().unwrap());
            let done = sa == sb && a.lock().unwrap().event_count() == 8;
            if done {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "replicas did not converge: A={:?} B={:?}", texts(&a), texts(&b));
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
        let ta = texts(&a);
        assert_eq!(ta, texts(&b), "byte-identical convergence");
        // every block from BOTH writers survives, seed is tombstoned away
        for t in ["a-one", "a-two", "b-one", "b-two", "b-after-dead-anchor"] {
            assert!(ta.iter().any(|x| x == t), "{} lost in the merge: {:?}", t, ta);
        }
        assert!(!ta.iter().any(|x| x == "seed"), "the tombstoned seed must not display");
        // intention: each writer's chained run stays CONTIGUOUS (no interleave)
        let pos = |t: &str| ta.iter().position(|x| x == t).unwrap();
        assert_eq!(pos("a-two"), pos("a-one") + 1, "A's run interleaved: {:?}", ta);
        assert_eq!(pos("b-two"), pos("b-one") + 1, "B's run interleaved: {:?}", ta);
    }

    #[test]
    fn notes_ops_fold_with_anchors_lww_and_tombstones() {
        let _g = test_guard();
        init(None, "", &[], Some(11)).unwrap();
        let t0 = event_count(); // the node is process-shared: index relatively
        let id = create("meeting minutes").unwrap();
        // three appended blocks form a run
        let b1 = append_block(&id, "ada", "agenda").unwrap();
        let b2 = append_block(&id, "ada", "old business").unwrap();
        let _b3 = append_block(&id, "bob", "new business").unwrap();
        let (title, blocks) = read_note(&id, 0).unwrap().expect("note exists");
        assert_eq!(title, "meeting minutes");
        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["agenda", "old business", "new business"]);
        // anchored insertion lands mid-document
        insert_after(&id, &b1, "bob", "roll call").unwrap();
        let (_, blocks) = read_note(&id, 0).unwrap().unwrap();
        let texts: Vec<&str> = blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["agenda", "roll call", "old business", "new business"]);
        // LWW text replacement names the block absolutely
        set_text(&id, &b2, "ada", "old business (carried)").unwrap();
        let (_, blocks) = read_note(&id, 0).unwrap().unwrap();
        assert!(blocks.iter().any(|b| b.text == "old business (carried)" && b.author == "ada"));
        // deletion is a tombstone: gone from the live view, still an anchor
        del_block(&id, &b2).unwrap();
        let (_, blocks) = read_note(&id, 0).unwrap().unwrap();
        let dead = blocks.iter().find(|b| b.bid == b2).expect("tombstone kept");
        assert!(!dead.alive);
        insert_after(&id, &b2, "bob", "budget").unwrap(); // anchoring to the tombstone still works
        let (_, blocks) = read_note(&id, 0).unwrap().unwrap();
        let live: Vec<&str> = blocks.iter().filter(|b| b.alive).map(|b| b.text.as_str()).collect();
        assert_eq!(live, vec!["agenda", "roll call", "budget", "new business"]);
        // time travel: at the moment after our first three events the note had one block
        // (event 1 = mknote, 2 = first append) — relative to the test's own start
        let (_, early) = read_note(&id, t0 + 2).unwrap().unwrap();
        assert_eq!(early.iter().filter(|b| b.alive).count(), 1);
        // retitle + list + bid round-trip
        retitle(&id, "minutes, v2").unwrap();
        let notes = list_notes(0).unwrap();
        assert!(notes.iter().any(|(nid, t, live)| nid == &id && t == "minutes, v2" && *live == 4));
        let bid = parse_bid(&b1).unwrap();
        assert_eq!(bid_string(&bid), b1);
        // kinds ride the id prefix; assemble joins the live blocks
        let pid = create_kind("p", "two-sector economy").unwrap();
        assert!(pid.starts_with('p'));
        append_block(&pid, "ada", "sector steel l=0.4 steel=0.2").unwrap();
        append_block(&pid, "bob", "demand steel=1.0").unwrap();
        let text = assemble(&pid).unwrap().unwrap();
        assert_eq!(text, "sector steel l=0.4 steel=0.2\ndemand steel=1.0");
        assert!(create_kind("x", "nope").is_err());
    }
}

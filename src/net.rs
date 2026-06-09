//! The distributed layer. Each node keeps an append-only, content-addressed log
//! of events. State is the deterministic fold of the agent's transition function
//! over the events in a fixed TOTAL ORDER (lamport, node_id, content-hash). Two
//! nodes holding the same SET of events therefore compute byte-identical state,
//! regardless of the order in which the network delivered them — strong eventual
//! consistency, with no consensus and no blockchain.
//!
//! Nodes synchronize by gossip + anti-entropy over TCP:
//!   HELLO  advertise node id
//!   HAVE   advertise the set of event hashes I hold
//!   WANT   request events by hash
//!   EVENT  push a serialized event
//! New local events are pushed immediately; a periodic HAVE sweep repairs any gaps.
//! The same protocol runs over a LAN or the open Internet (public IP + open port).

use crate::agent::Agent;
use crate::atom::Atom;
use crate::knot::{cell, num, Knot, N};
use crate::loom::Crash;
use crate::store::Store;

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub type Hash = [u8; 32];
pub type EventKey = (u64, u64, Hash); // (lamport, node_id, hash) — a deterministic total order

// ------------------------------- events -------------------------------------
pub struct Event {
    pub node_id: u64,
    pub lamport: u64,
    pub action: N,
}

impl Event {
    /// Event as a knot: [node_id [lamport action]].
    pub fn to_knot(&self) -> N {
        cell(
            num(self.node_id as u128),
            cell(num(self.lamport as u128), self.action.clone()),
        )
    }
    pub fn from_knot(k: &N) -> Option<Event> {
        let (idk, rest) = k.as_cell()?;
        let (lamk, action) = rest.as_cell()?;
        let node_id = idk.as_atom()?.to_u128()? as u64;
        let lamport = lamk.as_atom()?.to_u128()? as u64;
        Some(Event {
            node_id,
            lamport,
            action: action.clone(),
        })
    }
}

// ------------------------------- node ---------------------------------------
pub struct Node {
    pub id: u64,
    pub lamport: u64,
    pub events: BTreeMap<EventKey, N>, // ordered for the fold
    pub by_hash: HashMap<Hash, N>,     // for retrieval / dedup
    pub agent: Agent,
    store: Option<Store>,
    snapshot_every: usize,
    since_snapshot: usize,
    pub migrated: bool, // true if we discarded a snapshot from a different agent
    base_state: Option<N>,        // compaction baseline: folded state up to a watermark
    base_watermark: Option<EventKey>,
}

impl Node {
    pub fn new(id: u64, agent: Agent) -> Node {
        Node {
            id,
            lamport: 0,
            events: BTreeMap::new(),
            by_hash: HashMap::new(),
            agent,
            store: None,
            snapshot_every: 0,
            since_snapshot: 0,
            migrated: false,
            base_state: None,
            base_watermark: None,
        }
    }

    /// Open a node backed by a durable store directory, replaying the log to rebuild
    /// state. If a snapshot exists but was written by a *different* agent program,
    /// it is discarded (never trusted) and state is re-folded from the log — a safe
    /// upgrade, by construction.
    pub fn open(id: u64, agent: Agent, dir: &str, snapshot_every: usize) -> std::io::Result<Node> {
        let mut node = Node::new(id, agent);
        let store = Store::open(dir)?;
        // migration check against any existing snapshot
        if let Some((snap_cid, _n, _state)) = Store::read_snapshot(dir) {
            if snap_cid != node.agent.cid_atom() {
                node.migrated = true; // different program: snapshot cache is stale
            }
        }
        // adopt a compaction baseline written by THIS agent, if present
        if let Some((bcid, wm, bstate)) = Store::read_baseline(dir) {
            if bcid == node.agent.cid_atom() {
                node.base_state = Some(bstate);
                node.base_watermark = Some(wm);
            } else {
                node.migrated = true; // baseline from a different program: not trusted
            }
        }
        // the log is the source of truth; replay it (events covered by the baseline
        // are deduped automatically). No re-persisting.
        let events = Store::load_events(dir)?;
        for e in events {
            node.add_event_inner(e, false);
        }
        if let Some((wl, _, _)) = node.base_watermark {
            node.lamport = node.lamport.max(wl);
        }
        node.store = Some(store);
        node.snapshot_every = snapshot_every;
        Ok(node)
    }

    fn add_event_inner(&mut self, k: N, persist: bool) -> bool {
        let ev = match Event::from_knot(&k) {
            Some(e) => e,
            None => return false,
        };
        let h = k.cid();
        if self.by_hash.contains_key(&h) {
            return false;
        }
        if let Some(w) = self.base_watermark {
            if (ev.lamport, ev.node_id, h) <= w {
                return false; // already incorporated into the baseline
            }
        }
        if ev.lamport > self.lamport {
            self.lamport = ev.lamport;
        }
        self.events.insert((ev.lamport, ev.node_id, h), k.clone());
        self.by_hash.insert(h, k.clone());
        if persist {
            if let Some(s) = self.store.as_mut() {
                let _ = s.append(&k);
            }
            self.since_snapshot += 1;
            if self.snapshot_every > 0 && self.since_snapshot >= self.snapshot_every {
                let _ = self.snapshot();
            }
        }
        true
    }

    /// Insert an event knot received from a peer (durably). Returns true if new.
    pub fn add_event_knot(&mut self, k: N) -> bool {
        self.add_event_inner(k, true)
    }

    /// Create a local event, advancing this node's lamport clock (durably).
    pub fn local_action(&mut self, action: N) -> N {
        self.lamport += 1;
        let ev = Event {
            node_id: self.id,
            lamport: self.lamport,
            action,
        };
        let k = ev.to_knot();
        self.add_event_inner(k.clone(), true);
        k
    }

    /// Materialize current state by folding the agent over the ordered log.
    pub fn state(&self) -> Result<N, Crash> {
        self.state_at(self.events.len())
    }

    /// Time-travel: state as of the first `k` events in total order. Event sourcing
    /// gives this for free — useful for debugging and audit (something Urbit's opaque
    /// state makes hard).
    pub fn state_at(&self, k: usize) -> Result<N, Crash> {
        let mut s = self.base_state.clone().unwrap_or_else(|| self.agent.initial_state());
        for (_key, ev_knot) in self.events.iter().take(k) {
            if let Some(ev) = Event::from_knot(ev_knot) {
                s = self.agent.step(&ev.action, &s)?;
            }
        }
        Ok(s)
    }

    /// Write a snapshot (materialized state + agent CID) for fast recovery.
    pub fn snapshot(&mut self) -> Result<(), Crash> {
        let st = self.state()?;
        let cid = self.agent.cid_atom();
        let n = self.events.len();
        if let Some(s) = self.store.as_ref() {
            let _ = s.write_snapshot(&cid, n, &st);
        }
        self.since_snapshot = 0;
        Ok(())
    }

    /// Garbage-collect the log: fold every current event into a durable *baseline*
    /// (state + watermark), truncate the log, and drop the archived events from memory.
    /// State is preserved; the log is bounded. Recovery and gossip resume from the
    /// baseline. (Assumes events up to the watermark have propagated — straggler events
    /// at or below the watermark are treated as already incorporated.)
    pub fn compact(&mut self) -> Result<(), Crash> {
        let st = self.state()?;
        let wm = self.events.keys().next_back().copied().or(self.base_watermark);
        let cid = self.agent.cid_atom();
        if let (Some(w), Some(s)) = (wm, self.store.as_ref()) {
            let _ = s.write_baseline(&cid, w, &st);
        }
        if let Some(s) = self.store.as_mut() {
            let _ = s.reset_log();
        }
        self.base_state = Some(st);
        if wm.is_some() {
            self.base_watermark = wm;
        }
        self.events.clear();
        self.by_hash.clear();
        self.since_snapshot = 0;
        Ok(())
    }

    pub fn is_compacted(&self) -> bool {
        self.base_watermark.is_some()
    }

    /// Build a SNAP frame carrying our baseline, for a peer that lacks archived events.
    pub fn make_snap(&self) -> Option<Vec<u8>> {
        let st = self.base_state.clone()?;
        let w = self.base_watermark?;
        let base = encode_baseline(&self.agent.cid_atom(), w, &st);
        let mut v = vec![T_SNAP];
        v.extend_from_slice(&base.jam());
        Some(v)
    }

    /// Adopt a peer's baseline (same agent, strictly ahead of ours). Because a baseline
    /// is the deterministic fold of all events ≤ its watermark, adopting it is identical
    /// to having folded those events ourselves.
    pub fn adopt_snap(&mut self, payload: &[u8]) -> bool {
        let (k, _) = match Knot::cue(payload) {
            Some(x) => x,
            None => return false,
        };
        let parsed = decode_baseline(&k);
        let (cid, wm, state) = match parsed {
            Some(x) => x,
            None => return false,
        };
        if cid != self.agent.cid_atom() {
            return false;
        }
        if let Some(cur) = self.base_watermark {
            if wm <= cur {
                return false; // not ahead of what we already have
            }
        }
        self.base_state = Some(state);
        self.base_watermark = Some(wm);
        self.events.retain(|key, _| *key > wm);
        self.by_hash = self.events.values().map(|x| (x.cid(), x.clone())).collect();
        self.lamport = self.lamport.max(wm.0);
        true
    }

    pub fn has_store(&self) -> bool {
        self.store.is_some()
    }

    pub fn all_hashes(&self) -> Vec<Hash> {
        self.by_hash.keys().cloned().collect()
    }
    pub fn get(&self, h: &Hash) -> Option<N> {
        self.by_hash.get(h).cloned()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

// ------------------------------- wire protocol ------------------------------
const T_HELLO: u8 = 0x01;
const T_HAVE: u8 = 0x02;
const T_WANT: u8 = 0x03;
const T_EVENT: u8 = 0x04;
const T_SNAP: u8 = 0x05;
const MAX_FRAME: usize = 64 * 1024 * 1024;

fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let len = payload.len() as u32;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}
fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut lenb = [0u8; 4];
    r.read_exact(&mut lenb)?;
    let len = u32::from_be_bytes(lenb) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn msg_hello(id: u64) -> Vec<u8> {
    let mut v = vec![T_HELLO];
    v.extend_from_slice(&id.to_be_bytes());
    v
}
fn msg_hashes(tag: u8, hashes: &[Hash]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(&(hashes.len() as u32).to_be_bytes());
    for h in hashes {
        v.extend_from_slice(h);
    }
    v
}
fn msg_event(knot: &N) -> Vec<u8> {
    let mut v = vec![T_EVENT];
    v.extend_from_slice(&knot.jam());
    v
}
fn parse_hashes(payload: &[u8]) -> Option<Vec<Hash>> {
    if payload.len() < 5 {
        return None;
    }
    let count = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]) as usize;
    let mut out = Vec::with_capacity(count);
    let mut off = 5;
    for _ in 0..count {
        if off + 32 > payload.len() {
            return None;
        }
        let mut h = [0u8; 32];
        h.copy_from_slice(&payload[off..off + 32]);
        out.push(h);
        off += 32;
    }
    Some(out)
}

// ------------------------------- shared handles -----------------------------
pub type NodeHandle = Arc<Mutex<Node>>;
pub type Peers = Arc<Mutex<Vec<Sender<Vec<u8>>>>>;

fn encode_baseline(cid: &Atom, wm: EventKey, st: &N) -> N {
    cell(
        crate::knot::atom(cid.clone()),
        cell(
            cell(
                cell(num(wm.0 as u128), num(wm.1 as u128)),
                crate::knot::atom(Atom::from_bytes_le(wm.2.to_vec())),
            ),
            st.clone(),
        ),
    )
}

fn decode_baseline(k: &N) -> Option<(Atom, EventKey, N)> {
    let (cidk, rest) = k.as_cell()?;
    let (wmk, state) = rest.as_cell()?;
    let (lnk, whk) = wmk.as_cell()?;
    let (lk, nk) = lnk.as_cell()?;
    let cid = cidk.as_atom()?.clone();
    let wl = lk.as_atom()?.to_u128()? as u64;
    let wn = nk.as_atom()?.to_u128()? as u64;
    let mut wh = [0u8; 32];
    for (i, b) in whk.as_atom()?.bytes_le().iter().take(32).enumerate() {
        wh[i] = *b;
    }
    Some((cid, (wl, wn, wh), state.clone()))
}

fn broadcast(peers: &Peers, frame: Vec<u8>) {
    // Send to every live peer, and drop any whose writer thread has died (its
    // receiver is gone). Keeps the peer set bounded across reconnections — important
    // for long-lived Internet nodes that see peers come and go.
    let mut list = peers.lock().unwrap();
    list.retain(|tx| tx.send(frame.clone()).is_ok());
}

pub struct Config {
    pub name: String,
    pub listen: String,
    pub peers: Vec<String>,
    pub verbose: bool,
    pub compact_every: usize, // GC the log once it exceeds this many events (0 = never)
}

/// Start listener, peer connectors, and the anti-entropy sweep. Returns the shared
/// handles so a CLI can poke the node and read its state.
pub fn start(node: NodeHandle, cfg: Arc<Config>) -> Peers {
    let peers: Peers = Arc::new(Mutex::new(Vec::new()));

    // listener
    {
        let node = node.clone();
        let peers = peers.clone();
        let cfg = cfg.clone();
        let listen = cfg.listen.clone();
        thread::spawn(move || match TcpListener::bind(&listen) {
            Ok(l) => {
                if cfg.verbose {
                    eprintln!("[{}] listening on {}", cfg.name, listen);
                }
                for stream in l.incoming() {
                    if let Ok(s) = stream {
                        let node = node.clone();
                        let peers = peers.clone();
                        let cfg = cfg.clone();
                        thread::spawn(move || {
                            let _ = handle_conn(s, node, peers, cfg);
                        });
                    }
                }
            }
            Err(e) => eprintln!("[{}] bind {} failed: {}", cfg.name, listen, e),
        });
    }

    // peer connectors (retry forever)
    for addr in cfg.peers.iter().cloned() {
        let node = node.clone();
        let peers = peers.clone();
        let cfg = cfg.clone();
        thread::spawn(move || loop {
            match TcpStream::connect(&addr) {
                Ok(s) => {
                    if cfg.verbose {
                        eprintln!("[{}] connected to {}", cfg.name, addr);
                    }
                    let _ = handle_conn(s, node.clone(), peers.clone(), cfg.clone());
                }
                Err(_) => {}
            }
            thread::sleep(Duration::from_millis(1000));
        });
    }

    // anti-entropy: periodically advertise everything we hold, and GC the log
    {
        let node = node.clone();
        let peers = peers.clone();
        let cfg = cfg.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(1500));
            let hs = { node.lock().unwrap().all_hashes() };
            if !hs.is_empty() {
                broadcast(&peers, msg_hashes(T_HAVE, &hs));
            }
            if cfg.compact_every > 0 {
                let mut n = node.lock().unwrap();
                if n.event_count() >= cfg.compact_every {
                    let _ = n.compact();
                    if cfg.verbose {
                        eprintln!("[{}] compacted log -> baseline", cfg.name);
                    }
                }
            }
        });
    }

    peers
}

fn handle_conn(stream: TcpStream, node: NodeHandle, peers: Peers, cfg: Arc<Config>) -> io::Result<()> {
    stream.set_nodelay(true).ok();
    let mut reader = stream.try_clone()?;
    let mut writer = stream;

    // per-connection writer thread fed by a channel
    let (tx, rx) = channel::<Vec<u8>>();
    {
        let mut w = writer.try_clone()?;
        thread::spawn(move || {
            for frame in rx.iter() {
                if write_frame(&mut w, &frame).is_err() {
                    break;
                }
            }
        });
    }
    // register this peer for broadcasts
    peers.lock().unwrap().push(tx.clone());

    // greet: HELLO + HAVE(all); if we've GC'd our log, also offer the baseline so a
    // peer that can never fetch the archived events can still catch up.
    let _ = tx.send(msg_hello(node.lock().unwrap().id));
    {
        let n = node.lock().unwrap();
        if let Some(sn) = n.make_snap() {
            let _ = tx.send(sn);
        }
    }
    let hs = { node.lock().unwrap().all_hashes() };
    let _ = tx.send(msg_hashes(T_HAVE, &hs));
    let _ = &mut writer; // writer half owned by writer thread clone

    loop {
        let payload = read_frame(&mut reader)?;
        if payload.is_empty() {
            continue;
        }
        match payload[0] {
            T_HELLO => {
                if cfg.verbose && payload.len() >= 9 {
                    let mut idb = [0u8; 8];
                    idb.copy_from_slice(&payload[1..9]);
                    eprintln!("[{}] peer hello id={}", cfg.name, u64::from_be_bytes(idb));
                }
            }
            T_HAVE => {
                if let Some(hashes) = parse_hashes(&payload) {
                    let missing: Vec<Hash> = {
                        let n = node.lock().unwrap();
                        hashes.into_iter().filter(|h| !n.by_hash.contains_key(h)).collect()
                    };
                    if !missing.is_empty() {
                        let _ = tx.send(msg_hashes(T_WANT, &missing));
                    }
                }
            }
            T_WANT => {
                if let Some(hashes) = parse_hashes(&payload) {
                    let (frames, snap): (Vec<Vec<u8>>, Option<Vec<u8>>) = {
                        let n = node.lock().unwrap();
                        let fs: Vec<Vec<u8>> = hashes.iter().filter_map(|h| n.get(h)).map(|k| msg_event(&k)).collect();
                        // a peer wants events we no longer hold -> offer our baseline
                        let missing = hashes.iter().any(|h| n.get(h).is_none());
                        let snap = if missing && n.is_compacted() { n.make_snap() } else { None };
                        (fs, snap)
                    };
                    if let Some(sn) = snap {
                        let _ = tx.send(sn);
                    }
                    for f in frames {
                        let _ = tx.send(f);
                    }
                }
            }
            T_SNAP => {
                let adopted = { node.lock().unwrap().adopt_snap(&payload[1..]) };
                if adopted && cfg.verbose {
                    eprintln!("[{}] adopted peer baseline (log GC)", cfg.name);
                }
            }
            T_EVENT => {
                if let Some((knot, _used)) = Knot::cue(&payload[1..]) {
                    // integrity is intrinsic: the hash is derived from content. We just
                    // require it to parse as an event before accepting it.
                    if Event::from_knot(&knot).is_some() {
                        let is_new = { node.lock().unwrap().add_event_knot(knot.clone()) };
                        if is_new {
                            if cfg.verbose {
                                let (cnt, st) = {
                                    let n = node.lock().unwrap();
                                    (n.event_count(), n.state().ok())
                                };
                                eprintln!(
                                    "[{}] +event (total {}) state={}",
                                    cfg.name,
                                    cnt,
                                    st.map(|s| show_state(&s)).unwrap_or_else(|| "<crash>".into())
                                );
                            }
                            // propagate to everyone; echoes are deduped by hash
                            broadcast(&peers, msg_event(&knot));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Submit a local action and gossip it to peers.
pub fn submit(node: &NodeHandle, peers: &Peers, action: N) {
    let knot = { node.lock().unwrap().local_action(action) };
    broadcast(peers, msg_event(&knot));
}

pub fn show_state(s: &N) -> String {
    match &**s {
        Knot::Atom(a) => a
            .to_u128()
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:?}", s)),
        _ => format!("{:?}", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{act_add, act_incr, act_reset, Agent};

    fn mk(id: u64) -> Node {
        Node::new(id, Agent::new().unwrap())
    }

    #[test]
    fn compaction_preserves_state_and_bounds_log() {
        let dir = tmpdir("compact");
        {
            let mut n = Node::open(1, Agent::new().unwrap(), &dir, 0).unwrap();
            for _ in 0..5 {
                n.local_action(act_incr());
            }
            let before = n.state().unwrap();
            n.compact().unwrap();
            assert_eq!(n.event_count(), 0, "log archived into baseline");
            assert_eq!(n.state().unwrap(), before, "state preserved across compaction");
            n.local_action(act_incr()); // new event folds on top of the baseline
        }
        // restart: recover from baseline + the retained tail
        let n2 = Node::open(1, Agent::new().unwrap(), &dir, 0).unwrap();
        assert_eq!(show_state(&n2.state().unwrap()), "6");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_peer_bootstraps_from_baseline_snap() {
        let dir = tmpdir("snap");
        let mut a = Node::open(1, Agent::new().unwrap(), &dir, 0).unwrap();
        for _ in 0..4 {
            a.local_action(act_incr()); // state 4
        }
        a.compact().unwrap(); // A archives its early events
        let snap = a.make_snap().expect("A has a baseline to share");

        // B is brand new and can never fetch A's archived events — it adopts the baseline
        let mut b = mk(2);
        assert!(b.adopt_snap(&snap[1..]));
        assert_eq!(show_state(&b.state().unwrap()), "4");

        // A emits a fresh event after compaction; B applies it; they converge
        let e = a.local_action(act_incr()); // state 5
        assert!(b.add_event_knot(e));
        assert_eq!(b.state().unwrap(), a.state().unwrap());
        assert_eq!(show_state(&b.state().unwrap()), "5");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_nodes_converge_regardless_of_delivery_order() {
        // Node A and Node B each create some events, then exchange them in OPPOSITE
        // delivery orders. Final state must be identical (strong eventual consistency).
        let mut a = mk(1);
        let mut b = mk(2);

        let e1 = a.local_action(act_incr()); // A: +1
        let e2 = a.local_action(act_add(10)); // A: +10
        let e3 = b.local_action(act_incr()); // B: +1
        let e4 = b.local_action(act_reset()); // B: reset
        let e5 = b.local_action(act_add(5)); // B: +5

        // A receives B's events in forward order
        for e in [&e3, &e4, &e5] {
            a.add_event_knot(e.clone());
        }
        // B receives A's events in reverse order
        for e in [&e2, &e1] {
            b.add_event_knot(e.clone());
        }

        let sa = a.state().unwrap();
        let sb = b.state().unwrap();
        assert_eq!(sa, sb, "states diverged: {:?} vs {:?}", sa, sb);
        assert_eq!(sa.cid(), sb.cid()); // identical content address
    }

    #[test]
    fn duplicate_events_are_idempotent() {
        let mut a = mk(1);
        let e = a.local_action(act_incr());
        assert!(!a.add_event_knot(e.clone())); // already present
        assert_eq!(a.event_count(), 1);
    }

    fn tmpdir(tag: &str) -> String {
        let p = std::env::temp_dir().join(format!("lattice-node-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p.to_string_lossy().to_string()
    }

    #[test]
    fn state_survives_restart() {
        let dir = tmpdir("restart");
        {
            let mut n = Node::open(1, Agent::new().unwrap(), &dir, 0).unwrap();
            n.local_action(act_incr());
            n.local_action(act_add(100));
            assert_eq!(n.state().unwrap(), crate::knot::num(101));
            n.snapshot().unwrap();
        }
        // brand-new Node instance, same directory: must recover from the durable log
        let n2 = Node::open(1, Agent::new().unwrap(), &dir, 0).unwrap();
        assert_eq!(n2.event_count(), 2);
        assert_eq!(n2.state().unwrap(), crate::knot::num(101));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn time_travel_replay() {
        let mut n = mk(1);
        n.local_action(act_incr()); // 1
        n.local_action(act_add(10)); // 11
        n.local_action(act_reset()); // 0
        assert_eq!(n.state_at(0).unwrap(), crate::knot::num(0));
        assert_eq!(n.state_at(1).unwrap(), crate::knot::num(1));
        assert_eq!(n.state_at(2).unwrap(), crate::knot::num(11));
        assert_eq!(n.state_at(3).unwrap(), crate::knot::num(0));
    }

    #[test]
    fn upgrade_refolds_log_without_breach() {
        let dir = tmpdir("upgrade");
        {
            let mut n = Node::open(1, Agent::new_version(1).unwrap(), &dir, 0).unwrap();
            n.local_action(act_incr()); // v1: +1
            n.local_action(act_add(100));
            n.snapshot().unwrap();
            assert_eq!(n.state().unwrap(), crate::knot::num(101));
        }
        // reopen with v2 (incr adds 2): snapshot is from a different agent -> discarded,
        // log re-folded -> 2 + 100 = 102, no corruption.
        let n2 = Node::open(1, Agent::new_version(2).unwrap(), &dir, 0).unwrap();
        assert!(n2.migrated);
        assert_eq!(n2.event_count(), 2);
        assert_eq!(n2.state().unwrap(), crate::knot::num(102));
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! Persistence — the single-level store. State is durable across restarts because
//! the **event log is the source of truth**: an append-only, content-addressed file
//! of jammed events. On startup we replay it to rebuild state. A **snapshot** caches
//! the materialized state plus the agent's content-address for fast, integrity-checked
//! recovery.
//!
//! This design also disarms the failure mode that plagues comparable systems (the
//! Urbit "breach"): because state is a deterministic FOLD of an agent over the log,
//! upgrading the agent program can never corrupt state — you just re-fold the same
//! log through the new agent. The snapshot records which agent produced it, so a
//! mismatch is detected and the stale cache is simply discarded, never trusted.

use crate::atom::Atom;
use crate::knot::{cell, num, Knot, N};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub struct Store {
    dir: PathBuf,
    log: File, // opened for append
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("events.log")
}
fn snap_path(dir: &Path) -> PathBuf {
    dir.join("snapshot.knot")
}
fn baseline_path(dir: &Path) -> PathBuf {
    dir.join("baseline.knot")
}

impl Store {
    pub fn open(dir: &str) -> io::Result<Store> {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(log_path(&dir))?;
        Ok(Store { dir, log })
    }

    /// Append one event (a knot) to the durable log: u32 big-endian length + jam.
    pub fn append(&mut self, ev: &N) -> io::Result<()> {
        let j = ev.jam();
        self.log.write_all(&(j.len() as u32).to_be_bytes())?;
        self.log.write_all(&j)?;
        self.log.flush()?;
        Ok(())
    }

    /// Read every event from the durable log, in append order.
    pub fn load_events(dir: &str) -> io::Result<Vec<N>> {
        let p = log_path(Path::new(dir));
        let mut bytes = Vec::new();
        match File::open(&p) {
            Ok(mut f) => {
                f.read_to_end(&mut bytes)?;
            }
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        }
        let mut out = Vec::new();
        let mut off = 0;
        while off + 4 <= bytes.len() {
            let len = u32::from_be_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]) as usize;
            off += 4;
            if off + len > bytes.len() {
                break; // truncated tail (e.g. crash mid-write) — ignore the partial record
            }
            if let Some((k, _)) = Knot::cue(&bytes[off..off + len]) {
                out.push(k);
            }
            off += len;
        }
        Ok(out)
    }

    /// Atomically write a snapshot knot: [agent_cid [n_events state]].
    pub fn write_snapshot(&self, agent_cid: &Atom, n_events: usize, state: &N) -> io::Result<()> {
        let snap = cell(
            crate::knot::atom(agent_cid.clone()),
            cell(num(n_events as u128), state.clone()),
        );
        let tmp = self.dir.join("snapshot.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&snap.jam())?;
            f.flush()?;
        }
        fs::rename(&tmp, snap_path(&self.dir))?; // atomic replace
        Ok(())
    }

    /// Read the snapshot, if any: returns (agent_cid, n_events, state).
    pub fn read_snapshot(dir: &str) -> Option<(Atom, usize, N)> {
        let p = snap_path(Path::new(dir));
        let mut bytes = Vec::new();
        File::open(&p).ok()?.read_to_end(&mut bytes).ok()?;
        let (k, _) = Knot::cue(&bytes)?;
        let (cidk, rest) = k.as_cell()?;
        let (nk, state) = rest.as_cell()?;
        let cid = cidk.as_atom()?.clone();
        let n = nk.as_atom()?.to_u128()? as usize;
        Some((cid, n, state.clone()))
    }

    /// Atomically write a compaction *baseline*: the folded state of every event up to
    /// and including a watermark `(lamport, node_id, hash)`. Encoded as
    /// `[agent_cid [[lamport node_id] hash] state]`.
    pub fn write_baseline(
        &self,
        agent_cid: &Atom,
        wm: (u64, u64, [u8; 32]),
        state: &N,
    ) -> io::Result<()> {
        let (wl, wn, wh) = wm;
        let base = cell(
            crate::knot::atom(agent_cid.clone()),
            cell(
                cell(
                    cell(num(wl as u128), num(wn as u128)),
                    crate::knot::atom(Atom::from_bytes_le(wh.to_vec())),
                ),
                state.clone(),
            ),
        );
        let tmp = self.dir.join("baseline.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(&base.jam())?;
            f.flush()?;
        }
        fs::rename(&tmp, baseline_path(&self.dir))?;
        Ok(())
    }

    /// Read the compaction baseline, if any: (agent_cid, watermark, state).
    pub fn read_baseline(dir: &str) -> Option<(Atom, (u64, u64, [u8; 32]), N)> {
        let p = baseline_path(Path::new(dir));
        let mut bytes = Vec::new();
        File::open(&p).ok()?.read_to_end(&mut bytes).ok()?;
        let (k, _) = Knot::cue(&bytes)?;
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

    /// Truncate the durable event log to empty (used after a baseline is written).
    pub fn reset_log(&mut self) -> io::Result<()> {
        {
            let _ = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(log_path(&self.dir))?;
        }
        self.log = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(log_path(&self.dir))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{act_add, act_incr, Agent};
    use crate::knot::num;

    fn tmpdir(tag: &str) -> String {
        let p = std::env::temp_dir().join(format!("lattice-store-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p.to_string_lossy().to_string()
    }

    #[test]
    fn log_roundtrip_and_replay() {
        let dir = tmpdir("log");
        let agent = Agent::new().unwrap();
        {
            let mut s = Store::open(&dir).unwrap();
            // build three events as knots [node lamport action]
            for (l, act) in [(1u128, act_incr()), (2, act_add(10)), (3, act_incr())] {
                let ev = cell(num(7), cell(num(l), act));
                s.append(&ev).unwrap();
            }
        }
        // replay
        let events = Store::load_events(&dir).unwrap();
        assert_eq!(events.len(), 3);
        // fold through the agent: 1 + 10 + 1 = 12
        let mut st = agent.initial_state();
        for ev in &events {
            let action = ev.as_cell().unwrap().1.as_cell().unwrap().1.clone();
            st = agent.step(&action, &st).unwrap();
        }
        assert_eq!(st, num(12));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshot_roundtrip_and_migration_detection() {
        let dir = tmpdir("snap");
        let v1 = Agent::new_version(1).unwrap();
        let v2 = Agent::new_version(2).unwrap();
        let st = num(42);
        {
            let s = Store::open(&dir).unwrap();
            s.write_snapshot(&v1.cid_atom(), 5, &st).unwrap();
        }
        let (cid, n, state) = Store::read_snapshot(&dir).unwrap();
        assert_eq!(n, 5);
        assert_eq!(state, num(42));
        assert_eq!(cid, v1.cid_atom()); // matches the agent that wrote it
        assert_ne!(cid, v2.cid_atom()); // a different agent => mismatch => discard cache
        fs::remove_dir_all(&dir).ok();
    }
}

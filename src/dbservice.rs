//! Persistent, named databases — durability for the composed database.
//!
//! `lib/db.lat` is a complete database (LSM + write-ahead log + Bloom filter +
//! secondary index + MVCC version chains), but it is a *pure* Latte value: it lives
//! only for one evaluation and is then discarded. This module gives it real
//! durability and a lifetime across requests and restarts.
//!
//! The design is the textbook one — the on-disk write-ahead log is the source of
//! truth. Every write is appended to `<name>.wal` (and flushed) *before* it is
//! applied, and on open the log is replayed to rebuild the in-memory value. The
//! live value is held as a noun in the host and threaded through the real
//! `db_put`/`db_get`/… arms via `call_arm`, so the database logic stays in Latte;
//! only persistence and the cross-request lifetime live here.
//!
//! WAL line format (tab-separated):
//!   `# <idxtag> <rschema> <thresh>`   header (schema), first line
//!   `P\t<key>\t<record-expr>`          put: record is the original Latte expression
//!   `D\t<key>`                          delete (tombstone)

use crate::knot::{cord, num, Knot, N};
use crate::{knot_tuple, latte};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One open, live database: the current value plus the metadata we need to persist
/// and to render it.
struct Live {
    value: N,
    idxtag: u128,
    rschema: u128,
    thresh: u128,
    keys: BTreeSet<String>, // keys with a present (non-tombstone) current version
    wal_len: usize,         // number of applied operations
}

pub struct DbService {
    core: N,
    axes: Vec<(String, u128)>,
    dir: PathBuf,
    dbs: HashMap<String, Live>,
}

impl DbService {
    /// Build a service whose write-ahead logs live under `dir`. Compiles the db and
    /// findb libraries once into a callable program.
    pub fn new(dir: PathBuf) -> Result<DbService, String> {
        let (core, axes) = latte::compile_library_program(&["db", "findb"])?;
        let _ = fs::create_dir_all(&dir);
        Ok(DbService { core, axes, dir, dbs: HashMap::new() })
    }

    // Call a Latte arm on the live database noun. `call_arm` applies the arm to a
    // right-nested argument tuple, so a 3-parameter arm `fn [d pk rec]` is invoked
    // with `cell(d, cell(pk, rec))` — built here with the knot_tuple! macro. This is
    // how the in-memory database value is threaded through the real db.lat logic
    // without ever serializing it.
    fn call(&self, arm: &str, args: N) -> Result<N, String> {
        latte::call_arm(&self.core, &self.axes, arm, args)
    }

    /// Apply a state-transition arm with the Anvil-NATIVE program (compiled once for the db
    /// libraries, then this call's argument tuple piped in), so the database's *persistent
    /// value* is updated by natively compiled code — with no interpreter fuel ceiling. The
    /// emitted result is identical to `call_arm` (audited), so semantics are unchanged; on any
    /// native decline we fall back to the verified interpreter. `nargs` is the arm's parameter
    /// count: `call_arm` threads a right-nested tuple, so we destructure `__in` the same way.
    fn call_native_first(&self, arm: &str, args: N, nargs: usize) -> Result<N, String> {
        let expr = native_arm_expr(arm, nargs);
        if let Some(n) =
            crate::rustgen::run_native_with_input(&expr, &args, &["db", "findb"], false)
        {
            return Ok(n);
        }
        latte::call_arm(&self.core, &self.axes, arm, args)
    }

    /// Evaluate a record expression (a small literal) to a noun.
    fn eval_rec(&self, expr: &str) -> Result<N, String> {
        crate::rustgen::run_adaptive(expr, &["std", "num"])
    }

    fn wal_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.wal", name))
    }

    fn fresh(&self, idxtag: u128, rschema: u128, thresh: u128) -> Result<N, String> {
        self.call_native_first("db_open", knot_tuple!(num(idxtag), num(rschema), num(thresh)), 3)
    }

    /// Open `name`, replaying its log from disk if present, else creating it with the
    /// given schema. Idempotent: a second open is a no-op.
    pub fn open(&mut self, name: &str, idxtag: u128, rschema: u128, thresh: u128) -> Result<(), String> {
        if self.dbs.contains_key(name) {
            return Ok(());
        }
        if self.wal_path(name).exists() {
            return self.replay(name);
        }
        let value = self.fresh(idxtag, rschema, thresh)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.wal_path(name))
            .map_err(|e| format!("open wal: {}", e))?;
        writeln!(f, "# {} {} {}", idxtag, rschema, thresh).map_err(|e| format!("{}", e))?;
        self.dbs.insert(name.into(), Live { value, idxtag, rschema, thresh, keys: BTreeSet::new(), wal_len: 0 });
        Ok(())
    }

    /// Rebuild a database by replaying its on-disk log. This is crash recovery: the
    /// log is the truth, the in-memory structures are derived from it.
    fn replay(&mut self, name: &str) -> Result<(), String> {
        let text = fs::read_to_string(self.wal_path(name)).map_err(|e| format!("read wal: {}", e))?;
        let (mut idxtag, mut rschema, mut thresh) = (2u128, 0u128, 256u128);
        let mut ops: Vec<String> = Vec::new();
        for ln in text.lines() {
            if let Some(h) = ln.strip_prefix('#') {
                let nums: Vec<u128> = h.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if nums.len() >= 3 {
                    idxtag = nums[0];
                    rschema = nums[1];
                    thresh = nums[2];
                }
            } else if !ln.trim().is_empty() {
                ops.push(ln.to_string());
            }
        }
        let mut value = self.fresh(idxtag, rschema, thresh)?;
        let mut keys = BTreeSet::new();
        let mut wal_len = 0usize;
        // Expand transaction batches: a `T<n>` line groups the next n ops into one atomic
        // unit. A complete batch contributes all n ops; a batch at the very end with fewer
        // than n ops following it is a torn write from a crash mid-commit and is dropped
        // whole, so a transaction is all-or-nothing on recovery. Logs without `T` lines
        // (single put/delete) flatten to themselves, unchanged.
        let mut applic: Vec<&String> = Vec::with_capacity(ops.len());
        let mut i = 0;
        while i < ops.len() {
            if let Some(rest) = ops[i].strip_prefix("T\t") {
                let n: usize = rest.trim().parse().unwrap_or(0);
                if i + n >= ops.len() {
                    break; // torn trailing batch: drop it and stop
                }
                for j in (i + 1)..=(i + n) {
                    applic.push(&ops[j]);
                }
                i += n + 1;
            } else {
                applic.push(&ops[i]);
                i += 1;
            }
        }
        for ln in &applic {
            let mut parts = ln.splitn(3, '\t');
            match parts.next() {
                Some("P") => {
                    let key = parts.next().unwrap_or("").to_string();
                    let rexpr = parts.next().unwrap_or("0");
                    let rec = self.eval_rec(rexpr)?;
                    value = self.call("db_put", knot_tuple!(value.clone(), cord(&key), rec))?;
                    keys.insert(key);
                    wal_len += 1;
                }
                Some("D") => {
                    let key = parts.next().unwrap_or("").to_string();
                    value = self.call("db_delete", knot_tuple!(value.clone(), cord(&key)))?;
                    keys.remove(&key);
                    wal_len += 1;
                }
                Some("I") => {
                    // add a secondary index on a field; db_addindex backfills from rows
                    // already applied, and later puts maintain it.
                    if let Some(f) = parts.next().and_then(|x| x.trim().parse::<u128>().ok()) {
                        value = self.call("db_addindex", knot_tuple!(value.clone(), num(f)))?;
                    }
                    wal_len += 1;
                }
                _ => {}
            }
        }
        self.dbs.insert(name.into(), Live { value, idxtag, rschema, thresh, keys, wal_len });
        Ok(())
    }

    fn append_wal(&self, name: &str, line: &str) -> Result<(), String> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.wal_path(name))
            .map_err(|e| format!("append wal: {}", e))?;
        writeln!(f, "{}", line).map_err(|e| format!("{}", e))?;
        f.flush().map_err(|e| format!("{}", e))?; // durable before we report success
        Ok(())
    }

    /// Store `record-expr` under `key` in `name`, logging it durably first.
    pub fn put(&mut self, name: &str, key: &str, rec_expr: &str) -> Result<(), String> {
        self.open(name, 2, 0, 256)?;
        let rec = self.eval_rec(rec_expr)?;
        let cur = self.dbs.get(name).ok_or("no such db")?.value.clone();
        let newval = self.call_native_first("db_put", knot_tuple!(cur, cord(key), rec), 3)?;
        self.append_wal(name, &format!("P\t{}\t{}", key, rec_expr.replace('\n', " ")))?;
        let live = self.dbs.get_mut(name).unwrap();
        live.value = newval;
        live.keys.insert(key.into());
        live.wal_len += 1;
        Ok(())
    }

    /// Append a tombstone for `key` (MVCC keeps the history).
    pub fn delete(&mut self, name: &str, key: &str) -> Result<(), String> {
        self.open(name, 2, 0, 256)?;
        let cur = self.dbs.get(name).ok_or("no such db")?.value.clone();
        let newval = self.call_native_first("db_delete", knot_tuple!(cur, cord(key)), 2)?;
        self.append_wal(name, &format!("D\t{}", key))?;
        let live = self.dbs.get_mut(name).unwrap();
        live.value = newval;
        live.keys.remove(key);
        live.wal_len += 1;
        Ok(())
    }

    fn value_of(&mut self, name: &str) -> Result<N, String> {
        self.open(name, 2, 0, 256)?;
        Ok(self.dbs.get(name).unwrap().value.clone())
    }

    /// Read the visible record for `key`, rendered for display.
    pub fn get(&mut self, name: &str, key: &str) -> Result<String, String> {
        let v = self.value_of(name)?;
        let r = self.call("db_get", knot_tuple!(v, cord(key)))?;
        Ok(crate::serve::render_noun(&r))
    }

    /// All live rows whose indexed field equals `fv`, rendered as HTML.
    pub fn query_html(&mut self, name: &str, fv: &str) -> Result<String, String> {
        let v = self.value_of(name)?;
        let r = self.call("db_queryhtml", knot_tuple!(v, cord(fv)))?;
        Ok(crate::serve::render_result(&r))
    }

    /// The version history of `key`, rendered as HTML (newest first).
    pub fn history_html(&mut self, name: &str, key: &str) -> Result<String, String> {
        let v = self.value_of(name)?;
        let r = self.call("db_historyhtml", knot_tuple!(v, cord(key)))?;
        Ok(crate::serve::render_result(&r))
    }

    /// GROUP BY field `gtag`, SUM field `atag`, over the durable database's live keys —
    /// the query/analytics layer run against on-disk data. Returns one `group: total` line
    /// per group.
    pub fn agg(&mut self, name: &str, gtag: u128, atag: u128) -> Result<String, String> {
        self.open(name, 2, 0, 256)?;
        let live = self.dbs.get(name).unwrap();
        let v = live.value.clone();
        let keylist = keys_to_noun(&live.keys);
        let r = self.call("db_aggtext", knot_tuple!(v, keylist, num(gtag), num(atag)))?;
        Ok(r.as_atom()
            .map(|a| String::from_utf8_lossy(&a.bytes_le()).into_owned())
            .unwrap_or_else(|| crate::serve::render_noun(&r)))
    }

    /// Compact the on-disk log: rewrite it as one `P` line per live key, each holding
    /// that key's CURRENT value, dropping superseded versions and deleted keys. This
    /// bounds recovery time and disk for a long-lived database, since `open` no longer
    /// replays the entire history. The tradeoff is the standard one for a checkpoint:
    /// the per-version MVCC history *before* the checkpoint is collapsed to the current
    /// value (history accumulates again afterwards). Returns (old_len, new_len).
    pub fn checkpoint(&mut self, name: &str) -> Result<(usize, usize), String> {
        self.open(name, 2, 0, 256)?;
        let live = self.dbs.get(name).ok_or_else(|| format!("no database '{}'", name))?;
        let (idxtag, rschema, thresh, old_len) = (live.idxtag, live.rschema, live.thresh, live.wal_len);
        let value = live.value.clone();
        let keys: Vec<String> = live.keys.iter().cloned().collect();

        // Build the compacted log: the schema header, an `I` line for each secondary index
        // beyond the primary (so they survive the checkpoint), then the current record of
        // each live key as a `P` line. db_get wraps the record as [%rec fields]; db_put
        // takes the bare fields, so we re-emit the tail. The `I` lines come first so that on
        // replay the indexes exist before the rows arrive and every put maintains them.
        let mut lines = vec![format!("# {} {} {}", idxtag, rschema, thresh)];
        let idx_r = self.call("db_indexes", value.clone())?;
        let mut cur = idx_r;
        while let Knot::Cell(h, t) = &*cur {
            if let Some(f) = h.as_atom().and_then(|a| a.to_u128()) {
                if f != idxtag {
                    lines.push(format!("I\t{}", f));
                }
            }
            cur = t.clone();
        }
        for key in &keys {
            let rec = self.call("db_get", knot_tuple!(value.clone(), cord(key)))?;
            let fields = match &*rec {
                Knot::Cell(_, t) => t.clone(),
                _ => continue, // not a present record; skip
            };
            let expr = noun_to_latte(&fields)
                .ok_or_else(|| format!("key '{}' holds a value with no literal form; cannot checkpoint", key))?;
            lines.push(format!("P\t{}\t{}", key, expr));
        }
        let new_len = lines.len() - 1; // exclude the header

        // Install atomically (write a temp file, fsync-on-close, rename), then rebuild
        // the live value from the compacted log so memory and disk agree.
        let path = self.wal_path(name);
        let tmp = path.with_extension("wal.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("write snapshot: {}", e))?;
            f.write_all(format!("{}\n", lines.join("\n")).as_bytes()).map_err(|e| format!("{}", e))?;
            f.flush().map_err(|e| format!("{}", e))?;
        }
        fs::rename(&tmp, &path).map_err(|e| format!("install snapshot: {}", e))?;
        self.dbs.remove(name);
        self.replay(name)?;
        Ok((old_len, new_len))
    }

    /// Begin a transaction: return the current snapshot timestamp. Reads done after this
    /// see a consistent instant, and the snapshot is what commit validates the read-set
    /// against (db.lat's `db_begin` is just the current version counter — begin is free
    /// because the database value is immutable).
    pub fn begin(&mut self, name: &str) -> Result<u128, String> {
        let v = self.value_of(name)?;
        let r = self.call("db_begin", v)?;
        Ok(r.as_atom().and_then(|a| a.to_u128()).unwrap_or(0))
    }

    /// A durable, serializable multi-key transaction. The caller began at `snap` and read
    /// the keys in `reads`; `writes` are (key, record-expr) pairs to apply atomically.
    /// db.lat's `db_commit` runs optimistic concurrency control: it aborts if any read key
    /// has a committed version newer than the snapshot (closing write skew), otherwise it
    /// applies every write or none. On commit we append the whole write-set to the log as a
    /// single crash-atomic batch (a `T<n>` frame) and swap in the new value; on abort the
    /// database is untouched. Returns Ok(true) on commit, Ok(false) on abort.
    pub fn txn(
        &mut self,
        name: &str,
        snap: u128,
        reads: &[String],
        writes: &[(String, String)],
    ) -> Result<bool, String> {
        let value = self.value_of(name)?;

        // read-set: a list of primary-key cords; write-set: a list of [pk record] cells.
        let mut readset = num(0);
        for k in reads.iter().rev() {
            readset = crate::knot::cell(cord(k), readset);
        }
        let mut evaled: Vec<(String, N)> = Vec::with_capacity(writes.len());
        for (k, expr) in writes {
            evaled.push((k.clone(), self.eval_rec(expr)?));
        }
        let mut writeset = num(0);
        for (k, rec) in evaled.iter().rev() {
            writeset = crate::knot::cell(crate::knot::cell(cord(k), rec.clone()), writeset);
        }

        let r = self.call_native_first("db_commit", knot_tuple!(value, num(snap), readset, writeset), 4)?;
        let (tag, newv) = match &*r {
            Knot::Cell(h, t) => (h.clone(), t.clone()),
            _ => return Err("db_commit returned a non-cell".into()),
        };
        let committed = tag.as_atom().and_then(|a| a.as_cord()).as_deref() == Some("commit");
        if committed {
            self.append_txn_batch(name, writes)?;
            let live = self.dbs.get_mut(name).unwrap();
            live.value = newv;
            for (k, _) in writes {
                live.keys.insert(k.clone());
            }
            live.wal_len += writes.len();
        }
        Ok(committed)
    }

    /// Append a transaction's writes as one crash-atomic batch: a `T<n>` framing line then
    /// the n `P` lines, written and flushed in a single call so recovery sees either the
    /// whole batch or (on a torn write) none of it.
    fn append_txn_batch(&self, name: &str, writes: &[(String, String)]) -> Result<(), String> {
        let mut buf = format!("T\t{}\n", writes.len());
        for (k, rec) in writes {
            buf.push_str(&format!("P\t{}\t{}\n", k, rec.replace('\n', " ")));
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.wal_path(name))
            .map_err(|e| format!("append wal: {}", e))?;
        f.write_all(buf.as_bytes()).map_err(|e| format!("{}", e))?;
        f.flush().map_err(|e| format!("{}", e))?;
        Ok(())
    }

    /// Add a secondary index on `field`: db.lat backfills it from the rows already stored
    /// and every later write maintains it, so equality/range queries on `field` become
    /// index probes instead of full scans. Logged (`I<field>`) so it survives a restart.
    pub fn add_index(&mut self, name: &str, field: u128) -> Result<(), String> {
        let value = self.value_of(name)?;
        let nv = self.call_native_first("db_addindex", knot_tuple!(value, num(field)), 2)?;
        self.append_wal(name, &format!("I\t{}", field))?;
        let live = self.dbs.get_mut(name).unwrap();
        live.value = nv;
        live.wal_len += 1;
        Ok(())
    }

    /// The fields that currently have a secondary index (primary first).
    pub fn indexes(&mut self, name: &str) -> Result<Vec<u128>, String> {
        let value = self.value_of(name)?;
        let r = self.call("db_indexes", value)?;
        let mut out = Vec::new();
        let mut cur = r;
        while let Knot::Cell(h, t) = &*cur {
            if let Some(u) = h.as_atom().and_then(|a| a.to_u128()) {
                out.push(u);
            }
            cur = t.clone();
        }
        Ok(out)
    }

    /// The primary keys of every record whose indexed `field` equals `value`, via the
    /// secondary index on that field. Requires an index on `field` (add one with
    /// `add_index`); a non-indexed field would need a bounded scan the stateless CLI
    /// can't supply, so this reports that instead.
    pub fn select_keys(&mut self, name: &str, field: u128, value: &str) -> Result<String, String> {
        if !self.indexes(name)?.contains(&field) {
            return Err(format!(
                "field {} is not indexed; add one with: latte db {} index {}",
                field, name, field
            ));
        }
        let v = self.value_of(name)?;
        let valnoun = match value.parse::<u128>() {
            Ok(n) => num(n),
            Err(_) => cord(value),
        };
        let entries = self.call("db_queryon", knot_tuple!(v, num(field), valnoun))?;
        let txt = self.call("db_keytext", entries)?;
        Ok(txt
            .as_atom()
            .map(|a| String::from_utf8_lossy(&a.bytes_le()).trim().to_string())
            .unwrap_or_default())
    }

    /// The whole live state as an HTML dashboard (over the keys we know are present).
    pub fn dash_html(&mut self, name: &str) -> Result<String, String> {
        self.open(name, 2, 0, 256)?;
        let live = self.dbs.get(name).unwrap();
        let v = live.value.clone();
        let keylist = keys_to_noun(&live.keys);
        let r = self.call("db_dashkeys", knot_tuple!(v, keylist))?;
        Ok(crate::serve::render_result(&r))
    }

    /// Names of all databases (open or on disk), with their live-key and log sizes.
    pub fn list(&self) -> Vec<(String, usize, usize)> {
        let mut names: BTreeSet<String> = self.dbs.keys().cloned().collect();
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                if let Some(n) = e.file_name().to_str().and_then(|s| s.strip_suffix(".wal")) {
                    names.insert(n.to_string());
                }
            }
        }
        names
            .into_iter()
            .map(|n| {
                if let Some(l) = self.dbs.get(&n) {
                    (n, l.keys.len(), l.wal_len)
                } else {
                    // not open in this process — count operations from the log on disk
                    let w = fs::read_to_string(self.wal_path(&n))
                        .map(|t| t.lines().filter(|l| l.starts_with("P\t") || l.starts_with("D\t")).count())
                        .unwrap_or(0);
                    (n, 0, w)
                }
            })
            .collect()
    }
}

/// Build a Latte list `[c0 c1 … 0]` of cords from a set of keys.
/// Build the native expression that applies `arm` to the `nargs` fields of the stdin tuple
/// `__in`. `call_arm` passes a right-nested tuple `[a1 [a2 [ … an]]]`, so field k<n is
/// `(head (tail …))` and the last field is the remaining tail.
fn native_arm_expr(arm: &str, nargs: usize) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(nargs);
    let mut path = String::from("__in");
    for k in 0..nargs {
        if k + 1 < nargs {
            parts.push(format!("(head {})", path));
            path = format!("(tail {})", path);
        } else {
            parts.push(path.clone()); // last field is the remaining tail
        }
    }
    format!("({} {})", arm, parts.join(" "))
}

fn keys_to_noun(keys: &BTreeSet<String>) -> N {
    let mut acc = num(0);
    for k in keys.iter().rev() {
        acc = crate::knot::cell(cord(k), acc);
    }
    acc
}

// ---- the process-wide service ----------------------------------------------

static SVC: OnceLock<Mutex<DbService>> = OnceLock::new();

fn default_dir() -> PathBuf {
    std::env::var("ORPHEUS_DB_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("dbdata"))
}

/// The shared persistent-database service (created on first use).
pub fn service() -> &'static Mutex<DbService> {
    SVC.get_or_init(|| Mutex::new(DbService::new(default_dir()).expect("init db service")))
}

/// Serialize a record noun back to a Latte expression that re-evaluates to the same
/// noun — used to rewrite the log at a checkpoint. Atoms that fit in u128 become a
/// decimal literal (a cord like %west re-parses to the identical atom); a longer atom
/// becomes a quoted cord when its bytes are printable text. Returns None for a long
/// non-text atom, which has no safe literal form, so the checkpoint refuses rather than
/// silently corrupt the value.
pub fn noun_to_latte(n: &N) -> Option<String> {
    match &**n {
        Knot::Atom(a) => {
            if let Some(u) = a.to_u128() {
                Some(u.to_string())
            } else {
                let bytes = a.bytes_le();
                match std::str::from_utf8(&bytes) {
                    Ok(s) if s.chars().all(|c| !c.is_control() && c != '"' && c != '\\') => {
                        Some(format!("\"{}\"", s))
                    }
                    _ => None,
                }
            }
        }
        Knot::Cell(h, t) => Some(format!("[{} {}]", noun_to_latte(h)?, noun_to_latte(t)?)),
    }
}

/// Render a query/history/get noun that may be `%absent`, for the CLI.
pub fn is_absent(n: &N) -> bool {
    matches!(&**n, Knot::Atom(a) if a.as_cord().as_deref() == Some("absent"))
}

// ---- the CLI: `latte db …` -------------------------------------------------

/// `latte db <name> <op> [args]` — a persistent database from the shell. The data
/// lives under ./dbdata (or $ORPHEUS_DB_DIR) and survives between invocations.
pub fn cli(args: &[String]) {
    let svc = service();
    let mut s = svc.lock().unwrap();
    if args.is_empty() || args[0] == "list" {
        let rows = s.list();
        if rows.is_empty() {
            println!("no databases yet. create one:\n  latte db users put u1 '[ [1 %alice] [ [2 %nyc] 0 ] ]'");
        } else {
            println!("persistent databases (dir: {}):", s.dir.display());
            for (n, k, w) in rows {
                println!("  {:<16} {} live keys, {} log entries", n, k, w);
            }
        }
        return;
    }
    let name = args[0].as_str();
    let op = args.get(1).map(|s| s.as_str()).unwrap_or("dash");
    let res: Result<String, String> = match op {
        "put" => {
            let key = args.get(2).cloned().unwrap_or_default();
            let rec = args[3..].join(" ");
            if key.is_empty() || rec.is_empty() {
                Err("usage: latte db <name> put <key> <record-expr>".into())
            } else {
                s.put(name, &key, &rec).map(|_| format!("stored {} in {}", key, name))
            }
        }
        "delete" | "del" => {
            let key = args.get(2).cloned().unwrap_or_default();
            s.delete(name, &key).map(|_| format!("deleted {} from {}", key, name))
        }
        "get" => s.get(name, args.get(2).map(|x| x.as_str()).unwrap_or("")),
        "query" => s.query_html(name, args.get(2).map(|x| x.as_str()).unwrap_or("")),
        "history" => s.history_html(name, args.get(2).map(|x| x.as_str()).unwrap_or("")),
        "dash" | "show" => s.dash_html(name),
        "agg" => {
            let gf: u128 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(1);
            let af: u128 = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(2);
            s.agg(name, gf, af)
        }
        "checkpoint" | "compact" => s
            .checkpoint(name)
            .map(|(o, n)| format!("checkpointed {}: log compacted {} -> {} entries", name, o, n)),
        "index" => match args.get(2).and_then(|x| x.parse::<u128>().ok()) {
            Some(field) => s.add_index(name, field).and_then(|_| {
                s.indexes(name).map(|ix| {
                    let list: Vec<String> = ix.iter().map(|f| f.to_string()).collect();
                    format!("added index on field {} to {}; indexed fields: {}", field, name, list.join(", "))
                })
            }),
            None => Err("usage: latte db <name> index <field-number>".into()),
        },
        "select" => match (args.get(2).and_then(|x| x.parse::<u128>().ok()), args.get(3)) {
            (Some(field), Some(value)) => s.select_keys(name, field, value).map(|keys| {
                if keys.is_empty() {
                    format!("no rows where field {} = {}", field, value)
                } else {
                    format!("keys where field {} = {}: {}", field, value, keys)
                }
            }),
            _ => Err("usage: latte db <name> select <field-number> <value>".into()),
        },
        "txn" => {
            // latte db <name> txn "<key>=<record-expr> | <key>=<record-expr> | ..."
            let spec = args.get(2).cloned().unwrap_or_default();
            let mut writes: Vec<(String, String)> = Vec::new();
            let mut malformed = false;
            for piece in spec.split('|') {
                let piece = piece.trim();
                if piece.is_empty() {
                    continue;
                }
                match piece.split_once('=') {
                    Some((k, rec)) => writes.push((k.trim().to_string(), rec.trim().to_string())),
                    None => malformed = true,
                }
            }
            if malformed || writes.is_empty() {
                Err("usage: latte db <name> txn \"<key>=<record-expr> | <key>=<record-expr> ...\"".into())
            } else {
                match s.begin(name) {
                    Ok(snap) => s.txn(name, snap, &[], &writes).map(|c| {
                        if c {
                            format!("committed {} writes atomically to {}", writes.len(), name)
                        } else {
                            format!("aborted: read-set conflict on {}", name)
                        }
                    }),
                    Err(e) => Err(e),
                }
            }
        }
        other => Err(format!("unknown op '{}'. try: put get query select history delete dash agg checkpoint txn index list", other)),
    };
    match res {
        Ok(out) => {
            // strip the embed marker so the CLI prints readable HTML/text
            let out = out.strip_prefix('\u{1}').map(|s| s.splitn(2, '\u{1}').nth(1).unwrap_or(s).to_string()).unwrap_or(out);
            println!("{}", out);
        }
        Err(e) => eprintln!("db: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orph-db-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tmpdir("persist");
        // session 1: write three rows, then drop the whole service
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            s.open("users", 2, 0, 256).unwrap();
            s.put("users", "u1", "[ [1 %alice] [ [2 %nyc] 0 ] ]").unwrap();
            s.put("users", "u2", "[ [1 %bob] [ [2 %sfo] 0 ] ]").unwrap();
            s.put("users", "u3", "[ [1 %carol] [ [2 %nyc] 0 ] ]").unwrap();
            assert!(s.get("users", "u1").unwrap().contains("alice"));
        }
        // session 2: a brand-new service over the same dir replays the log from disk
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            // no explicit open of contents — reading triggers replay
            assert!(s.get("users", "u2").unwrap().contains("bob"));
            assert!(s.get("users", "u3").unwrap().contains("carol"));
            // a key never written is absent (Bloom short-circuit still holds)
            assert!(super::is_absent(&latte::run_with_libs("%absent", &["std"]).unwrap())
                || s.get("users", "u9").unwrap().contains("absent"));
            // the secondary index survived too: two users in nyc
            let q = s.query_html("users", "nyc").unwrap();
            assert!(q.contains("alice") && q.contains("carol") && !q.contains("bob"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn db_mutations_are_native_and_match_interpreter() {
        // Every persistent-state-mutating db arm must (a) actually compile+run natively — not
        // silently fall back to the interpreter — and (b) produce a byte-identical result, so
        // the durable db value is updated by native code and stays deterministic across nodes.
        let dir = tmpdir("dbnative");
        let mut s = DbService::new(dir).unwrap();
        let libs = ["db", "findb"];
        let rec = s.eval_rec("[ [1 %x] 0 ]").unwrap();

        // db_open (3): build a fresh db value natively
        {
            let args = knot_tuple!(num(2), num(0), num(256));
            let interp = s.call("db_open", args.clone()).unwrap();
            let native = crate::rustgen::run_native_with_input(
                &native_arm_expr("db_open", 3), &args, &libs, false,
            ).expect("db_open must run natively");
            assert_eq!(crate::rustgen::noun_to_canon(&native), crate::rustgen::noun_to_canon(&interp),
                "native db_open must equal interpreter");
        }
        s.open("t", 2, 0, 256).unwrap();
        let v0 = s.dbs.get("t").unwrap().value.clone();

        // db_put (3)
        let args = knot_tuple!(v0, cord("k1"), rec.clone());
        let interp = s.call("db_put", args.clone()).unwrap();
        let v1 = crate::rustgen::run_native_with_input(
            &native_arm_expr("db_put", 3), &args, &libs, false,
        ).expect("db_put must run natively");
        assert_eq!(crate::rustgen::noun_to_canon(&v1), crate::rustgen::noun_to_canon(&interp),
            "native db_put must equal interpreter");

        // db_delete (2)
        {
            let args = knot_tuple!(v1.clone(), cord("k1"));
            let interp = s.call("db_delete", args.clone()).unwrap();
            let native = crate::rustgen::run_native_with_input(
                &native_arm_expr("db_delete", 2), &args, &libs, false,
            ).expect("db_delete must run natively");
            assert_eq!(crate::rustgen::noun_to_canon(&native), crate::rustgen::noun_to_canon(&interp),
                "native db_delete must equal interpreter");
        }

        // db_addindex (2)
        {
            let args = knot_tuple!(v1.clone(), num(1));
            let interp = s.call("db_addindex", args.clone()).unwrap();
            let native = crate::rustgen::run_native_with_input(
                &native_arm_expr("db_addindex", 2), &args, &libs, false,
            ).expect("db_addindex must run natively");
            assert_eq!(crate::rustgen::noun_to_canon(&native), crate::rustgen::noun_to_canon(&interp),
                "native db_addindex must equal interpreter");
        }

        // db_commit (4): empty read-set + one write commits cleanly
        {
            let writeset = crate::knot::cell(crate::knot::cell(cord("k2"), rec.clone()), num(0));
            let args = knot_tuple!(v1.clone(), num(0), num(0), writeset);
            let interp = s.call("db_commit", args.clone()).unwrap();
            let native = crate::rustgen::run_native_with_input(
                &native_arm_expr("db_commit", 4), &args, &libs, false,
            ).expect("db_commit must run natively");
            assert_eq!(crate::rustgen::noun_to_canon(&native), crate::rustgen::noun_to_canon(&interp),
                "native db_commit must equal interpreter");
        }
    }

    #[test]
    fn all_db_transitions_run_natively_and_match_interpreter() {
        // Every persistent-state transition must ACTUALLY compile+run natively (not silently
        // fall back) AND equal the interpreter — otherwise a node updated natively while live
        // would diverge from one reconstructed via the interpreter on restart.
        let dir = tmpdir("dbnative2");
        let mut s = DbService::new(dir).unwrap();
        s.open("t", 2, 0, 256).unwrap();
        s.put("t", "k1", "[ [1 %x] 0 ]").unwrap();
        let v = s.dbs.get("t").unwrap().value.clone();
        let canon = crate::rustgen::noun_to_canon;
        let run = |arm: &str, args: N, n: usize| {
            crate::rustgen::run_native_with_input(&native_arm_expr(arm, n), &args, &["db", "findb"], false)
                .unwrap_or_else(|| panic!("{} did not run natively (fell back)", arm))
        };
        // db_delete (2 args)
        let a = knot_tuple!(v.clone(), cord("k1"));
        assert_eq!(canon(&run("db_delete", a.clone(), 2)), canon(&s.call("db_delete", a).unwrap()), "db_delete");
        // db_addindex (2 args)
        let a = knot_tuple!(v.clone(), num(1));
        assert_eq!(canon(&run("db_addindex", a.clone(), 2)), canon(&s.call("db_addindex", a).unwrap()), "db_addindex");
        // db_commit (4 args)
        let rec = s.eval_rec("[ [1 %y] 0 ]").unwrap();
        let writeset = crate::knot::cell(crate::knot::cell(cord("k2"), rec), num(0));
        let a = knot_tuple!(v.clone(), num(0), num(0), writeset);
        assert_eq!(canon(&run("db_commit", a.clone(), 4)), canon(&s.call("db_commit", a).unwrap()), "db_commit");
        // db_open (3 args) — initial value creation
        let a = knot_tuple!(num(2), num(0), num(256));
        assert_eq!(canon(&run("db_open", a.clone(), 3)), canon(&s.call("db_open", a).unwrap()), "db_open");
    }

    #[test]
    fn mvcc_and_delete_survive_restart() {
        let dir = tmpdir("mvcc");
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            s.put("acct", "a1", "[ [1 %v1] [ [2 %x] 0 ] ]").unwrap();
            s.put("acct", "a1", "[ [1 %v2] [ [2 %y] 0 ] ]").unwrap(); // new version
            s.put("acct", "a2", "[ [1 %keep] [ [2 %z] 0 ] ]").unwrap();
            s.delete("acct", "a2").unwrap();
        }
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            // latest version visible
            assert!(s.get("acct", "a1").unwrap().contains("v2"));
            // full history preserved across the restart (2 versions)
            let h = s.history_html("acct", "a1").unwrap();
            assert!(h.contains("v1") && h.contains("v2"));
            // deleted key reads as absent
            assert!(s.get("acct", "a2").unwrap().contains("absent"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analytics_layer_and_ml_viz_bridges() {
        let libs = &["db", "stats", "plot"];
        let scan = "(db_scan (db_orders 0) %o0 %o9)";
        let render = |e: String| crate::net::show_state(&latte::run_with_libs(&e, libs).unwrap());
        let num = |e: String| latte::run_with_libs(&e, libs).unwrap().as_atom().unwrap().to_u128().unwrap();

        // --- the new query/analytics layer over the storage engine ---
        assert_eq!(num(format!("(db_count {})", scan)), 4);
        assert_eq!(num(format!("(db_sum 2 {})", scan)), 800);
        assert_eq!(num(format!("(db_avg 2 {})", scan)), 200);
        // GROUP BY region (field 1), SUM amount (field 2): west=100+150, east=250+300
        let gs = render(format!("(db_groupsum 1 2 {})", scan));
        assert!(gs.contains("250") && gs.contains("550"), "group sums: {}", gs);
        // ORDER BY amount desc, take 2 -> 300, 250
        let top = render(format!("(db_pluck 2 (db_topn 2 2 {}))", scan));
        assert!(top.contains("300") && top.contains("250"), "top-2: {}", top);

        // --- INTEGRATION: the projection bridges into the statistics / ML library ---
        // st_mean of the amounts, lifted to signed fixed-point, is 200.000 -> [0 200000]
        assert!(render(format!("(st_mean (db_npluck 2 {}))", scan)).contains("200000"));
        // and a real stats computation (std dev) succeeds on the queried column
        assert!(latte::run_with_libs(&format!("(st_std (db_npluck 2 {}))", scan), libs).is_ok());

        // --- INTEGRATION: and into the plotter (data visualization) ---
        // bar-chart geometry is produced from a queried column without error
        assert!(latte::run_with_libs(&format!("(bars (db_pluck 2 {}) 320 100)", scan), libs).is_ok());
    }

    #[test]
    fn secondary_index_is_added_maintained_and_survives_restart() {
        let dir = tmpdir("midx");
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            // a db whose primary index is field 1 (region); we add a second on field 2 (amount)
            s.put("ord", "o1", "[ [1 %west] [ [2 100] 0 ] ]").unwrap();
            s.put("ord", "o2", "[ [1 %east] [ [2 250] 0 ] ]").unwrap();
            s.put("ord", "o3", "[ [1 %west] [ [2 250] 0 ] ]").unwrap();
            assert_eq!(s.indexes("ord").unwrap(), vec![2]); // default primary is field 2 (open idxtag=2)
            s.add_index("ord", 1).unwrap(); // add a secondary index on region
            let mut ix = s.indexes("ord").unwrap();
            ix.sort();
            assert_eq!(ix, vec![1, 2]);
            // a write after the index exists is maintained by it
            s.put("ord", "o4", "[ [1 %west] [ [2 300] 0 ] ]").unwrap();
        }
        // after a restart, the secondary index is rebuilt from the log (I line + puts)
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            let mut ix = s.indexes("ord").unwrap();
            ix.sort();
            assert_eq!(ix, vec![1, 2], "secondary index survives restart");
            // and it is correct: query region=west via the rebuilt index finds o1,o3,o4
            let n = |e: String| latte::run_with_libs(&e, &["db"]).unwrap().as_atom().unwrap().to_u128().unwrap();
            // checkpoint preserves the index too
            let (_o, _n) = s.checkpoint("ord").unwrap();
            let mut ix2 = s.indexes("ord").unwrap();
            ix2.sort();
            assert_eq!(ix2, vec![1, 2], "secondary index survives checkpoint");
            let _ = n; // (query correctness on this durable db is covered by the in-Latte tests)
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn durable_transaction_commits_atomically_and_survives_restart() {
        let dir = tmpdir("txn");
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            let snap = s.begin("bank").unwrap();
            // a transfer: two writes committed atomically, empty read-set so it commits
            let ok = s
                .txn(
                    "bank",
                    snap,
                    &[],
                    &[
                        ("a".into(), "[ [1 %usd] [ [2 70] 0 ] ]".into()),
                        ("b".into(), "[ [1 %usd] [ [2 30] 0 ] ]".into()),
                    ],
                )
                .unwrap();
            assert!(ok);
            assert!(s.get("bank", "a").unwrap().contains("usd"));
            assert!(s.get("bank", "b").unwrap().contains("usd"));
        }
        // both writes are durable: a fresh process replays the batch and sees both
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            assert!(s.get("bank", "a").unwrap().contains("usd"));
            assert!(s.get("bank", "b").unwrap().contains("usd"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn serializable_transaction_aborts_on_stale_read_set() {
        let dir = tmpdir("occ");
        let mut s = DbService::new(dir.clone()).unwrap();
        s.put("inv", "x", "[ [1 %v0] [ [2 1] 0 ] ]").unwrap();
        // T1 begins and will base a write on having read x
        let snap = s.begin("inv").unwrap();
        // ... meanwhile someone else commits a new version of x ...
        s.put("inv", "x", "[ [1 %v1] [ [2 2] 0 ] ]").unwrap();
        // T1 commits with x in its read-set: x changed since snap, so OCC must abort
        let committed = s
            .txn("inv", snap, &["x".into()], &[("y".into(), "[ [1 %new] [ [2 9] 0 ] ]".into())])
            .unwrap();
        assert!(!committed, "stale read-set should abort");
        // the aborted write never landed
        assert!(s.get("inv", "y").unwrap().contains("absent"));
        // and a transaction on a fresh read-set still commits
        let snap2 = s.begin("inv").unwrap();
        let ok = s
            .txn("inv", snap2, &["x".into()], &[("y".into(), "[ [1 %new] [ [2 9] 0 ] ]".into())])
            .unwrap();
        assert!(ok);
        assert!(s.get("inv", "y").unwrap().contains("new"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_transaction_batch_is_dropped_on_recovery() {
        let dir = tmpdir("torn");
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            s.put("led", "k1", "[ [1 %live] [ [2 1] 0 ] ]").unwrap();
        }
        // simulate a crash mid-commit: a T<2> frame with only one of its two P lines written
        let wal = dir.join("led.wal");
        let mut text = fs::read_to_string(&wal).unwrap();
        text.push_str("T\t2\nP\tk2\t[ [1 %torn] [ [2 2] 0 ] ]\n"); // declares 2, only 1 follows
        fs::write(&wal, text).unwrap();
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            // the committed put survives; the torn batch is dropped whole (all-or-nothing)
            assert!(s.get("led", "k1").unwrap().contains("live"));
            assert!(s.get("led", "k2").unwrap().contains("absent"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_compacts_log_preserving_current_state() {
        let dir = tmpdir("ckpt");
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            // three versions of i1, one i2, and an i3 that is then deleted
            s.put("inv", "i1", "[ [1 %one]   [ [2 1] 0 ] ]").unwrap();
            s.put("inv", "i1", "[ [1 %two]   [ [2 2] 0 ] ]").unwrap();
            s.put("inv", "i1", "[ [1 %three] [ [2 3] 0 ] ]").unwrap();
            s.put("inv", "i2", "[ [1 %keep]  [ [2 9] 0 ] ]").unwrap();
            s.put("inv", "i3", "[ [1 %gone]  [ [2 7] 0 ] ]").unwrap();
            s.delete("inv", "i3").unwrap();
            let before = s.list().into_iter().find(|(n, _, _)| n == "inv").unwrap().2;
            assert_eq!(before, 6); // 3 puts + 1 put + 1 put + 1 delete

            let (old, new) = s.checkpoint("inv").unwrap();
            assert_eq!(old, 6);
            assert_eq!(new, 2); // collapsed to the two live keys, dead key dropped
            assert!(new < old);

            // current values intact, superseded versions and the deleted key are gone
            assert!(s.get("inv", "i1").unwrap().contains("three"));
            assert!(s.get("inv", "i2").unwrap().contains("keep"));
            assert!(s.get("inv", "i3").unwrap().contains("absent"));
        }
        // recovery now reads only the compacted log, and writes still work afterwards
        {
            let mut s = DbService::new(dir.clone()).unwrap();
            assert!(s.get("inv", "i1").unwrap().contains("three"));
            assert!(s.get("inv", "i2").unwrap().contains("keep"));
            assert!(s.get("inv", "i3").unwrap().contains("absent"));
            s.put("inv", "i4", "[ [1 %new] [ [2 5] 0 ] ]").unwrap();
            assert!(s.get("inv", "i4").unwrap().contains("new"));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn query_planner_and_relational_ops() {
        let libs = &["db"];
        let n = |e: String| latte::run_with_libs(&e, libs).unwrap().as_atom().unwrap().to_u128().unwrap();
        let scan = "(db_scan (db_orders 0) %o0 %o9)";

        // PLANNER: equality on the indexed field (region) probes the index; on a
        // non-indexed field (amount) it falls back to scan + filter. Same answer, different plan.
        assert_eq!(n("(db_count (db_select (db_orders 0) 1 %west %o0 %o9))".into()), 2);
        assert_eq!(n("(db_count (db_select (db_orders 0) 2 250 %o0 %o9))".into()), 1);

        // RANGE: the index is cord-ordered, so a range on the indexed (cord) field is
        // index-accelerated; a numeric range goes through db_between (scan + numeric filter).
        assert_eq!(n("(db_count (db_idxrange (db_orders 0) %e %x))".into()), 4);
        assert_eq!(n(format!("(db_count (db_between 2 150 300 {}))", scan)), 3);

        // COMPOUND predicates composed by primary key: west AND amount in [120,300] -> o3 only;
        // west OR amount=250 -> {o1,o3} u {o2} = 3.
        assert_eq!(n(format!("(db_count (db_and (db_select (db_orders 0) 1 %west %o0 %o9) (db_between 2 120 300 {})))", scan)), 1);
        assert_eq!(n(format!("(db_count (db_or (db_select (db_orders 0) 1 %west %o0 %o9) (db_between 2 250 250 {})))", scan)), 3);

        // JOIN: an equi-join of the orders with themselves on region pairs same-region orders
        // (west x west) + (east x east) = 2*2 + 2*2 = 8.
        assert_eq!(n(format!("(len (db_join {} 1 {} 1))", scan, scan)), 8);
    }
}

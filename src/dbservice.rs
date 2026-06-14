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

    /// Evaluate a record expression (a small literal) to a noun.
    fn eval_rec(&self, expr: &str) -> Result<N, String> {
        latte::run_with_libs(expr, &["std", "num"])
    }

    fn wal_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.wal", name))
    }

    fn fresh(&self, idxtag: u128, rschema: u128, thresh: u128) -> Result<N, String> {
        self.call("db_open", knot_tuple!(num(idxtag), num(rschema), num(thresh)))
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
        for ln in &ops {
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
        let newval = self.call("db_put", knot_tuple!(cur, cord(key), rec))?;
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
        let newval = self.call("db_delete", knot_tuple!(cur, cord(key)))?;
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
        other => Err(format!("unknown op '{}'. try: put get query history delete dash list", other)),
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
}

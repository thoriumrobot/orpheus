//! Mocha — the application environment of Orpheus.
//!
//! Mocha runs higher-level *apps* that are written in Latte (see `lib/mocha.lat`,
//! `lib/todo.lat`, `lib/lexicon.lat`). An app exposes `poke` (a state transition) and
//! `peek` (a read-only view). Mocha hosts an app on the same persistent, distributed
//! runtime the rest of Orpheus uses: each poke becomes a durable, gossiped event, so an
//! app automatically gains persistence, strong-eventual-consistency convergence,
//! time-travel, and log compaction. The host here is a thin shell — command parsing,
//! I/O, and the bridge to SCArs — while all app logic lives in Latte.

use crate::agent::Agent;
use crate::atom::Atom;
use crate::knot::{cell, cord, num, Knot, N};
use crate::loom::{edit, slot, tar, Crash};
use crate::net::{self, Config, Node};
use crate::{latte, sca};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TODO_LAT: &str = include_str!("../lib/todo.lat");
pub const LEXICON_LAT: &str = include_str!("../lib/lexicon.lat");
pub const FORGE_LAT: &str = include_str!("../lib/forge.lat");
pub const EDITOR_LAT: &str = include_str!("../lib/editor.lat");
/// Networked chess as a Mocha app (moves gossip between connected machines).
pub const CHESSGAME_LAT: &str = include_str!("../lib/chessgame.lat");

fn app_source(name: &str) -> Option<&'static str> {
    match name {
        "todo" => Some(TODO_LAT),
        "lexicon" => Some(LEXICON_LAT),
        "forge" => Some(FORGE_LAT),
        "editor" => Some(EDITOR_LAT),
        "chessgame" => Some(CHESSGAME_LAT),
        _ => None,
    }
}

/// A loaded Mocha app: the compiled core plus the `peek` arm's axis. (Poke runs through
/// an `Agent` on the `Node`, which already understands `poke`.)
pub struct Mocha {
    core: N,
    peek_axis: u128,
}

impl Mocha {
    pub fn load(src: &str) -> Result<Mocha, String> {
        let (core, axes) = latte::compile_module(src)?;
        let peek_axis = axes
            .iter()
            .find(|(n, _)| n == "peek")
            .map(|(_, a)| *a)
            .ok_or_else(|| "app has no `peek` arm".to_string())?;
        Ok(Mocha { core, peek_axis })
    }

    /// Evaluate a read-only view: sample = [query state], run `peek`.
    pub fn peek(&self, query: &N, state: &N) -> Result<N, Crash> {
        let sample = cell(query.clone(), state.clone());
        let core2 = edit(&Atom::from_u128(3), &sample, &self.core)?;
        let armf = slot(&Atom::from_u128(self.peek_axis), &core2)?;
        tar(&core2, &armf)
    }
}

// ----- command parsing ------------------------------------------------------
/// Turn a shell line ("add buy milk") into an action noun [tag arg]. For the lexicon's
/// `add`, the host derives the Heart form with SCArs and stores [solar heart].
fn parse_cmd(app: &str, line: &str) -> Option<N> {
    // split off the tag only: the argument keeps its whitespace verbatim
    // (a forge snippet's formatting is part of the snippet)
    let line = line.trim_start();
    let (tag, rest) = match line.split_once(char::is_whitespace) {
        Some((t, r)) => (t, r.trim_start()),
        None => (line, ""),
    };
    if tag.is_empty() {
        return None;
    }
    let rest = rest.to_string();
    let arg = if app == "lexicon" && tag == "add" {
        let heart = sca::evolve(&rest).unwrap_or_else(|_| rest.clone());
        cell(cord(&rest), cord(&heart))
    } else if app == "forge" && tag == "add" {
        // "add <author> <name> <code...>" -> [author [name code]]
        let mut w = rest.splitn(3, char::is_whitespace);
        let author = w.next().unwrap_or("anon");
        let name = w.next().unwrap_or("snippet");
        let code = w.next().unwrap_or("");
        cell(cord(author), cell(cord(name), cord(code)))
    } else if app == "forge" && tag == "del" {
        cord(&rest)
    } else if rest.is_empty() {
        num(0)
    } else {
        cord(&rest)
    };
    Some(cell(cord(tag), arg))
}

// ----- rendering ------------------------------------------------------------
fn cord_or_num(a: &Atom) -> String {
    match a.as_cord() {
        Some(s) if !s.is_empty() && s.bytes().all(|b| b >= 0x20) => s,
        _ => a.to_u128().map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
    }
}

fn render_elem(n: &N) -> String {
    match &**n {
        Knot::Atom(a) => cord_or_num(a),
        Knot::Cell(h, t) => match (h.as_atom(), t.as_atom()) {
            (Some(a), Some(b)) => format!("{} → {}", cord_or_num(a), cord_or_num(b)),
            _ => {
                // a forge entry [author [name code]]: "name (author): code"
                if let (Knot::Atom(a), Knot::Cell(nm, code)) = (&**h, &**t) {
                    if let (Some(nm), Some(code)) = (nm.as_atom(), code.as_atom()) {
                        let c = code.as_text().unwrap_or_else(|| cord_or_num(&code));
                        let first = c.lines().next().unwrap_or("").to_string();
                        let more = c.lines().count().saturating_sub(1);
                        return format!(
                            "{} ({}): {}{}",
                            cord_or_num(&nm),
                            cord_or_num(a),
                            first,
                            if more > 0 { format!("  …+{} lines", more) } else { String::new() }
                        );
                    }
                }
                "[…]".into()
            }
        },
    }
}

fn render_list(mut n: N) -> Vec<String> {
    let mut out = Vec::new();
    while let Knot::Cell(h, t) = &*n {
        out.push(render_elem(h));
        n = t.clone();
    }
    out
}

fn render_peek(tag: &str, v: &N) -> String {
    match tag {
        "count" => v.as_atom().and_then(|a| a.to_u128()).map(|n| n.to_string()).unwrap_or_else(|| "0".into()),
        "list" | "all" | "names" | "by" => {
            let items = render_list(v.clone());
            if items.is_empty() {
                "(empty)".into()
            } else {
                items.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n")
            }
        }
        "has" => match v.as_atom().and_then(|a| a.to_u128()) {
            Some(0) => "yes".into(),
            _ => "no".into(),
        },
        // a forge `get`: print the snippet's CODE verbatim (ready to compile)
        "get" => match &**v {
            Knot::Atom(a) if a.is_zero() => "(not found)".into(),
            Knot::Cell(_, t) => match &**t {
                Knot::Cell(_, code) => code
                    .as_atom()
                    .and_then(|a| a.as_text())
                    .unwrap_or_else(|| render_elem(v)),
                _ => render_elem(v),
            },
            _ => render_elem(v),
        },
        "heart" => match &**v {
            Knot::Atom(a) if a.is_zero() => "(none)".into(),
            Knot::Atom(a) => cord_or_num(a),
            _ => render_elem(v),
        },
        "text" | "last" => render_elem(v),
        _ => net::show_state(v),
    }
}

// ----- CLI ------------------------------------------------------------------
pub fn cmd_mocha(args: &[String]) {
    if args.is_empty() {
        return demo();
    }
    let mut app = String::new();
    let mut store: Option<String> = None;
    let mut listen: Option<String> = None;
    let mut peers: Vec<String> = Vec::new();
    let mut pokes: Vec<String> = Vec::new();
    let mut peeks: Vec<String> = Vec::new();
    let mut run_secs: Option<u64> = None;
    let mut id: Option<u64> = None;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--app" => { i += 1; app = args[i].clone(); }
            "--store" => { i += 1; store = Some(args[i].clone()); }
            "--listen" => { i += 1; listen = Some(args[i].clone()); }
            "--peer" => { i += 1; peers.push(args[i].clone()); }
            "--poke" => { i += 1; pokes.push(args[i].clone()); }
            "--peek" => { i += 1; peeks.push(args[i].clone()); }
            "--run-secs" => { i += 1; run_secs = args[i].parse().ok(); }
            "--id" => { i += 1; id = args[i].parse().ok(); }
            "-v" | "--verbose" => { verbose = true; }
            other => { eprintln!("mocha: unknown arg {}", other); return; }
        }
        i += 1;
    }

    let src = match app_source(&app) {
        Some(s) => s,
        None => {
            eprintln!("mocha: unknown app '{}' (try: todo, lexicon)", app);
            return;
        }
    };
    let agent = match Agent::from_source(src, &app) {
        Ok(a) => a,
        Err(e) => { eprintln!("mocha: app failed to compile: {}", e); return; }
    };
    let q = match Mocha::load(src) {
        Ok(q) => q,
        Err(e) => { eprintln!("mocha: {}", e); return; }
    };
    let id = id.unwrap_or_else(|| {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        nanos ^ (std::process::id() as u64).wrapping_mul(2654435761)
    });

    let node = match &store {
        Some(dir) => match Node::open(id, agent, dir, 0) {
            Ok(n) => n,
            Err(e) => { eprintln!("mocha: cannot open store: {}", e); return; }
        },
        None => Node::new(id, agent),
    };
    println!("Mocha — app '{}' on Orpheus", app);

    if let Some(addr) = listen {
        // distributed: pokes become gossiped events; peeks read the converged state
        let handle: net::NodeHandle = Arc::new(Mutex::new(node));
        let cfg = Arc::new(Config { name: app.clone(), listen: addr, peers, verbose, compact_every: 0 });
        let peers_handle = net::start(handle.clone(), cfg);
        std::thread::sleep(Duration::from_millis(800)); // let links settle
        for p in &pokes {
            if let Some(act) = parse_cmd(&app, p) {
                net::submit(&handle, &peers_handle, act);
                println!("poke: {}", p);
            }
        }
        std::thread::sleep(Duration::from_millis(400));
        run_peeks(&q, &handle.lock().unwrap(), &peeks);
        if let Some(secs) = run_secs {
            std::thread::sleep(Duration::from_secs(secs));
        }
    } else {
        // local: pokes persist (if --store) and fold immediately
        let mut node = node;
        for p in &pokes {
            if let Some(act) = parse_cmd(&app, p) {
                node.local_action(act);
                println!("poke: {}", p);
            }
        }
        run_peeks(&q, &node, &peeks);
    }
}


/// `lattice team` — collaborative coding over connected machines, on the Forge app.
/// Each `--share` appends one snippet authored by `--as`; the shared log converges
/// across every peer. This is a thin front-end over `mocha --app forge`.
pub fn cmd_team(args: &[String]) {
    let mut who = String::from("anon");
    let mut name = String::from("snippet");
    let mut q: Vec<String> = vec!["--app".into(), "forge".into()];
    let mut shares: Vec<(String, String)> = Vec::new();
    let mut show = false;
    let mut get: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--as" => { i += 1; who = args.get(i).cloned().unwrap_or(who); }
            "--name" => { i += 1; name = args.get(i).cloned().unwrap_or(name); }
            "--share" => { i += 1; if let Some(c) = args.get(i) { shares.push((name.clone(), c.clone())); } }
            "--get" => { i += 1; get = args.get(i).cloned(); }
            "--names" => { q.push("--peek".into()); q.push("names".into()); }
            "--show" => { show = true; }
            // pass networking/store flags straight through to the Mocha runner
            "--listen" | "--peer" | "--store" | "--id" | "--run-secs" => {
                q.push(args[i].clone());
                i += 1;
                if let Some(v) = args.get(i) { q.push(v.clone()); }
            }
            "-v" | "--verbose" => q.push("-v".into()),
            other => { eprintln!("team: unknown arg {}", other); return; }
        }
        i += 1;
    }
    for (nm, c) in &shares {
        q.push("--poke".into());
        q.push(format!("add {} {} {}", who, nm, c));
    }
    if let Some(nm) = &get {
        q.push("--peek".into());
        q.push(format!("get {}", nm));
    }
    if show || (shares.is_empty() && get.is_none() && !q.contains(&"--peek".to_string())) {
        q.push("--peek".into());
        q.push("all".into());
    }
    cmd_mocha(&q);
}

fn run_peeks(q: &Mocha, node: &Node, peeks: &[String]) {
    let state = match node.state() {
        Ok(s) => s,
        Err(_) => { eprintln!("mocha: state crashed"); return; }
    };
    for query in peeks {
        let mut w = query.split_whitespace();
        let tag = w.next().unwrap_or("");
        let rest: String = w.collect::<Vec<_>>().join(" ");
        let q_arg = if rest.is_empty() { num(0) } else { cord(&rest) };
        let qnoun = cell(cord(tag), q_arg);
        match q.peek(&qnoun, &state) {
            Ok(v) => {
                let r = render_peek(tag, &v);
                if r.contains('\n') {
                    println!("peek {}:\n{}", query, r);
                } else {
                    println!("peek {} = {}", query, r);
                }
            }
            Err(_) => println!("peek {} = <crash>", query),
        }
    }
}

// ----- guided demo ----------------------------------------------------------
fn demo() {
    println!("Mocha — the Orpheus application environment\n");
    println!("Apps are written in Latte (poke + peek) and run on the persistent,");
    println!("distributed runtime: every poke is a durable, gossiped event.\n");

    // todo app
    println!("app 'todo' — a to-do list:");
    let src = TODO_LAT;
    let agent = Agent::from_source(src, "todo").unwrap();
    let q = Mocha::load(src).unwrap();
    let mut state = agent.initial_state();
    for cmd in ["add water the garden", "add write the spec", "add water the garden", "drop water the garden"] {
        let act = parse_cmd("todo", cmd).unwrap();
        state = agent.step(&act, &state).unwrap();
        println!("  poke: {}", cmd);
    }
    let count = q.peek(&cell(cord("count"), num(0)), &state).unwrap();
    let list = q.peek(&cell(cord("list"), num(0)), &state).unwrap();
    println!("  peek count = {}", render_peek("count", &count));
    println!("  peek list:\n{}", render_peek("list", &list));

    // lexicon app — integrates SCArs
    println!("\napp 'lexicon' — Solar→Heart dictionary (Heart forms derived by SCArs):");
    let src = LEXICON_LAT;
    let agent = Agent::from_source(src, "lexicon").unwrap();
    let q = Mocha::load(src).unwrap();
    let mut state = agent.initial_state();
    for solar in ["ligā", "nīvō", "mazdā"] {
        let act = parse_cmd("lexicon", &format!("add {}", solar)).unwrap();
        state = agent.step(&act, &state).unwrap();
        println!("  poke: add {}", solar);
    }
    let all = q.peek(&cell(cord("all"), num(0)), &state).unwrap();
    println!("  peek all:\n{}", render_peek("all", &all));
    let heart = q.peek(&cell(cord("heart"), cord("ligā")), &state).unwrap();
    println!("  peek heart ligā = {}", render_peek("heart", &heart));

    println!("\nRun a real one:  latte mocha --app todo --store /tmp/todo \\");
    println!("                   --poke \"add buy milk\" --peek count --peek list");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fold(app: &str, src: &str, cmds: &[&str]) -> (Mocha, Agent, N) {
        let agent = Agent::from_source(src, app).unwrap();
        let q = Mocha::load(src).unwrap();
        let mut state = agent.initial_state();
        for c in cmds {
            let act = parse_cmd(app, c).unwrap();
            state = agent.step(&act, &state).unwrap();
        }
        (q, agent, state)
    }

    #[test]
    fn chess_fold_native_matches_interpreter() {
        // The chess `poke` (domove→islegal→legal) is heavy enough that the agent's default
        // interpreter fold runs OUT OF FUEL — so before native folding, a chess node's
        // state() could not be materialized after real moves. Native folding has no such
        // ceiling. This test proves (a) native folds the game correctly, and (b) the native
        // result is identical to the reference interpreter run at a high fuel budget — so
        // persistence stays deterministic across machines.
        let mv = |f: u128, t: u128| cell(cord("move"), cell(num(f), cell(num(t), num(0))));
        let plies = [mv(52, 36), mv(12, 28), mv(62, 45), mv(1, 18)]; // e4 e5 Nf3 Nc6
        let agent = Agent::from_source(CHESSGAME_LAT, "chessgame").unwrap();
        let libs = ["std", "chess", "chessgame"];
        let mut st = agent.initial_state();
        let t0 = std::time::Instant::now();
        for act in &plies {
            let next = agent.step(act, &st).expect("native fold succeeds with no fuel limit");
            // reference: the same poke on the interpreter at a high fuel budget
            let expr = format!(
                "(tail (poke {} {}))",
                crate::dbservice::noun_to_latte(act).unwrap(),
                crate::dbservice::noun_to_latte(&st).unwrap()
            );
            let reference = latte::run_with_libs_fuel(&expr, &libs, 8_000_000_000)
                .expect("reference interpreter fold");
            assert_eq!(next, reference, "native fold must equal the interpreter reference");
            st = next;
        }
        eprintln!("[chess_fold] native==interp(high-fuel) over {} plies, {:?}", plies.len(), t0.elapsed());
        let q = Mocha::load(CHESSGAME_LAT).unwrap();
        let board = q.peek(&cell(cord("board"), num(0)), &st).unwrap();
        let init = q.peek(&cell(cord("board"), num(0)), &agent.initial_state()).unwrap();
        assert_ne!(board, init, "moves must have applied (board changed)");
        let side = q.peek(&cell(cord("side"), num(0)), &st).unwrap();
        assert_eq!(side.as_atom().unwrap().to_u128(), Some(1), "White to move after 4 plies");
    }

    #[test]
    fn todo_poke_and_peek() {
        let (q, _a, st) = fold("todo", TODO_LAT, &["add a", "add b", "add c", "drop b"]);
        let count = q.peek(&cell(cord("count"), num(0)), &st).unwrap();
        assert_eq!(count.as_atom().unwrap().to_u128(), Some(2));
        let has_b = q.peek(&cell(cord("has"), cord("b")), &st).unwrap();
        assert_eq!(has_b.as_atom().unwrap().to_u128(), Some(1)); // 1 = not a member
        let has_a = q.peek(&cell(cord("has"), cord("a")), &st).unwrap();
        assert_eq!(has_a.as_atom().unwrap().to_u128(), Some(0)); // 0 = present
    }

    #[test]
    fn todo_clear() {
        let (q, _a, st) = fold("todo", TODO_LAT, &["add a", "add b", "clear"]);
        let count = q.peek(&cell(cord("count"), num(0)), &st).unwrap();
        assert_eq!(count.as_atom().unwrap().to_u128(), Some(0));
    }

    #[test]
    fn lexicon_stores_scars_derivations() {
        let (q, _a, st) = fold("lexicon", LEXICON_LAT, &["add ligā", "add nīvō"]);
        let count = q.peek(&cell(cord("count"), num(0)), &st).unwrap();
        assert_eq!(count.as_atom().unwrap().to_u128(), Some(2));
        // the Heart form stored for "ligā" must match SCArs.evolve("ligā")
        let heart = q.peek(&cell(cord("heart"), cord("ligā")), &st).unwrap();
        let expect = sca::evolve("ligā").unwrap();
        assert_eq!(heart.as_atom().unwrap().as_cord().as_deref(), Some(expect.as_str()));
        assert!(!expect.is_empty() && expect != "ligā"); // the change actually fired
    }
}

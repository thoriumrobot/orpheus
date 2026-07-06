//! Distributed execution across connected Orpheus instances.
//!
//! The gossip layer (src/net.rs) makes connected instances agree on STATE;
//! this layer makes them share WORK. A worker (`latte worker`) is an Orpheus
//! instance that answers evaluation tasks over TCP: a task is a Latte
//! expression plus its library scope, the answer is the resulting noun. All
//! Latte programs are pure functions of their source, so a task computes the
//! same noun on any instance — remote execution is an *audited acceleration*
//! of a pure meaning (the jet principle, applied across machines).
//!
//! Two behaviors ride on that primitive, both ON BY DEFAULT once workers are
//! connected (`latte workers add HOST:PORT`, or ORPHEUS_WORKERS):
//!
//! 1. **Data-parallel evaluation.** The profiler (src/rustgen.rs) already
//!    measures every program it sees; it now also detects the distributable
//!    shapes — `(dmap f xs)`, `(map f xs)`, `(predict_all w b xs)` (ML batch
//!    prediction) — and the adaptive engine splits the list across the
//!    connected workers: `(map f chunk)` per worker, results concatenated.
//!    `dmap` (lib/dist.lat) distributes whenever workers exist — that is its
//!    meaning; a plain `map` distributes only once its MEASURED interpreter
//!    time crosses the distribution threshold (ORPHEUS_DIST_NS, default
//!    25 ms), below which network overhead could not pay. A failed worker's
//!    chunk falls back to local evaluation, so distribution never changes a
//!    result or loses one.
//!
//! 2. **Distributed model training** — local SGD with periodic model
//!    averaging (FedAvg; McMahan et al. 2017, arXiv:1602.05629). The data is
//!    round-robin sharded across workers; each ROUND every worker runs
//!    `train` (lib/ml.lat) for E local gradient steps on its own shard;
//!    the coordinator consolidates the returned models with `fedavg`
//!    (lib/dist.lat) — a weighted model average, Σ (n_k/n)·w_k, computed in
//!    Latte on Loom — and redistributes the consolidated model as the next
//!    round's starting point. Cycles of distributed execution and
//!    consolidation improve the model round over round (the report prints
//!    the full-data MSE after each consolidation); only the FINAL
//!    consolidated model is committed to the persistent, gossiped event log
//!    (one `%put` event on the kv agent) — the intermediate rounds never
//!    touch the log.
//!
//! Wire protocol (length-framed like src/net.rs, but request/response):
//!   TASK   [expr-cord [fuel libs]]   → evaluate expr with libs in scope
//!   RESULT [0 value] | [1 error-cord]

use crate::knot::{cell, cord, num, Knot, N};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const T_TASK: u8 = 0x11;
const T_RESULT: u8 = 0x12;
const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Fuel for remote tasks: model-training rounds are deliberate long
/// computations, so workers get a deep budget (the caller's own default
/// applies locally).
pub const TASK_FUEL: u64 = 2_000_000_000;

// ------------------------------- framing ------------------------------------

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

// ------------------------------- cons-list helpers ---------------------------

/// Collect a 0-terminated Latte cons list into a Vec of its elements.
pub fn to_vec(n: &N) -> Vec<N> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        out.push(h.clone());
        cur = t.clone();
    }
    out
}

/// Build a 0-terminated Latte cons list from elements.
pub fn from_vec(xs: &[N]) -> N {
    let mut out = num(0);
    for x in xs.iter().rev() {
        out = cell(x.clone(), out);
    }
    out
}

/// Render a noun as Latte source that evaluates back to the same noun:
/// atoms as decimal literals, cells as `[ h t ]`. Returns None when an atom
/// exceeds the lexer's u128 literal range (the caller then keeps the work
/// local — degradation is always to the correct, slower path).
pub fn noun_literal(n: &N) -> Option<String> {
    match &**n {
        Knot::Atom(a) => a.to_u128().map(|v| v.to_string()),
        Knot::Cell(h, t) => Some(format!("[ {} {} ]", noun_literal(h)?, noun_literal(t)?)),
    }
}

// ------------------------------- AST → source --------------------------------

/// Render a parsed Latte expression back to source text that parses to the
/// same AST. Used to lift the function argument of a distributable call out
/// of the original program and into per-chunk task expressions. Verified by
/// a round-trip test over the forms that appear in real library code.
pub fn render(a: &crate::latte::Ast) -> String {
    use crate::latte::Ast::*;
    match a {
        Lit(n) => n.to_string(),
        Tag(s) => format!("%{}", s),
        Text(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")),
        Nil => "nil".to_string(),
        Var(s) => s.clone(),
        Tuple(xs) => format!("[ {} ]", xs.iter().map(render).collect::<Vec<_>>().join(" ")),
        Inc(e) => format!("+({})", render(e)),
        Head(e) => format!("(head {})", render(e)),
        Tail(e) => format!("(tail {})", render(e)),
        IsCell(e) => format!("(iscell {})", render(e)),
        Eq(x, y) => format!("({} == {})", render(x), render(y)),
        If(c, t, e) => format!("if {} then {} else {}", render(c), render(t), render(e)),
        Let(nm, v, b) => format!("let {} = {} in {}", nm, render(v), render(b)),
        Case(e, arms) => {
            let mut s = format!("case {} of ", render(e));
            for (i, (pat, r)) in arms.iter().enumerate() {
                if i > 0 {
                    s.push_str(" ; ");
                }
                match pat {
                    Some(t) => s.push_str(&format!("%{} -> {}", t, render(r))),
                    None => s.push_str(&format!("_ -> {}", render(r))),
                }
            }
            s.push_str(" end");
            s
        }
        Loop(binds, body) => {
            let bs = binds
                .iter()
                .map(|(n, v)| format!("{} = {}", n, render(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("loop with [{}] : {} end", bs, render(body))
        }
        Again(args) => format!("again({})", args.iter().map(render).collect::<Vec<_>>().join(", ")),
        Call(f, args) => {
            let mut s = format!("({}", f);
            for x in args {
                s.push(' ');
                s.push_str(&render(x));
            }
            s.push(')');
            s
        }
        Fast(nm, b) => format!("fast %{} {}", nm, render(b)),
        Gate(ps, b) => format!("(fn [{}] -> {})", ps.join(" "), render(b)),
    }
}

// ------------------------------- worker (server) -----------------------------

fn task_knot(expr: &str, libs: &[&str], fuel: u64) -> N {
    let libs_list = from_vec(&libs.iter().map(|l| cord(l)).collect::<Vec<_>>());
    cell(cord(expr), cell(num(fuel as u128), libs_list))
}

fn parse_task(k: &N) -> Option<(String, Vec<String>, u64)> {
    let (ek, rest) = k.as_cell()?;
    let (fk, lk) = rest.as_cell()?;
    let expr = ek.as_atom()?.as_cord()?;
    let fuel = fk.as_atom()?.to_u128()? as u64;
    let libs = to_vec(lk)
        .iter()
        .filter_map(|c| c.as_atom().and_then(|a| a.as_cord()))
        .collect();
    Some((expr, libs, fuel))
}

/// Serve evaluation tasks on an already-bound listener (tests bind :0 and
/// pass the listener in; the CLI binds the requested address).
pub fn serve_on(listener: TcpListener, verbose: bool) {
    if verbose {
        if let Ok(a) = listener.local_addr() {
            eprintln!("[worker] serving evaluation tasks on {}", a);
        }
    }
    for stream in listener.incoming() {
        if let Ok(s) = stream {
            std::thread::spawn(move || {
                let _ = worker_conn(s, verbose);
            });
        }
    }
}

/// `latte worker --listen ADDR` — turn this instance into a worker.
pub fn serve(listen: &str, verbose: bool) -> io::Result<()> {
    let l = TcpListener::bind(listen)?;
    serve_on(l, verbose);
    Ok(())
}

fn worker_conn(stream: TcpStream, verbose: bool) -> io::Result<()> {
    stream.set_nodelay(true).ok();
    let mut reader = stream.try_clone()?;
    let mut writer = stream;
    loop {
        let payload = read_frame(&mut reader)?;
        if payload.is_empty() || payload[0] != T_TASK {
            continue;
        }
        let reply = match Knot::cue(&payload[1..]).and_then(|(k, _)| parse_task(&k)) {
            Some((expr, libs, fuel)) => {
                if verbose {
                    let head: String = expr.chars().take(60).collect();
                    eprintln!("[worker] task: {}{}", head, if expr.len() > 60 { "…" } else { "" });
                }
                let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
                match crate::latte::run_with_libs_fuel(&expr, &refs, fuel) {
                    Ok(v) => cell(num(0), v),
                    Err(e) => cell(num(1), cord(&e)),
                }
            }
            None => cell(num(1), cord("malformed task")),
        };
        let mut frame = vec![T_RESULT];
        frame.extend_from_slice(&reply.jam());
        write_frame(&mut writer, &frame)?;
    }
}

// ------------------------------- client --------------------------------------

/// Evaluate one expression on a remote worker.
pub fn eval_remote(addr: &str, expr: &str, libs: &[&str], fuel: u64) -> Result<N, String> {
    let stream = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| format!("bad worker address {}: {}", addr, e))?,
        Duration::from_millis(2000),
    )
    .map_err(|e| format!("connect {}: {}", addr, e))?;
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(300))).ok();
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = stream;
    let mut frame = vec![T_TASK];
    frame.extend_from_slice(&task_knot(expr, libs, fuel).jam());
    write_frame(&mut writer, &frame).map_err(|e| format!("send {}: {}", addr, e))?;
    let payload = read_frame(&mut reader).map_err(|e| format!("recv {}: {}", addr, e))?;
    if payload.is_empty() || payload[0] != T_RESULT {
        return Err(format!("{}: malformed reply", addr));
    }
    let (k, _) = Knot::cue(&payload[1..]).ok_or_else(|| format!("{}: undecodable reply", addr))?;
    let (tagk, val) = k.as_cell().ok_or_else(|| format!("{}: malformed result", addr))?;
    match tagk.as_atom().and_then(|a| a.to_u128()) {
        Some(0) => Ok(val.clone()),
        _ => Err(val
            .as_atom()
            .and_then(|a| a.as_cord())
            .unwrap_or_else(|| "remote error".into())),
    }
}

/// Run one task per entry, in parallel, round-robin across `workers`. A task
/// whose worker fails (unreachable, crashed, malformed) is re-run LOCALLY, so
/// the answer set is always complete and always correct. Returns the results
/// in task order plus the number of tasks that fell back to local execution.
pub fn run_tasks(workers: &[String], tasks: &[String], libs: &[&str], fuel: u64) -> (Vec<Result<N, String>>, usize) {
    let libs_owned: Vec<String> = libs.iter().map(|s| s.to_string()).collect();
    let handles: Vec<_> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let expr = t.clone();
            let worker = if workers.is_empty() { None } else { Some(workers[i % workers.len()].clone()) };
            let libs = libs_owned.clone();
            std::thread::spawn(move || {
                let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
                if let Some(w) = worker {
                    match eval_remote(&w, &expr, &refs, fuel) {
                        Ok(v) => return (Ok(v), false),
                        Err(_) => {} // fall through to local
                    }
                }
                (crate::latte::run_with_libs_fuel(&expr, &refs, fuel), true)
            })
        })
        .collect();
    let mut out = Vec::with_capacity(tasks.len());
    let mut local = 0usize;
    for h in handles {
        match h.join() {
            Ok((r, fell_back)) => {
                if fell_back {
                    local += 1;
                }
                out.push(r);
            }
            Err(_) => out.push(Err("task thread panicked".into())),
        }
    }
    (out, local)
}

// ------------------------------- worker registry -----------------------------

fn workers_path() -> std::path::PathBuf {
    crate::rustgen::cache_dir().join("workers")
}

/// The connected workers this instance may distribute to: the ORPHEUS_WORKERS
/// environment variable (comma-separated host:port), plus one address per
/// line of the persistent registry (`latte workers add`). Order preserved,
/// duplicates removed.
pub fn workers() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Ok(env) = std::env::var("ORPHEUS_WORKERS") {
        for a in env.split(',') {
            let a = a.trim();
            if !a.is_empty() && !out.iter().any(|x| x == a) {
                out.push(a.to_string());
            }
        }
    }
    if let Ok(s) = std::fs::read_to_string(workers_path()) {
        for line in s.lines() {
            let a = line.trim();
            if !a.is_empty() && !a.starts_with('#') && !out.iter().any(|x| x == a) {
                out.push(a.to_string());
            }
        }
    }
    out
}

pub fn workers_add(addr: &str) -> io::Result<()> {
    let mut cur = workers();
    if !cur.iter().any(|x| x == addr) {
        cur.push(addr.to_string());
    }
    let file_set: Vec<String> = {
        // keep only what belongs in the FILE (env entries stay in the env)
        let mut v: Vec<String> = std::fs::read_to_string(workers_path())
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !v.iter().any(|x| x == addr) {
            v.push(addr.to_string());
        }
        v
    };
    std::fs::create_dir_all(crate::rustgen::cache_dir())?;
    std::fs::write(workers_path(), file_set.join("\n") + "\n")
}

pub fn workers_remove(addr: &str) -> io::Result<()> {
    let v: Vec<String> = std::fs::read_to_string(workers_path())
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && l != addr)
        .collect();
    std::fs::create_dir_all(crate::rustgen::cache_dir())?;
    std::fs::write(workers_path(), if v.is_empty() { String::new() } else { v.join("\n") + "\n" })
}

pub fn workers_clear() -> io::Result<()> {
    match std::fs::remove_file(workers_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Is a worker answering? (used by `latte workers` to show liveness)
pub fn worker_alive(addr: &str) -> bool {
    addr.parse()
        .ok()
        .and_then(|a| TcpStream::connect_timeout(&a, Duration::from_millis(400)).ok())
        .is_some()
}

// --------------------------- distributable shapes ----------------------------

/// The data-parallel decompositions the profiler can detect. Each carries the
/// pieces needed to rebuild per-chunk task expressions.
pub enum Shape {
    /// `(dmap f xs)` / `(map f xs)` — explicit is true for dmap (the
    /// distributable map distributes whenever workers exist; that is its
    /// documented meaning).
    Map { f_src: String, xs: crate::latte::Ast, explicit: bool },
    /// `(predict_all w b xs)` — ML batch prediction (lib/ml.lat): the model
    /// is broadcast, the inputs are sharded.
    Predict { w: crate::latte::Ast, b: crate::latte::Ast, xs: crate::latte::Ast },
}

/// Parse an expression and detect a distributable top-level shape.
pub fn detect(expr: &str) -> Option<Shape> {
    let ast = crate::latte::parse(expr).ok()?;
    if let crate::latte::Ast::Call(name, args) = ast {
        match (name.as_str(), args.len()) {
            ("dmap", 2) | ("map", 2) => {
                let explicit = name == "dmap";
                let f_src = render(&args[0]);
                // the rendered function must round-trip (it is re-parsed on
                // the worker); if it does not, stay local
                if crate::latte::parse(&f_src).is_err() {
                    return None;
                }
                let mut it = args.into_iter();
                let _f = it.next()?;
                let xs = it.next()?;
                Some(Shape::Map { f_src, xs, explicit })
            }
            ("predict_all", 3) => {
                let mut it = args.into_iter();
                Some(Shape::Predict { w: it.next()?, b: it.next()?, xs: it.next()? })
            }
            _ => None,
        }
    } else {
        None
    }
}

/// A one-line human description of a detected shape, for the profiler report.
pub fn detect_kind(expr: &str) -> Option<&'static str> {
    match detect(expr)? {
        Shape::Map { explicit: true, .. } => Some("data-parallel map (dmap — distributes whenever workers are connected)"),
        Shape::Map { explicit: false, .. } => Some("data-parallel map"),
        Shape::Predict { .. } => Some("ML batch prediction (model broadcast, inputs sharded)"),
    }
}

/// The measured-time floor below which distributing could not pay for its
/// network round-trips (ORPHEUS_DIST_NS, default 25 ms).
pub fn dist_threshold_ns() -> u64 {
    std::env::var("ORPHEUS_DIST_NS").ok().and_then(|v| v.parse().ok()).unwrap_or(25_000_000)
}

/// The DEFAULT distribution decision, consulted by the adaptive engine on
/// every evaluation (src/rustgen.rs `run_adaptive` and the CLI eval path):
/// distribute when (a) workers are connected, (b) the expression has a
/// distributable shape, and (c) the shape is explicit (`dmap`) or the
/// profiler has MEASURED the program at or above the distribution threshold —
/// the same measurement-beats-guesswork policy the compile decision uses.
/// Returns None to mean "stay local".
pub fn maybe_distribute(expr: &str, libs: &[&str]) -> Option<Result<N, String>> {
    if std::env::var("ORPHEUS_DIST").ok().as_deref() == Some("0") {
        return None;
    }
    // cheap prefilter before parsing: all distributable shapes start with one
    // of three call heads
    let t = expr.trim_start();
    if !(t.starts_with("(dmap ") || t.starts_with("(map ") || t.starts_with("(predict_all ")) {
        return None;
    }
    let ws = workers();
    if ws.is_empty() {
        return None;
    }
    let shape = detect(expr)?;
    let explicit = matches!(shape, Shape::Map { explicit: true, .. });
    if !explicit {
        let heavy = crate::rustgen::profile_lookup(expr, libs)
            .map(|p| p.interp_ns >= dist_threshold_ns())
            .unwrap_or(false);
        if !heavy {
            return None;
        }
    }
    let audit = std::env::var("ORPHEUS_DIST_AUDIT").ok().as_deref() == Some("1");
    Some(distribute(&ws, &shape, expr, libs, audit))
}

/// Execute a detected shape across `workers`: evaluate the data locally,
/// split it into contiguous chunks (order-preserving), run one task per chunk
/// remotely (local fallback per chunk), and concatenate. With `audit` set the
/// whole expression is ALSO evaluated locally and compared — the distributed
/// engine is held to the interpreter's answer, exactly like a jet.
pub fn distribute(workers: &[String], shape: &Shape, expr: &str, libs: &[&str], audit: bool) -> Result<N, String> {
    let out = match shape {
        Shape::Map { f_src, xs, .. } => {
            let xs_val = crate::latte::run_with_libs(&render(xs), libs)?;
            if noun_literal(&xs_val).is_none() {
                return crate::latte::run_with_libs(expr, libs); // oversize atom: stay local
            }
            let items = to_vec(&xs_val);
            distribute_map(workers, f_src, &items, libs)?
        }
        Shape::Predict { w, b, xs } => {
            let wv = crate::latte::run_with_libs(&render(w), libs)?;
            let bv = crate::latte::run_with_libs(&render(b), libs)?;
            let xs_val = crate::latte::run_with_libs(&render(xs), libs)?;
            let (wl, bl) = match (noun_literal(&wv), noun_literal(&bv)) {
                (Some(a), Some(c)) if noun_literal(&xs_val).is_some() => (a, c),
                _ => return crate::latte::run_with_libs(expr, libs), // oversize atom: stay local
            };
            let items = to_vec(&xs_val);
            let f_src = format!("(fn [x] -> (predict {} {} x))", wl, bl);
            distribute_map(workers, &f_src, &items, libs)?
        }
    };
    if audit {
        let local = crate::latte::run_with_libs(expr, libs)?;
        if local != out {
            return Err("distribution audit FAILED: remote result differs from the interpreter".into());
        }
    }
    Ok(out)
}

/// Chunk `items` contiguously across the workers and map `f_src` over each
/// chunk remotely; concatenate in order.
fn distribute_map(workers: &[String], f_src: &str, items: &[N], libs: &[&str]) -> Result<N, String> {
    if items.is_empty() {
        return Ok(num(0));
    }
    let parts = workers.len().max(1).min(items.len());
    let per = (items.len() + parts - 1) / parts;
    let mut tasks = Vec::new();
    for chunk in items.chunks(per) {
        // the caller has verified the full list renders; every chunk of a
        // renderable noun is renderable
        let lit = noun_literal(&from_vec(chunk)).ok_or("unrenderable chunk")?;
        tasks.push(format!("(map {} {})", f_src, lit));
    }
    let (results, local) = run_tasks(workers, &tasks, libs, TASK_FUEL);
    let mut all = Vec::with_capacity(items.len());
    for r in results {
        all.extend(to_vec(&r?));
    }
    eprintln!(
        "dist: map over {} elements → {} chunk(s) across {} worker(s){}",
        items.len(),
        tasks.len(),
        workers.len(),
        if local > 0 { format!(" ({} ran locally after worker failure)", local) } else { String::new() }
    );
    Ok(from_vec(&all))
}

/// The profiler's distribution paragraph for `latte profile` — states the
/// detected shape and the decision the adaptive engine will now take.
pub fn profile_note(expr: &str, libs: &[&str], interp_ns: u64) -> Option<String> {
    let kind = detect_kind(expr)?;
    let ws = workers();
    let explicit = matches!(detect(expr), Some(Shape::Map { explicit: true, .. }));
    let thr = dist_threshold_ns();
    let heavy = interp_ns >= thr;
    let _ = libs;
    let decision = if explicit && !ws.is_empty() {
        format!("distribute by default across {} connected worker(s) (dmap is the distributable map)", ws.len())
    } else if explicit {
        "distribute by default as soon as workers are connected (latte worker / latte workers add)".into()
    } else if heavy && !ws.is_empty() {
        format!(
            "distribute by default — measured {:.3} ms ≥ {:.1} ms distribution threshold, {} worker(s) connected",
            interp_ns as f64 / 1e6,
            thr as f64 / 1e6,
            ws.len()
        )
    } else if heavy {
        format!(
            "would distribute by default once workers are connected (measured {:.3} ms ≥ {:.1} ms threshold)",
            interp_ns as f64 / 1e6,
            thr as f64 / 1e6
        )
    } else {
        format!(
            "stay local — measured {:.3} ms < {:.1} ms distribution threshold (network overhead would not pay)",
            interp_ns as f64 / 1e6,
            thr as f64 / 1e6
        )
    };
    Some(format!("  distributable: {}\n  dist decision: {}", kind, decision))
}

// --------------------------- distributed training ----------------------------

/// One round of distributed training + the consolidated model's full-data MSE.
pub struct Round {
    pub mse: String,
}

/// The result of a distributed FedAvg training run.
pub struct FedReport {
    pub w: N,
    pub b: N,
    pub rounds: Vec<Round>,
    pub shards: usize,
    pub local_fallbacks: usize,
    pub persisted: Option<String>,
}

fn spos(mag: u128) -> N {
    cell(num(0), num(mag))
}

fn fmt_signed(n: &N) -> String {
    if let Knot::Cell(s, m) = &**n {
        let sign = s.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = m.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let sgn = if sign == 1 && mag != 0 { "-" } else { "" };
        format!("{}{}.{:03}", sgn, mag / 1000, mag % 1000)
    } else {
        format!("{:?}", n)
    }
}

/// Train `y = w·x + b` by local SGD with periodic model averaging (FedAvg)
/// across the connected workers, then commit the FINAL consolidated model to
/// the persistent log (kv agent, key %model) when `store` is given.
///
/// The mechanics per round: each shard k gets the task
/// `(train W B XSk YSk LR E)` (lib/ml.lat — E local gradient steps from the
/// current consolidated model); the returned models are consolidated with
/// `(fedavg models sizes)` (lib/dist.lat) — every consolidation is computed
/// in Latte on Loom, not in Rust. Workers that fail fall back to local
/// execution of their shard, so a degraded cluster still converges.
pub fn fedavg_linear(
    workers: &[String],
    xs: &[N],
    ys: &[N],
    rounds: u64,
    local_iters: u64,
    lr: &N,
    store: Option<&str>,
    audit: bool,
) -> Result<FedReport, String> {
    let libs_owned = crate::latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    if xs.len() != ys.len() || xs.is_empty() {
        return Err("training data is empty or mismatched".into());
    }
    // Shard round-robin so heterogeneous data spreads evenly (the same split
    // lib/dist.lat's `shard` computes; verified equal by the test suite).
    let shards = workers.len().max(2).min(xs.len());
    let mut sx: Vec<Vec<N>> = vec![Vec::new(); shards];
    let mut sy: Vec<Vec<N>> = vec![Vec::new(); shards];
    for (i, (x, y)) in xs.iter().zip(ys.iter()).enumerate() {
        sx[i % shards].push(x.clone());
        sy[i % shards].push(y.clone());
    }
    let sizes: Vec<N> = sx.iter().map(|s| num(s.len() as u128)).collect();
    let sizes_lit = noun_literal(&from_vec(&sizes)).ok_or("unrenderable sizes")?;
    let xs_full = noun_literal(&from_vec(xs)).ok_or("unrenderable data")?;
    let ys_full = noun_literal(&from_vec(ys)).ok_or("unrenderable data")?;
    let lr_lit = noun_literal(lr).ok_or("unrenderable learning rate")?;

    let mut w = spos(0);
    let mut b = spos(0);
    let mut report_rounds = Vec::new();
    let mut fallbacks = 0usize;
    for _round in 0..rounds {
        let (wl, bl) = (
            noun_literal(&w).ok_or("unrenderable model")?,
            noun_literal(&b).ok_or("unrenderable model")?,
        );
        // one task per shard: E local gradient steps from the consolidated model
        let tasks: Vec<String> = sx
            .iter()
            .zip(sy.iter())
            .map(|(cx, cy)| {
                Ok(format!(
                    "(train {} {} {} {} {} {})",
                    wl,
                    bl,
                    noun_literal(&from_vec(cx)).ok_or("unrenderable shard")?,
                    noun_literal(&from_vec(cy)).ok_or("unrenderable shard")?,
                    lr_lit,
                    local_iters
                ))
            })
            .collect::<Result<_, String>>()?;
        let (results, local) = run_tasks(workers, &tasks, &libs, TASK_FUEL);
        fallbacks += local;
        let models: Vec<N> = results.into_iter().collect::<Result<_, _>>()?;
        // consolidation — FedAvg, computed in Latte on Loom
        let models_lit = noun_literal(&from_vec(&models)).ok_or("unrenderable models")?;
        let merged = crate::latte::run_with_libs(&format!("(fedavg {} {})", models_lit, sizes_lit), &libs)?;
        let (mw, mb) = merged.as_cell().ok_or("fedavg returned a non-cell")?;
        w = mw.clone();
        b = mb.clone();
        if audit {
            // the consolidated model must equal the weighted mean the
            // interpreter computes from the same parts — recomputed here in
            // Rust-side Latte again with shards permuted, order must not matter
            let mut perm_models = models.clone();
            perm_models.reverse();
            let mut perm_sizes = sizes.clone();
            perm_sizes.reverse();
            let m2 = crate::latte::run_with_libs(
                &format!(
                    "(fedavg {} {})",
                    noun_literal(&from_vec(&perm_models)).ok_or("unrenderable")?,
                    noun_literal(&from_vec(&perm_sizes)).ok_or("unrenderable")?
                ),
                &libs,
            )?;
            if m2 != merged {
                return Err("consolidation audit FAILED: fedavg is order-sensitive".into());
            }
        }
        // full-data MSE of the consolidated model — the cycle-over-cycle
        // improvement the report shows
        let msev = crate::latte::run_with_libs(
            &format!(
                "(mse {} {} {} {})",
                noun_literal(&w).ok_or("unrenderable")?,
                noun_literal(&b).ok_or("unrenderable")?,
                xs_full,
                ys_full
            ),
            &libs,
        )?;
        report_rounds.push(Round { mse: fmt_signed(&msev) });
    }

    // Only now — after the cycles of distributed execution and consolidation —
    // does the persistent state change: ONE event carrying the final model.
    let mut persisted = None;
    if let Some(dir) = store {
        let agent = crate::agent::Agent::new_kv()?;
        let mut node = crate::net::Node::open(1, agent, dir, 0).map_err(|e| e.to_string())?;
        let action = cell(cord("put"), cell(cord("model"), cell(w.clone(), b.clone())));
        node.local_action(action);
        node.snapshot().map_err(|e| format!("{:?}", e))?;
        persisted = Some(dir.to_string());
    }

    Ok(FedReport { w, b, rounds: report_rounds, shards, local_fallbacks: fallbacks, persisted })
}

/// The demo dataset for distributed linear training: y = 2x + 1 over
/// x = 1..=12, in signed fixed point (×1000). Larger than fit_demo's four
/// points so sharding has something to divide.
pub fn demo_data() -> (Vec<N>, Vec<N>) {
    let xs: Vec<N> = (1..=12u128).map(|x| spos(x * 1000)).collect();
    let ys: Vec<N> = (1..=12u128).map(|x| spos(2 * x * 1000 + 1000)).collect();
    (xs, ys)
}

/// `latte ml linear`, distributed by default when workers are connected:
/// print the FedAvg training report. Used by src/numerics.rs.
pub fn cmd_ml_fedavg(workers: &[String], rounds: u64, local_iters: u64, store: Option<&str>) {
    println!("ml — DISTRIBUTED linear regression: local SGD + FedAvg consolidation (lib/ml.lat + lib/dist.lat)\n");
    println!("  task: fit  y = w·x + b  to 12 points of y = 2x + 1  [true w=2, b=1]");
    if workers.is_empty() {
        println!("  workers: none connected — shards run locally (add with `latte worker` + `latte workers add`)");
    } else {
        println!("  workers: {}", workers.join(", "));
    }
    println!(
        "  {} round(s) × {} local gradient steps per shard, then FedAvg consolidation in Latte\n",
        rounds, local_iters
    );
    let (xs, ys) = demo_data();
    // learning rate 0.025: gradient descent on x = 1..12 is stable only below
    // 2 / mean(x²) ≈ 0.037 — the four-point fit_demo's 0.2 would diverge here
    match fedavg_linear(workers, &xs, &ys, rounds, local_iters, &spos(25), store, false) {
        Ok(rep) => {
            for (i, r) in rep.rounds.iter().enumerate() {
                println!("  round {}: consolidated model MSE = {}", i + 1, r.mse);
            }
            println!("\n  learned w = {}", fmt_signed(&rep.w));
            println!("  learned b = {}", fmt_signed(&rep.b));
            println!("  shards: {}  local fallbacks: {}", rep.shards, rep.local_fallbacks);
            match rep.persisted {
                Some(dir) => println!(
                    "  persistent state updated: final consolidated model committed as ONE event (kv %model) in {}",
                    dir
                ),
                None => println!("  (pass --store DIR to commit the final model to a persistent, gossiped log)"),
            }
        }
        Err(e) => println!("  error: {}", e),
    }
}

// ------------------------------- tests ----------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_worker() -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || serve_on(l, false));
        addr
    }

    fn libs() -> Vec<String> {
        crate::latte::all_libs()
    }

    #[test]
    fn noun_literal_round_trips_through_eval() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        let v = cell(num(7), cell(cell(num(1), num(2000)), num(0)));
        let lit = noun_literal(&v).unwrap();
        let back = crate::latte::run_with_libs(&lit, &refs).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn render_round_trips_representative_forms() {
        // every rendered form must re-parse to the same AST — the property
        // task construction depends on
        for src in [
            "(fn [x] -> (mul x x))",
            "(fn [x y] -> (nadd (nmul x y) [ 0 1000 ]))",
            "(fn [x] -> if (x == 0) then 1 else +(x))",
            "(fn [x] -> let d = (mul x 2) in (add d 1))",
            "(fn [xs] -> (map (fn [e] -> +(e)) xs))",
            "(fn [x] -> (head (tail x)))",
        ] {
            let a1 = crate::latte::parse(src).unwrap();
            let r = render(&a1);
            let a2 = crate::latte::parse(&r).unwrap_or_else(|e| panic!("render of {} does not re-parse: {} — {}", src, r, e));
            assert_eq!(format!("{:?}", a1), format!("{:?}", a2), "render changed the AST of {}", src);
        }
    }

    #[test]
    fn shard_unshard_and_fedavg_agree_with_hand_values() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        // shard/unshard are inverses
        let v = crate::latte::run_with_libs(
            "(unshard (shard 3 [ 1 [ 2 [ 3 [ 4 [ 5 [ 6 [ 7 0 ] ] ] ] ] ] ]))",
            &refs,
        )
        .unwrap();
        let want = crate::latte::run_with_libs("[ 1 [ 2 [ 3 [ 4 [ 5 [ 6 [ 7 0 ] ] ] ] ] ] ]", &refs).unwrap();
        assert_eq!(v, want, "unshard . shard = identity");
        // fedavg: models 1.0 and 4.0 with weights 1 and 2 → (1 + 8)/3 = 3.0
        let m = crate::latte::run_with_libs(
            "(fedavg [ [ [ 0 1000 ] [ 0 1000 ] ] [ [ [ 0 4000 ] [ 0 4000 ] ] 0 ] ] [ 1 [ 2 0 ] ])",
            &refs,
        )
        .unwrap();
        let (w, b) = m.as_cell().unwrap();
        assert_eq!(super::fmt_signed(w), "3.000");
        assert_eq!(super::fmt_signed(b), "3.000");
    }

    #[test]
    fn worker_answers_tasks_identically_to_local_eval() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        let w = spawn_worker();
        for expr in ["(mul 6 7)", "(map (fn [x] -> (mul x x)) [ 1 [ 2 [ 3 0 ] ] ])"] {
            let remote = eval_remote(&w, expr, &refs, TASK_FUEL).unwrap();
            let local = crate::latte::run_with_libs(expr, &refs).unwrap();
            assert_eq!(remote, local, "worker disagreed with the interpreter on {}", expr);
        }
        // an erroring task comes back as an error, not a hang or a bogus value
        assert!(eval_remote(&w, "(no_such_arm 1)", &refs, TASK_FUEL).is_err());
    }

    #[test]
    fn dmap_distributes_across_workers_and_matches_pure_map() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        let ws = vec![spawn_worker(), spawn_worker()];
        let expr = "(dmap (fn [x] -> (mul x x)) [ 1 [ 2 [ 3 [ 4 [ 5 0 ] ] ] ] ])";
        let shape = detect(expr).expect("dmap detected");
        assert!(matches!(shape, Shape::Map { explicit: true, .. }));
        // audit=true: the distributed answer is HELD to the interpreter's
        let v = distribute(&ws, &shape, expr, &refs, true).unwrap();
        let pure = crate::latte::run_with_libs("(map (fn [x] -> (mul x x)) [ 1 [ 2 [ 3 [ 4 [ 5 0 ] ] ] ] ])", &refs).unwrap();
        assert_eq!(v, pure, "dmap result must equal pure map");
    }

    #[test]
    fn predict_all_distributes_model_broadcast() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        let ws = vec![spawn_worker()];
        let expr = "(predict_all [ 0 2000 ] [ 0 1000 ] [ [ 0 1000 ] [ [ 0 2000 ] [ [ 0 3000 ] 0 ] ] ])";
        let shape = detect(expr).expect("predict_all detected");
        let v = distribute(&ws, &shape, expr, &refs, true).unwrap();
        // y = 2x + 1 over x = 1,2,3 → 3,5,7
        let got: Vec<String> = to_vec(&v).iter().map(super::fmt_signed).collect();
        assert_eq!(got, vec!["3.000", "5.000", "7.000"]);
    }

    #[test]
    fn worker_failure_falls_back_to_local_execution() {
        let refs_owned = libs();
        let refs: Vec<&str> = refs_owned.iter().map(|s| s.as_str()).collect();
        // one live worker, one dead address: every chunk still gets a correct answer
        let ws = vec![spawn_worker(), "127.0.0.1:1".to_string()];
        let tasks = vec![
            "(mul 2 3)".to_string(),
            "(mul 4 5)".to_string(),
            "(mul 6 7)".to_string(),
        ];
        let (results, local) = run_tasks(&ws, &tasks, &refs, TASK_FUEL);
        let got: Vec<u128> = results
            .into_iter()
            .map(|r| r.unwrap().as_atom().unwrap().to_u128().unwrap())
            .collect();
        assert_eq!(got, vec![6, 20, 42]);
        assert!(local >= 1, "the dead worker's task must have fallen back locally");
    }

    #[test]
    fn distributed_fedavg_training_converges_and_updates_persistent_state() {
        let ws = vec![spawn_worker(), spawn_worker()];
        let (xs, ys) = demo_data();
        let dir = std::env::temp_dir().join(format!("orpheus-dist-fed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dirs = dir.to_string_lossy().to_string();
        let rep = fedavg_linear(&ws, &xs, &ys, 4, 200, &spos(25), Some(&dirs), true).unwrap();
        // cycles improve the consolidated model: MSE strictly decreases from
        // the first consolidation to the last
        let first: f64 = rep.rounds.first().unwrap().mse.parse().unwrap();
        let last: f64 = rep.rounds.last().unwrap().mse.parse().unwrap();
        assert!(
            last < first,
            "consolidation cycles must improve the model: first MSE {} vs last {}",
            first,
            last
        );
        // the learned model is close to the truth (w = 2, b = 1)
        let wmag = rep.w.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap();
        let bmag = rep.b.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap();
        assert!((1800..=2200).contains(&wmag), "w ≈ 2.0, got {}", wmag as f64 / 1000.0);
        assert!(bmag <= 1600, "b heads toward 1.0, got {}", bmag as f64 / 1000.0);
        // the final consolidated model — and ONLY it — became persistent state
        assert_eq!(rep.persisted.as_deref(), Some(dirs.as_str()));
        let node = crate::net::Node::open(1, crate::agent::Agent::new_kv().unwrap(), &dirs, 0).unwrap();
        assert_eq!(node.event_count(), 1, "exactly one event: the final consolidated model");
        let st = node.state().unwrap();
        // state is an assoc list [[%model [w b]] 0]
        let (pair, _) = st.as_cell().expect("kv state holds the model");
        let (key, model) = pair.as_cell().unwrap();
        assert_eq!(key.as_atom().unwrap().as_cord().as_deref(), Some("model"));
        assert_eq!(model, &cell(rep.w.clone(), rep.b.clone()), "persisted model is the final consolidated one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profiler_detects_distributable_shapes() {
        assert!(detect_kind("(dmap (fn [x] -> +(x)) [ 1 [ 2 0 ] ])").is_some());
        assert!(detect_kind("(map (fn [x] -> +(x)) xs)").is_some());
        assert!(detect_kind("(predict_all w b xs)").is_some());
        assert!(detect_kind("(add 1 2)").is_none());
        assert!(detect_kind("head [1 2]").is_none());
        // the note names the default behavior
        let note = profile_note("(dmap (fn [x] -> +(x)) [ 1 [ 2 0 ] ])", &["std"], 0).unwrap();
        assert!(note.contains("distributable"), "{}", note);
        assert!(note.contains("default"), "{}", note);
    }
}

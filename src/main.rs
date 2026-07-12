//! Latte on Mocha — command-line entry point.
//!
//!   latte node --listen ADDR [--peer ADDR]... [--do CMD]... [--run-secs N] [--name S] [--id N] [-v]
//!   latte eval "<latte expression>"          compile + run an expression against subject 0
//!   latte agent                                 print the compiled agent's content address
//!   latte selftest                              in-process convergence check (no sockets)
//!
//! A "node" maintains shared state with its peers over TCP. Run one on each
//! computer, point --peer at the others (public IP:port across the Internet, or
//! 127.0.0.1:PORT for a local multi-process test), and every node converges on
//! identical state — the same code, the same content hash, everywhere.

mod atom;
mod knot;
mod sha3;
mod secure;
mod loom;
mod fmt;
mod latte;
mod agent;
mod net;
mod dist;
mod ledger;
mod notes;
mod store;
mod sca;
mod facet;
mod serve;
mod site;
mod docs_embed;
mod cliutil;
mod mold;
mod check;
mod mocha;
mod plan;
mod jets;
mod numerics;
mod ddia;
mod dates;
mod marketdata;
mod viz;
mod gfx;
mod gpu;
mod conlang;
mod sentiment;
mod events;
mod newswire;
mod game;
mod dbservice;
mod dbsync;
mod httpd;
mod rustgen;
mod anvild;
mod registry;
mod fuzz;
mod icomb;
mod repl;

use agent::Agent;
use net::{Config, Node};
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};


fn cmd_net(args: &[String]) {
    let mut peano = false;
    let mut par: Option<usize> = None;
    let mut rest: Vec<String> = Vec::new();
    let mut skip = false;
    for (i, a) in args.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if a == "--peano" || a == "--unary" {
            peano = true;
        } else if a == "--par" || a == "--threads" {
            // default thread count = the machine's parallelism
            let auto = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
            par = args.get(i + 1).and_then(|v| v.parse().ok()).or(Some(auto));
            skip = args.get(i + 1).map(|v| v.parse::<usize>().is_ok()).unwrap_or(false);
        } else {
            rest.push(a.clone());
        }
    }
    let src = rest.join(" ");
    if src.is_empty() {
        eprintln!("usage: latte net [--peano] \"<expr>\"   compile the numeric fragment to an interaction net and reduce it");
        eprintln!("       default: native number agents (one interaction per arithmetic op, the HVM2 idea)");
        eprintln!("       --peano: unary Peano chains and lockstep agents (the pedagogical mode)");
        return;
    }
    if par.is_some() {
        eprintln!("note: the DEFAULT engine is the best-measured policy on every machine we have");
        eprintln!("profiled — sequential reduction over native number agents. --par opts into the");
        eprintln!("batch-claimed parallel reducer: verified equivalent by uniform confluence, but");
        eprintln!("a correctness demonstration, not an optimized runtime (HVM2-class throughput");
        eprintln!("needs lock-free linking; see docs/interaction-nets.md). This machine: {} CPU(s).",
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    }
    let res = if let Some(t) = par {
        icomb::run_str_parallel(&src, t)
    } else if peano {
        icomb::run_str_peano(&src)
    } else {
        icomb::run_str(&src)
    };
    match res {
        Ok((v, steps)) => {
            let libs = latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            let oracle = latte::run_with_libs(&src, &refs)
                .ok()
                .and_then(|n| n.as_atom().and_then(|a| a.to_u128()));
            match oracle {
                Some(o) => println!(
                    "{} → {}  ({} interaction steps; Loom interpreter: {} — {})",
                    src,
                    v,
                    steps,
                    o,
                    if o == v { "match" } else { "MISMATCH" }
                ),
                None => println!("{} → {}  ({} interaction steps)", src, v, steps),
            }
        }
        Err(e) => eprintln!(
            "net: {} — the interaction-net compiler handles +, *, <, and if over naturals",
            e
        ),
    }
}

fn cmd_cache(args: &[String]) {
    let dir = rustgen::cache_dir();
    match args.get(0).map(|s| s.as_str()) {
        Some("clear") | Some("clean") => {
            let n = rustgen::cache_clear();
            println!("cleared {} compiled program(s) from {}", n, dir.display());
        }
        Some("metrics") | Some("stats-detail") => {
            let m = rustgen::metrics_snapshot();
            let saved = m.est_saved_ms();
            println!("builds:         {} ({} ms total, {} ms avg)", m.builds, m.build_ms_total, m.avg_build_ms());
            println!("cache hits:     {}", m.hits);
            println!("remote pulls:   {}", m.pulls);
            println!("build failures: {} (see `latte cache log`)", m.build_failures);
            let total = m.builds + m.hits + m.pulls;
            let rate = if total > 0 { 100 * (m.hits + m.pulls) / total } else { 0 };
            println!("reuse rate:     {}% of {} native runs avoided a build", rate, total);
            println!("est. rustc time saved: {:.1} s", saved as f64 / 1000.0);
        }
        Some("log") => {
            let tail = rustgen::build_log_tail(16 * 1024);
            if tail.trim().is_empty() {
                println!("no native build failures logged");
            } else {
                print!("{}", tail);
            }
        }
        Some("verify") => {
            let repair = args.iter().any(|a| a == "--repair" || a == "--fix");
            let (ok, corrupt, no_sc) = rustgen::cache_verify(repair);
            println!(
                "verified {} ok, {} corrupt{}, {} without sidecar",
                ok,
                corrupt,
                if repair && corrupt > 0 { " (purged)" } else if corrupt > 0 { " (run with --repair to purge)" } else { "" },
                no_sc
            );
        }
        Some("warm") => {
            let expr = args[1..].join(" ");
            if expr.is_empty() {
                eprintln!("usage: latte cache warm \"<expr>\"   (precompile so a later run is instant)");
                return;
            }
            let libs_owned = latte::all_libs();
            let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
            let t0 = std::time::Instant::now();
            match rustgen::warm_native(&expr, &libs) {
                Ok(true) => println!("warmed in {:.1}s; subsequent runs use the cached native binary", t0.elapsed().as_secs_f64()),
                Ok(false) => println!("already warm"),
                Err(e) => eprintln!("warm: {}", e),
            }
        }
        Some("path") | Some("status") | None => {
            let (n, bytes) = rustgen::cache_stats();
            let (val, unit) = if bytes >= 1 << 20 {
                (bytes as f64 / (1u64 << 20) as f64, "MiB")
            } else {
                (bytes as f64 / 1024.0, "KiB")
            };
            println!("{}", dir.display());
            let cap = rustgen::cache_cap_bytes();
            let cap_str = if cap == 0 {
                "unbounded".to_string()
            } else {
                format!("{} MiB cap, LRU eviction", cap >> 20)
            };
            println!(
                "{} cached program(s), {:.1} {} on disk (opt-level {}, {}); `latte cache warm \"<expr>\"` to prebuild, `latte cache clear` to empty",
                n, val, unit, std::env::var("ORPHEUS_OPT").unwrap_or_else(|_| "0".to_string()), cap_str
            );
            match rustgen::shared_store_dir() {
                Some(d) => println!("shared store: {} (toolchain {})", d.display(), rustgen::toolchain_id()),
                None => println!("shared store: off (set ORPHEUS_CACHE_SHARED to a dir to share builds across hosts)"),
            }
            let m = rustgen::metrics_snapshot();
            println!(
                "metrics: {} builds, {} hits, {} pulls, ~{:.1}s rustc saved (`latte cache metrics` for detail)",
                m.builds, m.hits, m.pulls, m.est_saved_ms() as f64 / 1000.0
            );
        }
        Some(other) => eprintln!("cache: unknown subcommand '{}' (try: status | metrics | verify [--repair] | log | warm \"<expr>\" | clear)", other),
    }
}

fn cmd_anvil(args: &[String]) {
    match args.get(0).map(|s| s.as_str()) {
        Some("serve") | Some("start") => {
            // Foreground here; callers daemonize via `setsid … &`. Reuses the same Anvil cache as
            // the CLI, so anything the daemon builds is immediately runnable by other processes.
            if anvild::ping() {
                eprintln!("anvild already running");
                return;
            }
            eprintln!("anvild serving on the Anvil cache socket (Ctrl-C or `latte anvil stop` to quit)");
            if let Err(e) = anvild::serve() {
                eprintln!("anvild: {}", e);
            }
        }
        Some("registry") => {
            match args.get(1).map(|s| s.as_str()) {
                Some("serve") => {
                    let addr = args.get(2).map(|s| s.as_str()).unwrap_or("127.0.0.1:8099");
                    let root = args.get(3).map(|s| s.as_str()).unwrap_or("/tmp/anvil-registry");
                    if registry::registry_key().is_none() {
                        eprintln!("warning: ORPHEUS_REGISTRY_KEY not set — uploads will be rejected (reads only)");
                    }
                    if let Err(e) = registry::serve(addr, root) {
                        eprintln!("registry: {}", e);
                    }
                }
                _ => eprintln!("usage: latte anvil registry serve [addr] [root]   (set ORPHEUS_REGISTRY_KEY to sign)"),
            }
        }
        Some("shrink") => {
            let expr = args[1..].join(" ");
            if expr.is_empty() {
                eprintln!("usage: latte anvil shrink \"<expr>\"   (minimize to the smallest non-native subterm)");
                return;
            }
            if !fuzz::not_fully_native(&expr) {
                println!("nothing to shrink: this program is fully native (matches the interpreter)");
                return;
            }
            let mut pred = |c: &str| fuzz::not_fully_native(c);
            let min = fuzz::shrink_with(&expr, &mut pred, 200);
            println!("minimized to {} chars (from {}):\n  {}", min.len(), expr.len(), min);
        }
        Some("fuzz") => {
            let iters: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
            let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0)
            });
            println!("differential fuzz: {} iterations, seed {} (native vs interpreter)", iters, seed);
            match fuzz::run(iters, seed) {
                Ok(st) => println!(
                    "ok — no divergence: {} agreed, {} native-declined, {} skipped",
                    st.agreed, st.declined, st.skipped
                ),
                Err(e) => {
                    eprintln!("DIVERGENCE FOUND (reproduce with the seed above):\n{}", e);
                    std::process::exit(1);
                }
            }
        }
        Some("ping") => println!("{}", if anvild::ping() { "up" } else { "down" }),
        Some("stop") => println!("{}", if anvild::stop() { "stopped" } else { "no daemon" }),
        Some("stats") => match anvild::stats() {
            Some((n, b)) => println!("daemon cache: {} program(s), {:.1} MiB", n, b as f64 / (1u64 << 20) as f64),
            None => println!("no daemon"),
        },
        Some("warm") => {
            let expr = args[1..].join(" ");
            if expr.is_empty() {
                eprintln!("usage: latte anvil warm \"<expr>\"");
                return;
            }
            if !anvild::ping() {
                eprintln!("no daemon (start one with `latte anvil serve`)");
                return;
            }
            let libs = latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            println!("{}", if anvild::warm(&expr, &refs) { "warmed" } else { "warm failed" });
        }
        _ => eprintln!("usage: latte anvil <serve|ping|stop|stats|warm \"<expr>\"|fuzz [iters] [seed]|shrink \"<expr>\"|registry serve [addr] [root]>"),
    }
}

/// `latte profile "<expr>"` — the code profiler: measure the program on both engines,
/// persist the timings, and report which engine the adaptive policy will now choose.
/// Slow-when-interpreted programs are compiled automatically on their next run — the
/// measurement, not a structural guess, is what decides (see rustgen::run_adaptive).
fn cmd_profile(args: &[String]) {
    if args.iter().any(|a| a == "--list") {
        println!("{}", rustgen::profile_list());
        return;
    }
    let expr = match args.iter().find(|a| !a.starts_with("--")) {
        Some(e) => e.as_str(),
        None => {
            eprintln!("usage: latte profile \"<expr>\"   (measure both engines; the adaptive policy uses the result)");
            eprintln!("       latte profile --list       (every measured program, hottest first, with decisions)");
            return;
        }
    };
    let libs_owned = latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    match rustgen::profile_report(expr, &libs) {
        Ok(r) => println!("{}", r),
        Err(e) => eprintln!("profile error: {}", e),
    }
}

fn cmd_cli() {
    use std::io::Write;
    let libs_owned = latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    println!("Orpheus command line.  Type a Latte expression, or a meta-command:");
    println!("  :type EXPR   infer a type      :rust EXPR   compile to native Rust and run");
    println!("  def NAME [args] = BODY   define a function for this session (def lists; undef NAME removes)");
    println!("  :libs        list libraries    :status      instance report   :help  this help   :q  quit\n");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    print!("orpheus> ");
    let _ = out.flush();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let t = line.trim();
        if t.is_empty() {
            print!("orpheus> ");
            let _ = out.flush();
            continue;
        }
        if t == ":q" || t == ":quit" || t == ":exit" {
            break;
        } else if t == ":help" {
            println!(":type EXPR | :rust EXPR | :libs | :status | :version | def NAME [args] = BODY | undef NAME | :q | otherwise evaluate EXPR");
        } else if t == ":status" || t == ":doctor" {
            print!("{}", cliutil::status_report());
        } else if t == ":version" {
            println!("{}", cliutil::version_line());
        } else if t == "def" || t.starts_with("def ") {
            match latte::define_user_arm(t.strip_prefix("def").unwrap_or("")) {
                Ok(m) | Err(m) => println!("{}", m),
            }
        } else if let Some(n) = t.strip_prefix("undef ") {
            match latte::undefine_user_arm(n) {
                Ok(m) | Err(m) => println!("{}", m),
            }
        } else if t == ":libs" {
            println!("{}", latte::all_libs().join(" "));
        } else if let Some(e) = t.strip_prefix(":type ") {
            match latte::parse(e).and_then(|a| check::check(&a).map_err(|e| e)) {
                Ok(ty) => println!("{} : {}", e, ty.show()),
                Err(err) => println!("type error: {}", err),
            }
        } else if let Some(e) = t.strip_prefix(":rust ") {
            match rustgen::compile_to_rust(e, &libs) {
                Ok(_) => match rustgen::run_native_noun(e, &libs) {
                    Some(n) => println!("(native) {}", net::show_state(&n)),
                    None => println!("(native) the generated program failed to build or run"),
                },
                Err(err) => println!("compile error: {}", err),
            }
        } else {
            // refresh the scope each line: a `def` above registers the `user` module
            let libs_now = latte::all_libs();
            let libs: Vec<&str> = libs_now.iter().map(|s| s.as_str()).collect();
            if let Some(out) = eval_native(t, &libs, false) {
                println!("{}", out);
            } else {
                match latte::run_with_libs(t, &libs) {
                    Ok(v) => println!("{}", net::show_state(&v)),
                    Err(e) => println!("error: {}", e),
                }
            }
        }
        print!("orpheus> ");
        let _ = out.flush();
    }
}

fn cmd_rustc(args: &[String]) {
    let mut expr: Option<String> = None;
    let mut outfile: Option<String> = None;
    let mut run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => { i += 1; outfile = args.get(i).cloned(); }
            "--run" => run = true,
            other => { if expr.is_none() { expr = Some(other.to_string()); } }
        }
        i += 1;
    }
    let expr = match expr { Some(e) => e, None => { eprintln!("usage: latte rustc \"<expr>\" [-o file.rs] [--run]"); return; } };
    let libs_owned = latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    let src = match rustgen::compile_to_rust(&expr, &libs) {
        Ok(s) => s,
        Err(e) => { eprintln!("rustc: {}", e); std::process::exit(1); }
    };
    if let Some(f) = &outfile {
        if let Err(e) = std::fs::write(f, &src) { eprintln!("rustc: write {}: {}", f, e); return; }
        eprintln!("wrote {} ({} bytes of Rust)", f, src.len());
    }
    if run {
        match rustgen::run_native_noun(&expr, &libs) {
            Some(n) => println!("{}", net::show_state(&n)),
            None => eprintln!("rustc: the generated program failed to build or run"),
        }
        return;
    }
    if outfile.is_none() {
        print!("{}", src);
    }
}

fn main() {
    // the user package space: ./pkg next to the binary's working directory
    latte::set_pkg_dir(std::path::PathBuf::from("pkg"));
    let _ = latte::load_packages();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Starting Orpheus with no command launches the GUI (Hymn + console + editor).
    if args.is_empty() {
        cmd_gui(&[]);
        return;
    }
    let cmd = args.get(0).map(|s| s.as_str()).unwrap_or("gui");
    // Usability front matter: version, help, and the status report are answered here so
    // they work identically as `latte version`, `latte --version`, `-V`, etc.
    match cmd {
        "--version" | "-V" | "version" => {
            println!("{}", cliutil::version_line());
            return;
        }
        "status" | "doctor" | "info" => {
            print!("{}", cliutil::status_report());
            return;
        }
        "help" | "--help" | "-h" => {
            match args.get(1).map(|s| s.as_str()) {
                Some(topic) => print_command_help(topic),
                None => print_usage(),
            }
            return;
        }
        _ => {}
    }
    match cmd {
        "cli" | "console" | "--cli" => cmd_cli(),
        "cache" => cmd_cache(&args[1..]),
        "profile" => cmd_profile(&args[1..]),
        "worker" | "dist-worker" => cmd_worker(&args[1..]),
        "workers" => cmd_workers(&args[1..]),
        "anvil" => cmd_anvil(&args[1..]),
        "net" => cmd_net(&args[1..]),
        "node" => cmd_node(&args[1..]),
        "eval" => cmd_eval(&args[1..]),
        "agent" => cmd_agent(),
        "selftest" => cmd_selftest(),
        "bench" => cmd_bench(&args[1..]),
        "sca" => cmd_sca(&args[1..]),
        "evolve" => {
            for w in &args[1..] {
                match sca::evolve_latte(w) {
                    Ok(h) => println!("{}  ->  {}", w, h),
                    Err(e) => eprintln!("evolve error: {}", e),
                }
            }
        }
        "mold" => mold::cmd_mold(),
        "typecheck" => cmd_typecheck(&args[1..]),
        "mocha" => mocha::cmd_mocha(&args[1..]),
        "plan" => plan::cmd_plan(&args[1..]),
        "team" => mocha::cmd_team(&args[1..]),
        "repl" => repl::cmd_repl(),
        "gui" | "start" => cmd_gui(&args[1..]),
        "tensor" => numerics::cmd_tensor(&args[1..]),
        "ddia" => ddia::cmd_ddia(&args[1..]),
        "algo" => ddia::cmd_algo(&args[1..]),
        "dsa" => ddia::cmd_dsa(&args[1..]),
        "wgraph" => ddia::cmd_wgraph(&args[1..]),
        "numth" => ddia::cmd_numth(&args[1..]),
        "bits" => ddia::cmd_bits(&args[1..]),
        "strings" => ddia::cmd_strings(&args[1..]),
        "grid" => ddia::cmd_grid(&args[1..]),
        "design" => ddia::cmd_design(&args[1..]),
        "trees" => ddia::cmd_trees(&args[1..]),
        "dp" => ddia::cmd_dp(&args[1..]),
        "intervals" => ddia::cmd_intervals(&args[1..]),
        "search" => ddia::cmd_search(&args[1..]),
        "graphs" => ddia::cmd_graphs(&args[1..]),
        "backtrack" => ddia::cmd_backtrack(&args[1..]),
        "greedy" => ddia::cmd_greedy(&args[1..]),
        "db" => dbservice::cli(&args[1..]),
        "ml" => numerics::cmd_ml(&args[1..]),
        "nn" => numerics::cmd_nn(&args[1..]),
        "fin" => numerics::cmd_fin(&args[1..]),
        "trade" | "advisor" => numerics::cmd_trade(&args[1..]),
        "money" | "bonds" | "lab" => {
            // `latte bonds` / `latte lab` are shorthand for `latte money bonds` / `latte money lab`
            if args[0] == "bonds" || args[0] == "lab" {
                let mut a = vec![args[0].clone()];
                a.extend_from_slice(&args[1..]);
                numerics::cmd_money(&a[..]);
            } else {
                numerics::cmd_money(&args[1..]);
            }
        }
        "fetch" => numerics::cmd_fetch(&args[1..]),
        "ta" | "indicators" => numerics::cmd_ta(&args[1..]),
        "debug" => {
            // `latte debug [--break ARM] "<expr>"` — run with the tracer and
            // print the call tree (every arm call: args -> result, nested)
            let mut focus: Option<String> = None;
            let mut expr = String::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--break" | "--focus" if i + 1 < args.len() => {
                        focus = Some(args[i + 1].clone());
                        i += 1;
                    }
                    other => {
                        if !expr.is_empty() {
                            expr.push(' ');
                        }
                        expr.push_str(other);
                    }
                }
                i += 1;
            }
            if expr.is_empty() {
                eprintln!("usage: latte debug [--break ARM] \"<expr>\"");
                return;
            }
            let libs: Vec<String> = latte::all_libs();
            let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
            match latte::debug_trace(&expr, &refs, focus.as_deref()) {
                Ok((result, roots, truncated)) => {
                    println!("debug — the Loom call tracer (every arm call, nested)\n");
                    fn pr(n: &latte::TraceNode, depth: usize) {
                        let pad = "  ".repeat(depth);
                        println!(
                            "{}{} {}  ->  {}",
                            pad,
                            n.name,
                            n.args,
                            n.result.as_deref().unwrap_or("<crash>")
                        );
                        for c in &n.children {
                            pr(c, depth + 1);
                        }
                    }
                    for r in &roots {
                        pr(r, 0);
                    }
                    if roots.is_empty() {
                        println!("  (no traced calls{})", focus.as_deref().map(|f| format!(" matching --break {}", f)).unwrap_or_default());
                    }
                    if truncated {
                        println!("\n  … trace capped at 6000 calls (narrow it with --break ARM)");
                    }
                    println!("\nresult: {}", net::show_state(&result));
                }
                Err(e) => eprintln!("debug: {}", e),
            }
        }
        "fmt" | "format" => {
            // `latte fmt <file> [--write]` — format a Latte source (stdout, or in place)
            let write = args.iter().any(|a| a == "--write" || a == "-w");
            match args.get(1).filter(|a| !a.starts_with('-')) {
                Some(path) => match std::fs::read_to_string(path) {
                    Ok(src) => {
                        let f = latte::format_source(&src);
                        if write {
                            if f != src {
                                std::fs::write(path, f.as_bytes()).expect("write");
                                println!("formatted {}", path);
                            } else {
                                println!("{} already formatted", path);
                            }
                        } else {
                            print!("{}", f);
                        }
                    }
                    Err(e) => eprintln!("fmt: {}: {}", path, e),
                },
                None => eprintln!("usage: latte fmt <file.lat> [--write]"),
            }
        }
        "pkg" | "packages" => {
            // the package inventory: system libraries (lib/) vs user packages (pkg/)
            println!("packages — a package is a Latte source whose `core NAME` names it;");
            println!("compile one in the GUI (Compiler.Compile) and it is immediately part of");
            println!("the system; Store persists user modules to pkg/<name>.lat, which load");
            println!("automatically at startup.\n");
            println!("  system libraries (built in, sources in lib/):");
            let mut names = latte::all_libs();
            names.sort();
            names.dedup();
            let runtime = latte::runtime_lib_names();
            for n in &names {
                if !runtime.contains(n) {
                    print!("    {}", n);
                }
            }
            println!("\n\n  user packages (pkg/{{name}}.lat, loaded at startup):");
            if runtime.is_empty() {
                println!("    (none — Store a module from the GUI, or drop a .lat file in pkg/)");
            }
            for n in &runtime {
                println!("    {}  (pkg/{}.lat)", n, n);
            }
        }
        "trace" | "raytrace" => viz::cmd_trace(&args[1..]),
        "sentiment" => numerics::cmd_sentiment(&args[1..]),
        "news" | "wire" => newswire::cmd_news(&args[1..]),
        "gfx" | "draw" => numerics::cmd_gfx(&args[1..]),
        "gpu" | "compute" => numerics::cmd_gpu(&args[1..]),
        "chart" => viz::cmd_chart(&args[1..]),
        "jit" => cmd_jit(&args[1..]),
        "game" => game::cmd_game(&args[1..]),
        "rustc" | "build-rust" => cmd_rustc(&args[1..]),
        "icomb" => icomb::cmd_icomb(),
        "serve" => cmd_serve(&args[1..]),
        "android" => cmd_android(&args[1..]),
        other => {
            let sugg = cliutil::suggest(other, cliutil::COMMANDS);
            if !sugg.is_empty() {
                let list = sugg.join(", ");
                eprintln!("latte: unknown command '{}'. Did you mean: {}?\n", other, list);
            } else {
                eprintln!("latte: unknown command '{}'.\n", other);
            }
            print_usage();
        }
    }
}


fn cmd_gui(args: &[String]) {
    println!("{}", cliutil::version_line());
    let mut listen = "127.0.0.1:8088".to_string();
    let mut root = "lib/site".to_string();
    let mut store: Option<String> = None;
    let mut open: Option<bool> = None; // None = auto (open iff a desktop session is present)
    let mut chess_listen = String::new();
    let mut chess_peers: Vec<String> = Vec::new();
    let mut kv_store: Option<String> = None;
    let mut kv_listen = "0.0.0.0:9600".to_string();
    let mut kv_peers: Vec<String> = Vec::new();
    let mut kv_id: Option<u64> = None;
    let mut notes_store: Option<String> = None;
    let mut notes_listen = "0.0.0.0:9601".to_string();
    let mut notes_peers: Vec<String> = Vec::new();
    let mut notes_id: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => { i += 1; listen = args[i].clone(); }
            "--root" => { i += 1; root = args[i].clone(); }
            "--store" => { i += 1; store = Some(args[i].clone()); }
            "--open" => { open = Some(true); }
            "--no-open" => { open = Some(false); }
            "--chess-listen" => { i += 1; chess_listen = args[i].clone(); }
            "--peer" => { i += 1; chess_peers.push(args[i].clone()); }
            "--kv-store" => { i += 1; kv_store = Some(args[i].clone()); }
            "--kv-listen" => { i += 1; kv_listen = args[i].clone(); }
            "--kv-peer" => { i += 1; kv_peers.push(args[i].clone()); }
            "--kv-id" => { i += 1; kv_id = args[i].parse().ok(); }
            "--notes-store" => { i += 1; notes_store = Some(args[i].clone()); }
            "--notes-listen" => { i += 1; notes_listen = args[i].clone(); }
            "--notes-peer" => { i += 1; notes_peers.push(args[i].clone()); }
            "--notes-id" => { i += 1; notes_id = args[i].parse().ok(); }
            other => { eprintln!("gui: unknown arg {}", other); return; }
        }
        i += 1;
    }
    // The LEDGER: the GUI's persistent, gossiped kv node (src/ledger.rs) —
    // the state behind the /network page and the Kv.* tools. It listens for
    // peers by default (--kv-listen off to refuse), dials --kv-peer addresses
    // with retry-forever connectors, and is durable when --kv-store names a
    // directory. Two GUIs pointed at each other converge on one ledger.
    if kv_listen == "off" || kv_listen == "none" {
        kv_listen.clear();
    }
    // COLLABORATIVE NOTES: a second gossiped node (the notes agent,
    // lib/notes.lat) beside the ledger, on its own port — the state behind
    // the /notes editor and the Note.* tools.
    if notes_listen == "off" || notes_listen == "none" {
        notes_listen.clear();
    }
    match notes::init(notes_store.as_deref(), &notes_listen, &notes_peers, notes_id) {
        Ok(desc) => println!("{}", desc),
        Err(e) => {
            eprintln!("gui: notes node failed to start: {}", e);
            return;
        }
    }
    match ledger::init(kv_store.as_deref(), &kv_listen, &kv_peers, kv_id) {
        Ok(desc) => {
            println!("{}", desc);
            if !kv_listen.is_empty() || !notes_listen.is_empty() {
                println!("  (these node ports trust any host that can reach them — LAN/VPN/tunnel; docs/network-gui.md)");
            }
        }
        Err(e) => {
            eprintln!("gui: ledger failed to start: {}", e);
            return;
        }
    }
    let src = mocha::EDITOR_LAT;
    let agent = match agent::Agent::from_source(src, "editor") {
        Ok(a) => a,
        Err(e) => { eprintln!("gui: editor app failed to compile: {}", e); return; }
    };
    let q = match mocha::Mocha::load(src) {
        Ok(q) => q,
        Err(e) => { eprintln!("gui: {}", e); return; }
    };
    let node = match &store {
        Some(dir) => match net::Node::open(0xED170, agent, dir, 0) {
            Ok(n) => n,
            Err(e) => { eprintln!("gui: cannot open store: {}", e); return; }
        },
        None => net::Node::new(0xED170, agent),
    };
    let editor = serve::Editor::new(node, q);

    // The chess board's game runs as a Mocha app on its own Node, so moves gossip to any
    // peer machines (`--peer ADDR`). Standalone (no peers) it still backs vs-model and
    // local two-player. `--chess-listen` opens it for other machines to connect to.
    let chess = build_chess_node(&chess_listen, &chess_peers, store.as_deref());

    let url = format!("http://{}/", listen);
    println!("Orpheus GUI — open these in a browser:");
    println!("  {}          the System console (instructions + run every tool)", url);
    println!("  http://{}/editor    the WYSIWYG Facet editor", listen);
    println!("  http://{}/chess     play chess (vs the model, local 2-player, or networked)", listen);
    println!("  http://{}/chart     data visualization", listen);
    println!("  http://{}/plan      economic planner", listen);
    println!("  http://{}/network   connect instances: the shared ledger, peers, workers, training", listen);
    println!("  http://{}/notes     collaborative notes: write together across connected instances", listen);
    println!("  http://{}/board     the shared message board (persistent, multi-user)", listen);
    if !chess_listen.is_empty() || !chess_peers.is_empty() {
        println!("  chess node: listen='{}' peers={:?}", chess_listen, chess_peers);
    }
    // When launched from a desktop session (e.g. GNOME), pop the GUI in a window.
    if open.unwrap_or_else(desktop_session_present) {
        let u = url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(600));
            open_in_window(&u);
        });
    }
    serve::serve_gui(&listen, &root, editor, chess);
}

/// Build the chess Mocha node (rules in Latte: `lib/chessgame.lat`), start its networking,
/// and wrap it as a `ChessHandle`. Returns `None` only if the chess app fails to compile.
fn build_chess_node(listen: &str, peers: &[String], store: Option<&str>) -> Option<serve::ChessHandle> {
    let src = mocha::CHESSGAME_LAT;
    let agent = match agent::Agent::from_source(src, "chessgame") {
        Ok(a) => a,
        Err(e) => { eprintln!("gui: chess app failed to compile: {}", e); return None; }
    };
    let q = match mocha::Mocha::load(src) {
        Ok(q) => q,
        Err(e) => { eprintln!("gui: chess app: {}", e); return None; }
    };
    // a fixed id keeps the durable store stable; the agent cid is shared across machines
    let node = match store.map(|d| format!("{}/chess", d)) {
        Some(dir) => match net::Node::open(0xC4E55, agent, &dir, 0) {
            Ok(n) => n,
            Err(_) => net::Node::new(
                0xC4E55,
                agent::Agent::from_source(src, "chessgame").expect("chess app recompiles"),
            ),
        },
        None => net::Node::new(0xC4E55, agent),
    };
    let node = std::sync::Arc::new(std::sync::Mutex::new(node));
    let cfg = std::sync::Arc::new(net::Config {
        name: "chess".to_string(),
        listen: if listen.is_empty() { "127.0.0.1:0".to_string() } else { listen.to_string() },
        peers: peers.to_vec(),
        verbose: false,
        compact_every: 0,
        psk: crate::secure::configured_psk(None),
    });
    let peers_handle = net::start(node.clone(), cfg);
    Some(serve::Chess::new(node, peers_handle, q))
}

/// True if we appear to be inside a graphical desktop session (X11 or Wayland),
/// or on Android (Termux exposes no DISPLAY, but termux-open-url / `am start`
/// hand the URL to the system browser).
fn desktop_session_present() -> bool {
    std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("TERMUX_VERSION").is_some()
        || std::env::var_os("ANDROID_ROOT").is_some()
}

/// Open `url` in a window. Prefers a chromeless "app mode" window from a Chromium-class
/// browser or GNOME Web, falling back to the desktop's default handler (xdg-open / gio open).
fn open_in_window(url: &str) {
    use std::process::Command;
    if let Some(b) = std::env::var_os("BROWSER") {
        if Command::new(b).arg(url).spawn().is_ok() {
            return;
        }
    }
    let app_mode = [
        "google-chrome-stable", "google-chrome", "chromium",
        "chromium-browser", "brave-browser", "microsoft-edge",
    ];
    for b in app_mode {
        if Command::new(b)
            .arg(format!("--app={}", url))
            .arg("--new-window")
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    if Command::new("epiphany").arg("--application-mode").arg(url).spawn().is_ok() {
        return;
    }
    if Command::new("xdg-open").arg(url).spawn().is_ok() {
        return;
    }
    if Command::new("gio").arg("open").arg(url).spawn().is_ok() {
        return;
    }
    // Android: Termux's opener, then the bare activity manager (adb shells)
    if Command::new("termux-open-url").arg(url).spawn().is_ok() {
        return;
    }
    if Command::new("am")
        .args(["start", "-a", "android.intent.action.VIEW", "-d"])
        .arg(url)
        .spawn()
        .is_ok()
    {
        return;
    }
    eprintln!("(no browser found to open {} — open it manually)", url);
}

/// `latte android` — the phone entry point, for the Termux-free app.
///
/// The Android app (see android/) ships this binary inside its APK as
/// `lib/arm64-v8a/liblatte.so`, the one place Android still extracts a file with the
/// execute bit set. Its Activity spawns `liblatte.so android` and shows a WebView on the
/// URL this prints. Everything the GUI needs is INSIDE the executable (the `.lat`
/// libraries always were; `src/site.rs` now embeds the pages too), so there is no
/// repository, no Termux, and no writable-exec directory anywhere in the picture.
///
/// The command is a thin, honest wrapper: point every cache and store at the app's
/// private directory (which is what `HOME` already governs), bind loopback ONLY (an
/// Android app's port would otherwise be reachable from the local network), pick a free
/// port when 8088 is taken, print the URL on a line the Activity parses, and serve.
fn cmd_android(args: &[String]) {
    let mut port: u16 = 8088;
    let mut home: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => { i += 1; port = args.get(i).and_then(|p| p.parse().ok()).unwrap_or(8088); }
            "--home" => { i += 1; home = args.get(i).cloned(); }
            other => { eprintln!("unknown arg {}", other); return; }
        }
        i += 1;
    }
    // App-private storage: the Activity passes filesDir. Everything derived from HOME
    // (the market cache, the news wire, the Anvil program cache) then lands inside the
    // app's sandbox, which is the only place an Android app may write.
    if let Some(h) = &home {
        std::env::set_var("HOME", h);
        let _ = std::fs::create_dir_all(h);
    }
    // Loopback only. A phone is usually on someone else's Wi-Fi; the GUI exposes eval and
    // the ledger, so it must not be reachable from the network. Peers reach this node
    // through `latte net` with a PSK, never through the web console.
    let listen = {
        let mut chosen = format!("127.0.0.1:{}", port);
        if std::net::TcpListener::bind(&chosen).is_err() {
            match std::net::TcpListener::bind("127.0.0.1:0") {
                Ok(l) => {
                    let p = l.local_addr().map(|a| a.port()).unwrap_or(0);
                    drop(l);
                    chosen = format!("127.0.0.1:{}", p);
                }
                Err(e) => {
                    eprintln!("android: cannot bind a local port: {}", e);
                    return;
                }
            }
        }
        chosen
    };
    // A phone has no rustc; skip the probe so startup never spawns a process (which can
    // block under some Android sandboxes). The interpreter + JIT is the engine here.
    std::env::set_var("ORPHEUS_NO_RUSTC", "1");
    println!("Orpheus on Android — interpreter + JIT (no rustc on this device; that is fine)");
    println!("  storage: {}", std::env::var("HOME").unwrap_or_else(|_| "<unset>".into()));
    println!("  native : interpreter + JIT (cached native binaries still run if present)");
    // The Activity greps for exactly this line to point its WebView.
    println!("ORPHEUS_URL http://{}/", listen);
    // The site is embedded, so "lib/site" simply won't exist and serve() falls back.
    serve::serve(&listen, "lib/site");
}

/// The top-level usage text (also `latte help`).
fn print_usage() {
    eprintln!(
        "usage:\n  latte                              start the GUI (default) — open the printed URL
  latte node ...
  latte eval \"<expr>\"
  latte agent | selftest | bench [N]
  latte sca <word> <rule>..
  latte evolve <solar-word>..        SCArs: derive Heart Speech via lib/ligurian.sca
  latte mold                         tour the mold/aura type system
  latte typecheck <expr>             statically infer an expression's type
  latte mocha --app NAME ...        run a Mocha app (todo, lexicon, forge)
  latte plan [--iters N]             planning calc (Towards a New Socialism)
  latte team --as NAME --share ...   collaborative coding across machines (Forge)
  latte repl                         self-hosting Latte environment (REPL)\n  latte cli                          interactive command line (eval · :type · :rust · :libs)\n  latte cache [path|clear]           manage the compiled-program cache (Anvil)\n  latte profile \"<expr>\" | --list  the code profiler: measure both engines + detect distributable shapes; --list the table\n  latte worker [--listen ADDR]       serve evaluation tasks to connected instances (default 0.0.0.0:9700)\n  latte workers [add|rm|list|clear]  the worker registry: distribution is ON by default once workers are added\n  latte tensor                       n-dimensional tensor demo (lib/tensor.lat)\n  latte ddia [topic]                 data-intensive techniques in Latte: bloom, lsm, btree, vclock, crdt, lamport, chash, quorum, wire, mapred, merkle, mvcc, hll, cms, stream, raft\n  latte ml [linear|perceptron|kmeans|knn] [--iters N]  train a model in Latte (lib/ml.lat)\n  latte chart [bar|line|scatter] N.. data visualization to SVG (lib/plot.lat)\n  latte nn                           neural network with backprop (lib/nn.lat)\n  latte fin [--vol|--dir] [--iters N]  financial ML on Bitcoin: volatility-regime edge (lib/fin.lat)\n  latte gfx                          graphics: draw a scene to SVG (lib/gfx.lat)\n  latte gpu [--dim N]                data-parallel compute, auto-detects GPU (lib/gpu.lat)\n  latte trade [--account N] [--kelly F] [--sentiment S]  automatic trading advisor (sizing)\n  latte sentiment \"<text>\"          news sentiment: trained classifier + LM lexicon (+ --bond axis)\n  latte news [pulse|fetch|train|sources]  the NEWSWIRE: fresh press + social feeds, auto-fetched, event-aware weights\n  latte android [--home DIR]        the phone entry point (loopback GUI, embedded pages; used by the Android app)\n  latte jit \"<expr>\"               run on the JIT vs the interpreter (compare + time)\n  latte game chess [--max N] [--show K]  run a chess match between two machines\n  latte rustc \"<expr>\" [-o f.rs] [--run]  compile a Latte expression to native Rust (Anvil)\n  latte icomb                        interaction-combinator reduction (Lafont γ/δ/ε)\n  latte net \\\"<expr>\\\"               compile +/*/</if to an interaction net and reduce\n  latte gui [--listen ADDR] [--store D] [--kv-store D] [--kv-listen ADDR|off] [--kv-peer ADDR] [--notes-store D] [--notes-listen ADDR|off] [--notes-peer ADDR] [--chess-listen ADDR] [--peer ADDR]  GUI: System, editor, charts, chess, /network (ledger listens on 9600; --kv-peer links ledgers)\n  latte serve [--listen ADDR] [--root DIR]   run Hymn, hosting Facet pages (default lib/site)\n  latte status                       one screen: version, engine, cache, news wire, security, workers (alias: doctor)\n  latte version                      print the version and engine mode\n  latte help [command]               this list, or detailed help for one command"
    );
}

/// Per-command help: `latte help <cmd>`. Falls back to a pointer at the full usage and
/// the docs when a command has no dedicated blurb yet.
fn print_command_help(topic: &str) {
    let blurb: Option<&str> = match topic {
        "news" | "wire" => Some("latte news [pulse|fetch|train|sources] [--market SYM] [--live]\n  The NEWSWIRE: fresh press RSS + social posts, auto-fetched (30-min TTL), scored with\n  event-aware weights and causal routing. `pulse` shows the scored feed for a market;\n  `fetch` pulls now; `train` fits the SESTM return-supervised model; `sources` lists feeds.\n  See docs/newswire.md."),
        "trade" | "advisor" => Some("latte trade [--market SYM] [--account N] [--kelly F] [--news FILE] [--sentiment S] [--live]\n  The trading advisor: technical composite (60%) fused with event-aware news sentiment\n  (40%), sized by fractional Kelly x volatility targeting. `--market bonds` runs the\n  duration desk on live FRED yields. See docs/visualization-and-ml.md."),
        "node" => Some("latte node --listen ADDR [--peer ADDR].. [--psk SECRET] [--store DIR] [--do ACTION].. [--run-secs N]\n  Run a gossip node that converges shared state with its peers. With a PSK (flag,\n  ORPHEUS_PSK, or a psk file in --store) every peer link is mutually authenticated and\n  encrypted. See docs/security.md."),
        "serve" | "gui" | "start" => Some("latte gui | serve [--listen ADDR] [--root DIR]\n  The web console (System, editor, charts, chess, /network, the tools page). Loopback is\n  an open personal console; a public bind requires ORPHEUS_TOKEN (or a derived PSK token)\n  or it is refused. See docs/security.md."),
        "android" => Some("latte android [--home DIR] [--port N]\n  The phone entry point: binds the GUI to loopback, embeds all pages, prints ORPHEUS_URL\n  for the app WebView. No rustc needed — the interpreter is the engine. See docs/android.md."),
        "cache" => Some("latte cache [status|metrics|verify [--repair]|log|warm \"<expr>\"|clear]\n  The Anvil compiled-program cache. `status` shows size and location; `warm` prebuilds an\n  expression; `clear` empties it. ORPHEUS_CACHE relocates it."),
        "eval" => Some("latte eval [--explain] [--lib NAME=PATH].. \"<expr>\"\n  Evaluate a Latte expression with the standard libraries in scope. `--explain` reports\n  the run plan (native vs interpret). ORPHEUS_FUEL=<n> sets the step budget (0 = unlimited)."),
        "status" | "doctor" => Some("latte status\n  One screen of the instance's state: version, engine mode, cache, news wire, security,\n  and workers. Alias: latte doctor."),
        _ => None,
    };
    match blurb {
        Some(b) => println!("{}", b),
        None => {
            let sugg = cliutil::suggest(topic, cliutil::COMMANDS);
            if !sugg.is_empty() && sugg[0] != topic {
                println!("no detailed help for '{}'. Did you mean: {}?\n", topic, sugg.join(", "));
            } else {
                println!("no detailed help for '{}' yet — see `latte help` and the docs/ directory.\n", topic);
            }
            print_usage();
        }
    }
}

fn cmd_serve(args: &[String]) {
    let mut listen = "127.0.0.1:8080".to_string();
    let mut root = "lib/site".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => { i += 1; listen = args.get(i).cloned().unwrap_or(listen); }
            "--root" => { i += 1; root = args.get(i).cloned().unwrap_or(root); }
            other => { eprintln!("unknown arg {}", other); return; }
        }
        i += 1;
    }
    serve::serve(&listen, &root);
}

fn cmd_sca(args: &[String]) {
    // `latte sca --file RULES.sca <word>..` applies a whole rule file (with `class` lines);
    // `latte sca <word> <rule>..` applies inline rules.
    if args.first().map(|s| s.as_str()) == Some("--file") {
        let path = match args.get(1) {
            Some(p) => p,
            None => { eprintln!("usage: latte sca --file RULES.sca <word>.."); return; }
        };
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => { eprintln!("sca: cannot read {}: {}", path, e); return; }
        };
        // an assembled soundlib file records its ordered selection in a
        // `:: changes: id id …` header — surface it, so the user sees WHICH
        // named changes this file applies and in what order
        let sel = crate::conlang::sca_selection(&src);
        if !sel.is_empty() {
            println!("applying {} change(s): {}", sel.len(), sel.join(" → "));
        }
        for word in &args[2..] {
            match sca::run_sca_latte(word, &[src.clone()]) {
                Ok(out) => println!("{}  -->  {}", word, out),
                Err(e) => eprintln!("{} ! {}", word, e),
            }
        }
        return;
    }
    if args.is_empty() {
        eprintln!("usage: latte sca <word> <rule>..   |   latte sca --file RULES.sca <word>..");
        eprintln!("  rule syntax: FROM>TO/PRE_POST   (omit /PRE_POST for unconditional; empty TO deletes)");
        eprintln!("examples:");
        eprintln!("  latte sca kasa k>g s>z/a_a            => gaza");
        eprintln!("  latte sca apataka p>b/a_a t>d/a_a k>g/a_a   (intervocalic voicing) => abadaga");
        eprintln!("  latte sca anta t>/n_                  (delete t after n) => ana");
        eprintln!("  latte sca --file lib/breaking.sca kása kásta   (stress/cluster breaking)");
        return;
    }
    let word = &args[0];
    let rules: Vec<String> = args[1..].to_vec();
    match sca::run_sca_latte(word, &rules) {
        Ok(out) => {
            let rulestr = if rules.is_empty() { "(no rules)".to_string() } else { rules.join("  ") };
            println!("{}  --[ {} ]-->  {}", word, rulestr, out);
        }
        Err(e) => eprintln!("sca error: {}", e),
    }
}

fn cmd_bench(args: &[String]) {
    let n: u128 = args.get(0).and_then(|s| s.parse().ok()).unwrap_or(2_000_000);
    let agent = Agent::new().expect("agent compiles");
    let s0 = agent.initial_state();
    let action = agent::act_add(n);

    // First, prove equivalence on this very input with audit on.
    loom::set_jet_audit(true);
    loom::set_jets_enabled(true);
    let audited = agent.step(&action, &s0).expect("jet agrees with pure reduction");
    loom::set_jet_audit(false);

    // Pure interpreter (jets off): the in-language `loop` runs n increments.
    loom::set_jets_enabled(false);
    let t = std::time::Instant::now();
    let pure = agent.step(&action, &s0).expect("pure");
    let pure_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Jet (native add).
    loom::set_jets_enabled(true);
    let t = std::time::Instant::now();
    let jet = agent.step(&action, &s0).expect("jet");
    let jet_ms = t.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(pure, jet);
    assert_eq!(pure, audited);
    println!("add {}  (result {})", n, net::show_state(&pure));
    println!("  pure interpreter : {:>10.3} ms", pure_ms);
    println!("  jet (native)     : {:>10.3} ms", jet_ms);
    if jet_ms > 0.0 {
        println!("  speedup          : {:>10.0}x", pure_ms / jet_ms.max(0.0001));
    }
    println!("  results identical (audited) ✓");
}

fn now_nanos() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn cmd_agent() {
    let a = Agent::new().expect("agent compiles");
    println!("agent content-address (CID): {}", a.cid_hex());
}


/// `latte worker [--listen ADDR]` — serve evaluation tasks to connected
/// Orpheus instances. A worker is a full instance: tasks evaluate with every
/// library in scope on the adaptive engine.
fn cmd_worker(args: &[String]) {
    let mut listen = "0.0.0.0:9700".to_string();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--listen" && i + 1 < args.len() {
            listen = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    println!("Orpheus worker: serving evaluation tasks on {}", listen);
    println!("register on the coordinating instance:  latte workers add HOST:{}", listen.rsplit(':').next().unwrap_or("9700"));
    if let Err(e) = dist::serve(&listen, true) {
        eprintln!("worker: bind {} failed: {}", listen, e);
    }
}

/// `latte workers [list|add ADDR|rm ADDR|clear]` — manage the worker
/// registry. Once workers are registered, distribution is the DEFAULT: the
/// adaptive engine splits distributable programs across them automatically.
fn cmd_workers(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        None | Some("list") => {
            let ws = dist::workers();
            if ws.is_empty() {
                println!("no workers connected — distribution stays local.");
                println!("start one:      latte worker --listen 0.0.0.0:9700   (on another machine or shell)");
                println!("register it:    latte workers add HOST:9700");
                println!("or set          ORPHEUS_WORKERS=HOST:9700,HOST2:9700");
                return;
            }
            println!("connected workers (distribution is ON by default for distributable programs):");
            for w in ws {
                println!("  {}  {}", w, if dist::worker_alive(&w) { "alive" } else { "UNREACHABLE (chunks will fall back locally)" });
            }
        }
        Some("add") => match args.get(1) {
            Some(a) => match dist::workers_add(a) {
                Ok(()) => println!("added {} — the adaptive engine now distributes eligible work to it by default", a),
                Err(e) => eprintln!("workers add: {}", e),
            },
            None => eprintln!("usage: latte workers add HOST:PORT"),
        },
        Some("rm") => match args.get(1) {
            Some(a) => match dist::workers_remove(a) {
                Ok(()) => println!("removed {}", a),
                Err(e) => eprintln!("workers rm: {}", e),
            },
            None => eprintln!("usage: latte workers rm HOST:PORT"),
        },
        Some("clear") => match dist::workers_clear() {
            Ok(()) => println!("worker registry cleared — evaluation stays local"),
            Err(e) => eprintln!("workers clear: {}", e),
        },
        Some(x) => eprintln!("workers: unknown subcommand '{}' (list | add ADDR | rm ADDR | clear)", x),
    }
}

fn cmd_eval(args: &[String]) {
    let mut libs: Vec<String> = latte::all_libs();
    let mut rest: Vec<String> = Vec::new();
    let mut force_interp = false;
    let mut force_net = false;
    let mut force_rebuild = false;
    let mut explain = false;
    let mut no_dist = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--lib" {
            i += 1;
            if i < args.len() {
                if let Some((name, path)) = args[i].split_once('=') {
                    match std::fs::read_to_string(path) {
                        Ok(src) => {
                            latte::register_runtime_lib(name, &src);
                            libs.push(name.to_string());
                        }
                        Err(e) => {
                            eprintln!("--lib {}: {}", path, e);
                            return;
                        }
                    }
                } else {
                    eprintln!("--lib expects NAME=PATH");
                    return;
                }
            }
        } else if args[i] == "--interp" || args[i] == "--no-compile" {
            force_interp = true;
        } else if args[i] == "--net" {
            force_net = true;
        } else if args[i] == "--rebuild" {
            force_rebuild = true;
        } else if args[i] == "--no-dist" {
            no_dist = true;
        } else if args[i] == "--explain" || args[i] == "--why" {
            explain = true;
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    let src = rest.join(" ");
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    if explain {
        match rustgen::native_check(&src, &refs) {
            Ok(()) => println!(
                "native: yes — compiles to native code (if a run still falls back, see `latte cache log`)"
            ),
            Err(reason) => println!("native: no — {} → runs on the interpreter", reason),
        }
    }
    // `--net`: evaluate on the interaction-net engine (the numeric fragment), audited.
    if force_net {
        match icomb::run_str(&src) {
            Ok((v, steps)) => println!("{}  ({} interaction steps)", v, steps),
            Err(e) => eprintln!("net error: {}", e),
        }
        return;
    }
    // Distribution decision first (the default once workers are connected):
    // a distributable shape — explicit `dmap`, or a `map`/`predict_all` the
    // profiler has measured past the distribution threshold — is split across
    // the connected Orpheus instances; anything else stays local. `--no-dist`
    // (or ORPHEUS_DIST=0) opts out.
    if !force_interp && !no_dist {
        if let Some(r) = dist::maybe_distribute(&src, &refs) {
            match r {
                Ok(v) => println!("{}", net::show_state(&v)),
                Err(e) => eprintln!("error: {}", e),
            }
            return;
        }
    }
    // The optimizing compiler (Anvil) is the default engine. Policy, in order:
    //   1. binary already cached  → run it in-process (fast, no rustc);
    //   2. cold + daemon running  → ask the resident compiler to build in the background and answer
    //                               this call on the interpreter (so we never stall on a cold build);
    //   3. cold + no daemon       → build in-process now (cached for next time);
    //   4. anything declines      → interpreter.
    // On success the compiled and interpreted engines agree exactly, so a fallback is never wrong.
    if !force_interp {
        if force_rebuild {
            if let Some(out) = eval_native(&src, &refs, true) {
                println!("{}", out);
                return;
            }
        } else if let Some(n) = rustgen::run_native_cached(&src, &refs) {
            println!("{}", net::show_state(&n));
            return;
        } else if anvild::warm_bg(&src, &refs) {
            // resident compiler is building in the background; fall through to the interpreter
        } else if let Some(out) = eval_native(&src, &refs, false) {
            println!("{}", out);
            return;
        }
    }
    match latte::run_with_libs(&src, &refs) {
        Ok(v) => println!("{}", net::show_state(&v)),
        Err(e) => eprintln!("error: {}", e),
    }
}

/// Compile `expr` to native code via Anvil, build it (caching the binary by a hash of the emitted
/// source), run it, and return its stdout — or `None` if any stage fails (so the caller can fall
/// back to the interpreter).
fn eval_native(expr: &str, libs: &[&str], force_rebuild: bool) -> Option<String> {
    // Delegate to the shared Anvil runner (compile → cached build → run → noun), then render in
    // the CLI's style. This is the same engine the GUI console and other surfaces use.
    rustgen::run_native_noun_opts(expr, libs, force_rebuild).map(|n| net::show_state(&n))
}

fn cmd_jit(args: &[String]) {
    let src = args.join(" ");
    if src.is_empty() {
        eprintln!("usage: latte jit \"<expr>\"   (interpreter vs adaptive vs forced-compile: compare + time)");
        return;
    }
    let libs_owned = latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    let libs = &libs[..];
    // adaptive (the default): interpret cold, compile hot
    let t0 = std::time::Instant::now();
    let adaptive = latte::run_with_libs(&src, &libs);
    let at = t0.elapsed();
    // pure interpreter
    loom::set_jit_enabled(false);
    let t1 = std::time::Instant::now();
    let interp = latte::run_with_libs(&src, &libs);
    let it = t1.elapsed();
    loom::set_jit_enabled(true);
    // forced full compilation (threshold 0)
    loom::set_jit_threshold(0);
    let t2 = std::time::Instant::now();
    let forced = latte::run_with_libs(&src, &libs);
    let ft = t2.elapsed();
    loom::set_jit_threshold(32);
    match (&adaptive, &interp, &forced) {
        (Ok(a), Ok(b), Ok(c)) => {
            println!("result: {}", net::show_state(a));
            println!("agree:  interpreter={} forced-compile={}", a == b, a == c);
            println!("time:   interpreter {:?}   adaptive {:?}   forced-compile {:?}", it, at, ft);
            let best = if it <= at && it <= ft {
                "interpreter"
            } else if at <= ft {
                "adaptive"
            } else {
                "forced-compile"
            };
            println!("fastest here: {} (the adaptive default interprets one-shots, compiles hot loops)", best);
        }
        _ => println!("adaptive={:?}\ninterp={:?}\nforced={:?}", adaptive, interp, forced),
    }
}

fn cmd_selftest() {
    // Three independent nodes; each applies different actions; they exchange events
    // in different orders; all must end identical. (In-process, no sockets.)
    let a = Agent::new().unwrap();
    let mk = || Node::new(0, Agent::new().unwrap());
    let (mut n1, mut n2, mut n3) = (mk(), mk(), mk());
    n1.id = 1;
    n2.id = 2;
    n3.id = 3;

    let e1 = n1.local_action(agent::act_incr());
    let e2 = n1.local_action(agent::act_add(40));
    let e3 = n2.local_action(agent::act_incr());
    let e4 = n3.local_action(agent::act_add(100));
    let e5 = n3.local_action(agent::act_reset());
    let e6 = n3.local_action(agent::act_add(2));

    let all = [e1, e2, e3, e4, e5, e6];
    // deliver to n1 in forward order, n2 reversed, n3 shuffled
    for e in all.iter() {
        n1.add_event_knot(e.clone());
    }
    for e in all.iter().rev() {
        n2.add_event_knot(e.clone());
    }
    for idx in [3usize, 0, 5, 2, 4, 1] {
        n3.add_event_knot(all[idx].clone());
    }
    let (s1, s2, s3) = (n1.state().unwrap(), n2.state().unwrap(), n3.state().unwrap());
    println!("agent CID : {}", a.cid_hex());
    println!("node 1    : state={} cid={}", net::show_state(&s1), short(&s1.cid_hex()));
    println!("node 2    : state={} cid={}", net::show_state(&s2), short(&s2.cid_hex()));
    println!("node 3    : state={} cid={}", net::show_state(&s3), short(&s3.cid_hex()));
    if s1 == s2 && s2 == s3 {
        println!("CONVERGED ✓  (reset wiped 141, then +2)");
    } else {
        println!("DIVERGED ✗");
        std::process::exit(1);
    }
}

fn short(h: &str) -> String {
    h.chars().take(12).collect()
}


fn cmd_typecheck(args: &[String]) {
    if args.is_empty() {
        eprintln!("usage: latte typecheck \"<expr>\"");
        return;
    }
    let src = args.join(" ");
    match latte::parse(&src) {
        Ok(ast) => match check::check(&ast) {
            Ok(ty) => println!("{} : {}", src, ty.show()),
            Err(e) => println!("{}", e),
        },
        Err(e) => println!("parse error: {}", e),
    }
}

fn cmd_node(args: &[String]) {
    let mut listen = "127.0.0.1:9000".to_string();
    let mut peers = Vec::new();
    let mut dos = Vec::new();
    let mut run_secs: Option<u64> = None;
    let mut name = String::new();
    let mut id: Option<u64> = None;
    let mut verbose = false;
    let mut store_dir: Option<String> = None;
    let mut snapshot_every: usize = 32;
    let mut compact_every: usize = 0;
    let mut agent_name = String::from("v1");
    let mut psk_arg: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => { i += 1; listen = args[i].clone(); }
            "--peer" => { i += 1; peers.push(args[i].clone()); }
            "--do" => { i += 1; dos.push(args[i].clone()); }
            "--run-secs" => { i += 1; run_secs = args[i].parse().ok(); }
            "--name" => { i += 1; name = args[i].clone(); }
            "--id" => { i += 1; id = args[i].parse().ok(); }
            "--store" => { i += 1; store_dir = Some(args[i].clone()); }
            "--snapshot-every" => { i += 1; snapshot_every = args[i].parse().unwrap_or(32); }
            "--compact-every" => { i += 1; compact_every = args[i].parse().unwrap_or(0); }
            "--agent" => { i += 1; agent_name = args[i].clone(); }
            "--psk" => { i += 1; psk_arg = Some(args[i].clone()); }
            "-v" | "--verbose" => { verbose = true; }
            other => { eprintln!("unknown arg {}", other); return; }
        }
        i += 1;
    }
    if name.is_empty() {
        name = listen.clone();
    }
    let id = id.unwrap_or_else(|| now_nanos() ^ (std::process::id() as u64).wrapping_mul(2654435761));

    let agent = Agent::by_name(&agent_name).expect("agent compiles");
    println!("[{}] node id={} agent={} cid={}", name, id, agent.label(), short(&agent.cid_hex()));

    let node = match &store_dir {
        Some(dir) => {
            let n = Node::open(id, agent, dir, snapshot_every).expect("open store");
            if n.migrated {
                println!("[{}] agent program changed since last run — discarded stale snapshot and re-folded the durable log (safe upgrade, no breach).", name);
            }
            let recovered = n.event_count();
            if recovered > 0 {
                let st = n.state().map(|s| net::show_state(&s)).unwrap_or_else(|_| "<crash>".into());
                println!("[{}] recovered {} event(s) from {} — state={}", name, recovered, dir, st);
            }
            n
        }
        None => Node::new(id, agent),
    };
    let node = Arc::new(Mutex::new(node));
    let psk = match &psk_arg {
        Some(p) => Some(crate::secure::derive_psk(p)),
        None => crate::secure::configured_psk(store_dir.as_deref().map(std::path::Path::new)),
    };
    if psk.is_some() {
        println!("[{}] secure transport ENABLED (pre-shared key; peers are mutually authenticated and encrypted)", name);
    }
    let cfg = Arc::new(Config { name: name.clone(), listen: listen.clone(), peers: peers.clone(), verbose, compact_every, psk });
    let peers_handle = net::start(node.clone(), cfg);

    // perform startup actions after a short delay so links can establish
    if !dos.is_empty() {
        let node2 = node.clone();
        let peers2 = peers_handle.clone();
        let dos2 = dos.clone();
        let nm = name.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1200));
            for d in dos2 {
                if let Some(act) = agent::parse_action(&d) {
                    net::submit(&node2, &peers2, act);
                    println!("[{}] did: {}", nm, d);
                } else {
                    eprintln!("[{}] bad --do '{}'", nm, d);
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        });
    }

    match run_secs {
        Some(secs) => {
            std::thread::sleep(Duration::from_secs(secs));
            let mut n = node.lock().unwrap();
            let st = n.state().map(|s| (net::show_state(&s), short(&s.cid_hex())))
                .unwrap_or_else(|_| ("<crash>".into(), "".into()));
            if n.has_store() {
                let _ = n.snapshot(); // checkpoint on graceful exit
            }
            println!(
                "[{}] FINAL state={} cid={} events={}",
                name, st.0, st.1, n.event_count()
            );
        }
        None => {
            interactive(&node, &peers_handle, &name);
        }
    }
}

fn interactive(node: &net::NodeHandle, peers: &net::Peers, name: &str) {
    println!("[{}] interactive: incr | add N | reset | get | put K V | del K | clear | state | at K | snapshot | peers | log | quit", name);
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        let line = line.trim().to_string();
        if line.is_empty() { continue; }
        if line == "quit" || line == "exit" {
            let mut n = node.lock().unwrap();
            if n.has_store() { let _ = n.snapshot(); }
            break;
        }
        if let Some(rest) = line.strip_prefix("at ") {
            // time-travel: state as of the first K events in total order
            if let Ok(k) = rest.trim().parse::<usize>() {
                let n = node.lock().unwrap();
                match n.state_at(k) {
                    Ok(s) => println!("state@{} = {}", k, net::show_state(&s)),
                    Err(e) => println!("crash: {:?}", e),
                }
            } else {
                println!("usage: at <event-index>");
            }
            continue;
        }
        match line.as_str() {
            "state" => {
                let n = node.lock().unwrap();
                match n.state() {
                    Ok(s) => println!("state = {}  cid={}  events={}", net::show_state(&s), short(&s.cid_hex()), n.event_count()),
                    Err(e) => println!("crash: {:?}", e),
                }
            }
            "snapshot" => {
                let mut n = node.lock().unwrap();
                if n.has_store() {
                    let _ = n.snapshot();
                    println!("snapshot written ({} events)", n.event_count());
                } else {
                    println!("(no --store configured; nothing to snapshot)");
                }
            }
            "peers" => {
                println!("peers connected: {}", peers.lock().unwrap().len());
            }
            "log" => {
                let n = node.lock().unwrap();
                for (idx, (k, _)) in n.events.iter().enumerate() {
                    println!("  #{} lamport={} node={} hash={}", idx, k.0, k.1, short(&sha3::hex(&k.2)));
                }
            }
            other => match agent::parse_action(other) {
                Some(act) => {
                    net::submit(node, peers, act);
                    let n = node.lock().unwrap();
                    if let Ok(s) = n.state() {
                        println!("ok. state = {}", net::show_state(&s));
                    }
                }
                None => println!("unknown command '{}'", other),
            },
        }
    }
}

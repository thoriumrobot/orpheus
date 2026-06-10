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
mod loom;
mod latte;
mod agent;
mod net;
mod store;
mod sca;
mod facet;
mod serve;
mod mold;
mod check;
mod mocha;
mod plan;
mod jets;
mod numerics;
mod marketdata;
mod viz;
mod gfx;
mod gpu;
mod sentiment;
mod game;
mod rustgen;
mod icomb;
mod repl;

use agent::Agent;
use net::{Config, Node};
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};


fn cmd_net(args: &[String]) {
    let src = args.join(" ");
    if src.is_empty() {
        eprintln!("usage: latte net \"<expr>\"   compile +/*/</if over naturals to an interaction net and reduce it");
        return;
    }
    match icomb::run_str(&src) {
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
            match std::fs::remove_dir_all(&dir) {
                Ok(_) => println!("cleared compiled-program cache at {}", dir.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    println!("cache already empty ({})", dir.display())
                }
                Err(e) => eprintln!("cache: {}", e),
            }
        }
        Some("path") | None => {
            let n = std::fs::read_dir(&dir).map(|d| d.filter(|e| {
                e.as_ref().ok().map(|e| e.file_name().to_string_lossy().starts_with('e')).unwrap_or(false)
            }).count()).unwrap_or(0);
            println!("{}", dir.display());
            println!("{} cached program(s); set ORPHEUS_CACHE to relocate, `latte cache clear` to empty", n);
        }
        Some(other) => eprintln!("cache: unknown subcommand '{}' (try: path | clear)", other),
    }
}

fn cmd_cli() {
    use std::io::Write;
    let libs_owned = latte::all_libs();
    let libs: Vec<&str> = libs_owned.iter().map(|s| s.as_str()).collect();
    println!("Orpheus command line.  Type a Latte expression, or a meta-command:");
    println!("  :type EXPR   infer a type      :rust EXPR   compile to native Rust and run");
    println!("  :libs        list libraries    :help        this help     :q   quit\n");
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
            println!(":type EXPR | :rust EXPR | :libs | :q | otherwise evaluate EXPR");
        } else if t == ":libs" {
            println!("{}", libs_owned.join(" "));
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Starting Orpheus with no command launches the GUI (Hymn + console + editor).
    if args.is_empty() {
        cmd_gui(&[]);
        return;
    }
    let cmd = args.get(0).map(|s| s.as_str()).unwrap_or("gui");
    match cmd {
        "cli" | "console" | "--cli" => cmd_cli(),
        "cache" => cmd_cache(&args[1..]),
        "net" => cmd_net(&args[1..]),
        "node" => cmd_node(&args[1..]),
        "eval" => cmd_eval(&args[1..]),
        "agent" => cmd_agent(),
        "selftest" => cmd_selftest(),
        "bench" => cmd_bench(&args[1..]),
        "sca" => cmd_sca(&args[1..]),
        "evolve" => {
            for w in &args[1..] {
                match sca::evolve(w) {
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
        "ml" => numerics::cmd_ml(&args[1..]),
        "nn" => numerics::cmd_nn(&args[1..]),
        "fin" => numerics::cmd_fin(&args[1..]),
        "trade" | "advisor" => numerics::cmd_trade(&args[1..]),
        "fetch" => numerics::cmd_fetch(),
        "ta" | "indicators" => numerics::cmd_ta(&args[1..]),
        "sentiment" | "news" => numerics::cmd_sentiment(&args[1..]),
        "gfx" | "draw" => numerics::cmd_gfx(&args[1..]),
        "gpu" | "compute" => numerics::cmd_gpu(&args[1..]),
        "chart" => viz::cmd_chart(&args[1..]),
        "jit" => cmd_jit(&args[1..]),
        "game" => game::cmd_game(&args[1..]),
        "rustc" | "build-rust" => cmd_rustc(&args[1..]),
        "icomb" => icomb::cmd_icomb(),
        "serve" => cmd_serve(&args[1..]),
        _ => {
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
  latte repl                         self-hosting Latte environment (REPL)\n  latte cli                          interactive command line (eval · :type · :rust · :libs)\n  latte cache [path|clear]           manage the compiled-program cache (Anvil)\n  latte tensor                       n-dimensional tensor demo (lib/tensor.lat)\n  latte ml [linear|perceptron|kmeans|knn] [--iters N]  train a model in Latte (lib/ml.lat)\n  latte chart [bar|line|scatter] N.. data visualization to SVG (lib/plot.lat)\n  latte nn                           neural network with backprop (lib/nn.lat)\n  latte fin [--vol|--dir] [--iters N]  financial ML on Bitcoin: volatility-regime edge (lib/fin.lat)\n  latte gfx                          graphics: draw a scene to SVG (lib/gfx.lat)\n  latte gpu [--dim N]                data-parallel compute, auto-detects GPU (lib/gpu.lat)\n  latte trade [--account N] [--kelly F] [--sentiment S]  automatic trading advisor (sizing)\n  latte sentiment \"<text>\"          Loughran-McDonald news sentiment score\n  latte jit \"<expr>\"               run on the JIT vs the interpreter (compare + time)\n  latte game chess [--max N] [--show K]  run a chess match between two machines\n  latte rustc \"<expr>\" [-o f.rs] [--run]  compile a Latte expression to native Rust (Anvil)\n  latte icomb                        interaction-combinator reduction (Lafont γ/δ/ε)\n  latte net \\\"<expr>\\\"               compile +/*/</if to an interaction net and reduce\n  latte gui [--listen ADDR] [--store D] [--chess-listen ADDR] [--peer ADDR]  GUI: System, editor, charts, chess (--peer links machines)\n  latte serve [--listen ADDR] [--root DIR]   run Hymn, hosting Facet pages (default lib/site)"
            );
        }
    }
}


fn cmd_gui(args: &[String]) {
    let mut listen = "127.0.0.1:8088".to_string();
    let mut root = "lib/site".to_string();
    let mut store: Option<String> = None;
    let mut open: Option<bool> = None; // None = auto (open iff a desktop session is present)
    let mut chess_listen = String::new();
    let mut chess_peers: Vec<String> = Vec::new();
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
            other => { eprintln!("gui: unknown arg {}", other); return; }
        }
        i += 1;
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
    });
    let peers_handle = net::start(node.clone(), cfg);
    Some(serve::Chess::new(node, peers_handle, q))
}

/// True if we appear to be inside a graphical desktop session (X11 or Wayland).
fn desktop_session_present() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
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
    eprintln!("(no browser found to open {} — open it manually)", url);
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
        for word in &args[2..] {
            match sca::run_sca(word, &[src.clone()]) {
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
    match sca::run_sca(word, &rules) {
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

fn cmd_eval(args: &[String]) {
    let mut libs: Vec<String> = latte::all_libs();
    let mut rest: Vec<String> = Vec::new();
    let mut force_interp = false;
    let mut force_net = false;
    let mut force_rebuild = false;
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
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    let src = rest.join(" ");
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    // `--net`: evaluate on the interaction-net engine (the numeric fragment), audited.
    if force_net {
        match icomb::run_str(&src) {
            Ok((v, steps)) => println!("{}  ({} interaction steps)", v, steps),
            Err(e) => eprintln!("net error: {}", e),
        }
        return;
    }
    // The optimizing compiler (Anvil) is the default engine: compile to native Rust, build (with a
    // per-expression binary cache so repeats are instant), and run. Fall back to the interpreter
    // when compilation isn't possible (rustc unavailable, an unsupported construct, or a runtime
    // domain error) — never silently wrong, since on success the two agree exactly.
    if !force_interp {
        if let Some(out) = eval_native(&src, &refs, force_rebuild) {
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
    let cfg = Arc::new(Config { name: name.clone(), listen: listen.clone(), peers: peers.clone(), verbose, compact_every });
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

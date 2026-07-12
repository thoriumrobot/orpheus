//! cliutil — small usability helpers shared across the CLI surface.
//!
//! None of this changes what Orpheus DOES; it changes how discoverable and forgiving the
//! command line is. Three things every mature CLI has and this one lacked:
//!
//!   * a version string (`latte --version`), sourced from Cargo so it never drifts;
//!   * "did you mean" suggestions when a subcommand is mistyped, by Levenshtein distance
//!     over the real command table (a typo should cost one keystroke to fix, not a scan of
//!     a fifty-line usage dump);
//!   * `latte status` (a.k.a. `doctor`) — one screen that answers "what is this instance
//!     doing right now": version, engine (native vs interpret-only and why), cache, the
//!     news wire, the secure transport, workers, and the storage roots. A system with this
//!     much state kept behind env vars needs one place that reports it.

/// The version string, from Cargo at compile time.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A one-line banner: name, version, and the engine mode, for `--version` and status.
pub fn version_line() -> String {
    format!(
        "Orpheus (latte) {} — {}",
        version(),
        if crate::rustgen::rustc_available() {
            "native compiler available (Anvil compiles hot code)"
        } else {
            "interpreter + JIT (no rustc; the interpreter is the engine)"
        }
    )
}

/// Levenshtein edit distance (iterative, two-row). Small inputs; allocation is fine.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The closest known commands to `input`, nearest first, within a small distance budget
/// (scaled to the word length so short commands don't match everything). Also treats a
/// known command that `input` is a prefix of as a strong candidate ("sen" -> "sentiment").
pub fn suggest<'a>(input: &str, known: &[&'a str]) -> Vec<&'a str> {
    let budget = match input.len() {
        0..=2 => 1,
        3..=5 => 2,
        _ => 3,
    };
    let mut scored: Vec<(usize, &str)> = known
        .iter()
        .map(|&k| {
            let d = edit_distance(input, k);
            // a prefix match is worth a near-zero score so it ranks first
            let d = if k.starts_with(input) && !input.is_empty() { 0 } else { d };
            (d, k)
        })
        .filter(|(d, _)| *d <= budget)
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(3).map(|(_, k)| k).collect()
}

/// Every top-level command word the dispatcher accepts, for suggestions and `help`.
pub const COMMANDS: &[&str] = &[
    "gui", "start", "serve", "android", "cli", "console", "repl", "eval", "jit", "debug",
    "fmt", "typecheck", "cache", "profile", "bench", "selftest", "agent", "node", "net",
    "worker", "workers", "anvil", "rustc", "icomb", "sca", "evolve", "mold", "mocha",
    "plan", "team", "tensor", "ddia", "algo", "dsa", "wgraph", "numth", "bits", "strings",
    "grid", "design", "trees", "dp", "intervals", "search", "graphs", "backtrack", "greedy",
    "db", "ml", "nn", "fin", "trade", "advisor", "money", "bonds", "lab", "fetch", "ta",
    "indicators", "sentiment", "news", "wire", "gfx", "draw", "gpu", "compute", "chart",
    "game", "pkg", "packages", "trace", "raytrace", "status", "doctor", "version", "help",
];

/// Render `latte status` / `latte doctor`: the one-screen environment report.
pub fn status_report() -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "{}", version_line());
    let _ = writeln!(s);

    // --- engine -------------------------------------------------------------
    let has_rustc = crate::rustgen::rustc_available();
    let _ = writeln!(s, "engine");
    if has_rustc {
        let _ = writeln!(s, "  mode        : adaptive — hot code compiles to native via Anvil, the rest interprets");
    } else {
        let _ = writeln!(s, "  mode        : interpret-only (no rustc on PATH) — the interpreter + JIT is the engine");
    }
    let fuel = crate::loom::default_fuel();
    let fuel_str = if fuel == u64::MAX {
        "unlimited (ORPHEUS_FUEL=0)".to_string()
    } else {
        format!("{} steps", fuel)
    };
    let _ = writeln!(s, "  step budget : {}", fuel_str);

    // --- cache --------------------------------------------------------------
    let (n, bytes) = crate::rustgen::cache_stats();
    let _ = writeln!(s, "\nnative cache");
    let _ = writeln!(s, "  location    : {}", crate::rustgen::cache_dir().display());
    let _ = writeln!(s, "  contents    : {} program(s), {:.1} MiB", n, bytes as f64 / (1u64 << 20) as f64);

    // --- storage roots ------------------------------------------------------
    let _ = writeln!(s, "\nstorage");
    let home = std::env::var("HOME").unwrap_or_else(|_| "<unset>".into());
    let _ = writeln!(s, "  HOME        : {}", home);
    let _ = writeln!(s, "  news cache  : {}", crate::newswire::news_cache_dir().display());
    let _ = writeln!(s, "  db dir      : {}", std::env::var("ORPHEUS_DB_DIR").unwrap_or_else(|_| "./dbdata".into()));

    // --- the news wire ------------------------------------------------------
    let _ = writeln!(s, "\nnews wire");
    match crate::newswire::wire_age() {
        Some(a) => {
            let items = crate::newswire::load_wire().len();
            let _ = writeln!(s, "  store       : {} item(s), last fetch {} min ago", items, a / 60);
        }
        None => {
            let _ = writeln!(s, "  store       : empty (run `latte news fetch`)");
        }
    }
    let auto = std::env::var("ORPHEUS_NEWS_AUTO").map(|v| v != "0").unwrap_or(true);
    let _ = writeln!(s, "  auto-fetch  : {}", if auto { "on (30-min TTL)" } else { "off (ORPHEUS_NEWS_AUTO=0)" });

    // --- security -----------------------------------------------------------
    let _ = writeln!(s, "\nsecurity");
    let psk = crate::secure::configured_psk(None).is_some();
    let _ = writeln!(s, "  gossip PSK  : {}", if psk {
        "configured — peers are mutually authenticated and encrypted"
    } else {
        "not set — gossip is plaintext (fine on loopback; set ORPHEUS_PSK for the Internet)"
    });
    let tok = std::env::var("ORPHEUS_TOKEN").is_ok() || psk;
    let _ = writeln!(s, "  web token   : {}", if tok {
        "required (a public GUI bind is gated)"
    } else {
        "none (a public GUI bind will be refused; loopback is open)"
    });

    // --- distribution -------------------------------------------------------
    let _ = writeln!(s, "\ndistribution");
    let workers = crate::dist::workers();
    if workers.is_empty() {
        let _ = writeln!(s, "  workers     : none registered (`latte workers add ADDR` to distribute)");
    } else {
        let _ = writeln!(s, "  workers     : {} registered — distribution on by default", workers.len());
        for w in workers.iter().take(6) {
            let alive = if crate::dist::worker_alive(w) { "alive" } else { "unreachable" };
            let _ = writeln!(s, "                {} ({})", w, alive);
        }
    }

    let _ = writeln!(s, "\n(`latte help` lists commands; docs/environment.md documents every variable)");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("news", ""), 4);
    }

    #[test]
    fn suggest_finds_the_obvious_typo() {
        assert_eq!(suggest("newz", COMMANDS)[0], "news"); // nearest ranks first
        assert_eq!(suggest("tarde", COMMANDS)[0], "trade");
        assert_eq!(suggest("sentimnet", COMMANDS)[0], "sentiment");
    }

    #[test]
    fn suggest_uses_prefix_for_short_stubs() {
        let s = suggest("sen", COMMANDS);
        assert!(s.contains(&"sentiment"), "prefix should surface the full command: {:?}", s);
    }

    #[test]
    fn suggest_returns_nothing_for_gibberish() {
        assert!(suggest("zzzzzqqqqq", COMMANDS).is_empty());
    }

    #[test]
    fn version_is_nonempty() {
        assert!(!version().is_empty());
        assert!(version_line().contains("Orpheus"));
    }
}

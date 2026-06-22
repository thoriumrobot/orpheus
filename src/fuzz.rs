//! A differential fuzzer for the Anvil native backend.
//!
//! The safety argument for compiling Latte to native Rust is that, *on success, the native binary
//! agrees with the interpreter exactly*. This module stress-tests that claim: it generates random
//! well-formed, terminating Latte expressions drawn from the native subset and checks the two
//! engines against each other. The property asserted is **soundness** — if the native binary
//! produces a value, the interpreter produces the same value (a confidently-wrong native result is
//! the dangerous failure). Native *declining* (returning `None`, e.g. u128-overflow arithmetic that
//! legitimately falls back) is allowed and merely counted, so the check never false-flags a
//! designed fallback.
//!
//! Generation is seeded (SplitMix64), so any divergence is reproducible from its seed. Each case
//! compiles a fresh native binary, so the always-on test runs a modest count; `latte anvil fuzz
//! <iters> <seed>` runs an extensive on-demand campaign (a release gate).

use crate::{latte, rustgen};

/// SplitMix64 — a tiny, dependency-free, deterministic PRNG.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

struct Gen {
    rng: Rng,
    ctr: u32,
}

impl Gen {
    fn fresh(&mut self) -> String {
        self.ctr += 1;
        format!("v{}", self.ctr)
    }

    /// An atom-valued leaf: a small literal, an atom-typed variable, or a cord (cords are atoms).
    fn atom_leaf(&mut self, av: &[String]) -> String {
        match self.rng.below(3) {
            0 => format!("{}", self.rng.below(16)),
            1 if !av.is_empty() => av[self.rng.below(av.len())].clone(),
            _ => {
                let words = ["foo", "bar", "heart", "x", "a_long_cord_well_over_sixteen_bytes"];
                format!("\"{}\"", words[self.rng.below(words.len())])
            }
        }
    }

    /// A condition (loobean: 0 = true), built from atom comparisons so it never crashes.
    fn cond(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, d: usize) -> String {
        let a = self.atom(av, nv, d);
        let b = self.atom(av, nv, d);
        match self.rng.below(2) {
            0 => format!("({} == {})", a, b),
            _ => format!("(lt {} {})", a, b),
        }
    }

    /// An **atom-valued** expression that is total (never hits a domain error): subtraction can't
    /// underflow (`(sub (add x t) t) = x ≥ 0`), divisors are made `≥ 1` via `+()`, and arithmetic
    /// operands are always atoms — so the only way it fails to run natively is genuine u128
    /// overflow, which is a legitimate fallback.
    fn atom(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, depth: usize) -> String {
        if depth == 0 {
            return self.atom_leaf(av);
        }
        let d = depth - 1;
        match self.rng.below(18) {
            0 => format!("(add {} {})", self.atom(av, nv, d), self.atom(av, nv, d)),
            1 => format!("(mul {} {})", self.atom(av, nv, d), self.atom(av, nv, d)),
            2 => format!("(dec {})", self.atom(av, nv, d)),
            3 => format!("+({})", self.atom(av, nv, d)),
            4 => format!("(lt {} {})", self.atom(av, nv, d), self.atom(av, nv, d)),
            5 => {
                let t = self.atom(av, nv, d);
                format!("(sub (add {} {}) {})", self.atom(av, nv, d), t, t) // = first atom, ≥ 0
            }
            6 => format!("(mod {} +({}))", self.atom(av, nv, d), self.atom(av, nv, d)), // divisor ≥ 1
            7 => format!("(div {} +({}))", self.atom(av, nv, d), self.atom(av, nv, d)),
            8 => {
                let c = self.cond(av, nv, d);
                format!("if {} then {} else {}", c, self.atom(av, nv, d), self.atom(av, nv, d))
            }
            9 => {
                let x = self.fresh();
                let v = self.atom(av, nv, d);
                av.push(x.clone());
                let b = self.atom(av, nv, d);
                av.pop();
                format!("let {} = {} in {}", x, v, b)
            }
            10 => {
                let s = self.any(av, nv, d);
                let (a1, a2, def) = (self.atom(av, nv, d), self.atom(av, nv, d), self.atom(av, nv, d));
                format!(
                    "case {} of %short -> {} ; %a_descriptive_tag_over_sixteen_bytes -> {} ; _ -> {} end",
                    s, a1, a2, def
                )
            }
            11 => {
                let i = self.fresh();
                let acc = self.fresh();
                let iters = 1 + self.rng.below(6);
                let init = self.atom(av, nv, d);
                av.push(i.clone());
                av.push(acc.clone());
                let upd = self.atom(av, nv, d);
                av.pop();
                av.pop();
                format!(
                    "loop with [{i} = {n}, {acc} = {init}] : if ({i} == 0) then {acc} else again((dec {i}), {upd}) end",
                    i = i, n = iters, acc = acc, init = init, upd = upd
                )
            }
            // --- library calls: comparisons, list folds, and higher-order functions ---
            12 => {
                let f = ["gte", "gt", "lte"][self.rng.below(3)];
                format!("({} {} {})", f, self.atom(av, nv, d), self.atom(av, nv, d))
            }
            13 => format!("(len {})", self.list(av, nv, d)),
            14 => format!("(sum {})", self.list(av, nv, d)),
            15 => {
                let f = ["member", "elem"][self.rng.below(2)];
                format!("({} {} {})", f, self.atom(av, nv, d), self.list(av, nv, d))
            }
            16 => {
                // fold a list with a binary closure (exercises gate emission + capture)
                let f = ["foldl", "foldr"][self.rng.below(2)];
                let g = self.gate2(av, nv, d);
                format!("({} {} {} {})", f, g, self.atom(av, nv, d), self.list(av, nv, d))
            }
            _ => {
                // any/all: a predicate closure over a list
                let f = ["any", "all"][self.rng.below(2)];
                let g = self.gate1(av, nv, d);
                format!("({} {} {})", f, g, self.list(av, nv, d))
            }
        }
    }

    /// A **list-valued** expression (proper list ending in 0, elements atoms).
    fn list(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, depth: usize) -> String {
        if depth == 0 {
            return match self.rng.below(2) {
                0 => "0".to_string(),                                  // empty list
                _ => format!("(range (mod {} 8))", self.atom_leaf(av)), // 0..n, length ≤ 7
            };
        }
        let d = depth - 1;
        match self.rng.below(7) {
            0 => "0".to_string(),
            1 => format!("[{} {}]", self.atom(av, nv, d), self.list(av, nv, d)), // cons
            2 => format!("(range (mod {} 8))", self.atom(av, nv, d)),
            3 => format!("(reverse {})", self.list(av, nv, d)),
            4 => format!("(append {} {})", self.list(av, nv, d), self.list(av, nv, d)),
            5 => {
                let f = ["take", "drop"][self.rng.below(2)];
                format!("({} {} {})", f, self.atom(av, nv, d), self.list(av, nv, d))
            }
            _ => {
                // map / filter with a closure (exercises gate emission + capture)
                let f = ["map", "filter"][self.rng.below(2)];
                let g = self.gate1(av, nv, d);
                format!("({} {} {})", f, g, self.list(av, nv, d))
            }
        }
    }

    /// A unary closure `(fn [x] -> <atom>)`. The body may reference outer atom variables, so this
    /// exercises free-variable capture in native closures — not just the parameter.
    fn gate1(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, depth: usize) -> String {
        let x = self.fresh();
        av.push(x.clone());
        let body = self.atom(av, nv, depth);
        av.pop();
        format!("(fn [{}] -> {})", x, body)
    }

    /// A binary closure `(fn [acc v] -> <atom>)` for folds, again allowing capture.
    fn gate2(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, depth: usize) -> String {
        let acc = self.fresh();
        let v = self.fresh();
        av.push(acc.clone());
        av.push(v.clone());
        let body = self.atom(av, nv, depth);
        av.pop();
        av.pop();
        format!("(fn [{} {}] -> {})", acc, v, body)
    }

    /// An **any-valued** expression — may be a cell — exercising cell construction and head/tail
    /// (applied only to freshly built cells, so they never crash).
    fn any(&mut self, av: &mut Vec<String>, nv: &mut Vec<String>, depth: usize) -> String {
        if depth == 0 {
            return self.atom_leaf(av);
        }
        let d = depth - 1;
        match self.rng.below(7) {
            0 => format!("[{} {}]", self.any(av, nv, d), self.any(av, nv, d)),
            1 => format!("(head [{} {}])", self.any(av, nv, d), self.any(av, nv, d)),
            2 => format!("(tail [{} {}])", self.any(av, nv, d), self.any(av, nv, d)),
            3 => {
                let c = self.cond(av, nv, d);
                format!("if {} then {} else {}", c, self.any(av, nv, d), self.any(av, nv, d))
            }
            4 => {
                let x = self.fresh();
                let v = self.any(av, nv, d);
                nv.push(x.clone());
                let b = self.any(av, nv, d);
                nv.pop();
                format!("let {} = {} in {}", x, v, b)
            }
            5 => self.list(av, nv, d),
            _ => self.atom(av, nv, d),
        }
    }
}

/// Build one random closed program: bind `a`,`b`,`c` to small naturals, then a generated body
/// (any-valued, so cells and head/tail are exercised at the top too).
pub fn random_program(seed: u64) -> String {
    let mut g = Gen { rng: Rng::new(seed), ctr: 0 };
    let (a, b, c) = (g.rng.below(16), g.rng.below(16), g.rng.below(16));
    let mut av = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let mut nv: Vec<String> = Vec::new();
    let body = g.any(&mut av, &mut nv, 4);
    format!("let a = {} in let b = {} in let c = {} in {}", a, b, c, body)
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub agreed: u64,   // native produced a value and the interpreter matched
    pub declined: u64, // native declined (None) — a legitimate fallback
    pub skipped: u64,  // out of the native subset, or interpreter itself errored
}

/// Check one program for native/interpreter soundness. `Err` means a genuine divergence (native
/// confidently disagreed with the interpreter) — the serious failure the fuzzer hunts for.
pub fn check_one(src: &str) -> Result<Stats, String> {
    let libs = latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    let mut st = Stats::default();
    // Only meaningful for programs in the native subset.
    if rustgen::compile_to_rust(src, &refs).is_err() {
        st.skipped += 1;
        return Ok(st);
    }
    match rustgen::run_native_noun(src, &refs) {
        Some(nat) => match latte::run_with_libs(src, &refs) {
            Ok(interp) => {
                let (cn, ci) = (rustgen::noun_to_canon(&nat), rustgen::noun_to_canon(&interp));
                if cn == ci {
                    st.agreed += 1;
                } else {
                    return Err(format!(
                        "VALUE DIVERGENCE\n  expr   = {}\n  native = {}\n  interp = {}",
                        src, cn, ci
                    ));
                }
            }
            Err(e) => {
                return Err(format!(
                    "SUCCESS DIVERGENCE: native produced {} but interpreter errored ({})\n  expr = {}",
                    rustgen::noun_to_canon(&nat),
                    e,
                    src
                ));
            }
        },
        None => st.declined += 1, // legitimate fallback (e.g. overflow) — not a soundness failure
    }
    Ok(st)
}

/// Run `iters` random cases from `seed`. Stops and returns `Err` on the first divergence (with the
/// reproducing program); otherwise returns aggregate `Stats`.
/// All matched `()`/`[]` delimiter spans (byte ranges, inclusive) in `s`. Generated programs are
/// well-formed ASCII, so a simple stack scan suffices.
fn matched_pairs(s: &str) -> Vec<(usize, usize)> {
    let mut stack = Vec::new();
    let mut out = Vec::new();
    for (i, &c) in s.as_bytes().iter().enumerate() {
        match c {
            b'(' | b'[' => stack.push(i),
            b')' | b']' => {
                if let Some(o) = stack.pop() {
                    out.push((o, i));
                }
            }
            _ => {}
        }
    }
    out
}

/// Greedily minimize `src` while `diverges` still holds, trying two structural reductions on each
/// balanced subterm: collapse it to `0`, or hoist it to be the whole program. Every candidate is
/// valid Latte; a reduction that no longer reproduces (or breaks scope/parse) is simply rejected,
/// so the result is always a genuine, smaller reproducer. `budget` caps predicate evaluations
/// (each is a native build), so a real divergence shrinks in bounded time. Predicate-generic, so
/// it is unit-testable without compiling anything.
pub fn shrink_with(src: &str, diverges: &mut dyn FnMut(&str) -> bool, budget: u32) -> String {
    let mut cur = src.to_string();
    let mut spent = 0u32;
    'outer: loop {
        for (s, e) in matched_pairs(&cur) {
            let span = cur[s..=e].to_string();
            // (a) collapse this subterm to `0`
            if span != "0" {
                let cand = format!("{}0{}", &cur[..s], &cur[e + 1..]);
                if cand.len() < cur.len() {
                    if spent >= budget {
                        return cur;
                    }
                    spent += 1;
                    if diverges(&cand) {
                        cur = cand;
                        continue 'outer;
                    }
                }
            }
            // (b) hoist this subterm to be the whole program
            if span.len() < cur.len() {
                if spent >= budget {
                    return cur;
                }
                spent += 1;
                if diverges(&span) {
                    cur = span;
                    continue 'outer;
                }
            }
        }
        return cur; // no reduction helped — a local minimum
    }
}

/// True when the native backend doesn't *fully* agree with the interpreter on `src`: a value
/// divergence, native succeeding where the interpreter errors (both bugs), or native declining
/// where the interpreter succeeds (a legitimate fallback). Used by `latte anvil shrink` to minimize
/// a program to the smallest subterm that isn't purely native — a bug reproducer or a "what here
/// isn't native?" probe.
pub fn not_fully_native(src: &str) -> bool {
    let libs = latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    match (rustgen::run_native_noun(src, &refs), latte::run_with_libs(src, &refs)) {
        (Some(n), Ok(v)) => rustgen::noun_to_canon(&n) != rustgen::noun_to_canon(&v),
        (Some(_), Err(_)) => true, // native produced a value the interpreter rejects
        (None, Ok(_)) => true,     // native declined but the interpreter succeeded (fallback)
        (None, Err(_)) => false,   // both failed the same way
    }
}

pub fn run(iters: u64, seed: u64) -> Result<Stats, String> {
    let mut total = Stats::default();
    for k in 0..iters {
        // Derive a distinct, reproducible per-case seed.
        let mut r = Rng::new(seed.wrapping_add(k));
        let case_seed = r.next();
        let src = random_program(case_seed);
        match check_one(&src) {
            Ok(s) => {
                total.agreed += s.agreed;
                total.declined += s.declined;
                total.skipped += s.skipped;
            }
            Err(_) => {
                // Found a divergence — minimize it before reporting.
                let mut pred = |c: &str| matches!(check_one(c), Err(_));
                let minimal = shrink_with(&src, &mut pred, 200);
                let detail = check_one(&minimal).err().unwrap_or_else(|| "<divergence>".into());
                return Err(format!(
                    "seed={} case={}: minimized to {} chars (from {})\n  reproducer: {}\n{}",
                    seed, k, minimal.len(), src.len(), minimal, detail
                ));
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinker_minimizes_and_preserves_predicate() {
        // A predicate independent of native builds: "still mentions the marker 7". Shrinking must
        // collapse everything else while keeping a 7, and must never enlarge or drop the marker.
        let src = "let a = 1 in (add (mul 7 3) (foldl (fn [acc v] -> (sub acc v)) 0 [2 [4 0]]))";
        let mut keep7 = |s: &str| s.contains('7');
        let min = super::shrink_with(src, &mut keep7, 100_000);
        assert!(min.contains('7'), "predicate preserved");
        assert!(min.len() < src.len() / 2, "substantially smaller: {} -> {}", src.len(), min.len());
        // If no candidate satisfies the predicate, the input is returned unchanged.
        let mut never = |_: &str| false;
        assert_eq!(super::shrink_with(src, &mut never, 100_000), src);
        // The minimized form still satisfies the predicate when re-checked.
        assert!(keep7(&min));
    }

    #[test]
    fn generated_programs_parse() {
        // The generator must emit syntactically valid Latte (so the differential check is meaningful).
        for k in 0..50u64 {
            let src = random_program(0x1234 + k);
            assert!(latte::parse(&src).is_ok(), "generated invalid Latte:\n{}", src);
        }
    }

    #[test]
    fn native_matches_interpreter_fuzz() {
        // Modest, seeded, reproducible — each case builds a native binary, so keep the count small
        // in the always-on suite. Extensive runs go through `latte anvil fuzz`.
        let st = run(16, 0xC0FFEE).expect("native and interpreter must agree");
        assert!(
            st.agreed >= 1,
            "fuzzer should have compared at least one agreeing native run: {:?}",
            st
        );
    }
}

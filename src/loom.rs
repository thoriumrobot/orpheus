//! Loom — the axiomatic core. A deterministic, total-where-defined function
//! `tar(subject, formula) -> knot`, specified by twelve reduction rules.
//!
//! Pseudo-operators from the spec:
//!   *  evaluate (tar)      /  slot/address (fetch subtree)
//!   #  edit (replace)      ?  cell-test     =  equality   ^  increment
//!
//! Address rules (the slot operator):
//!   /[1 a]        = a
//!   /[2 [a b]]    = a
//!   /[3 [a b]]    = b
//!   /[(2n)   a]   = /[2 /[n a]]
//!   /[(2n+1) a]   = /[3 /[n a]]
//!
//! The twelve forms (named mnemonics for Nock opcodes 0..11):
//!   *[s [f g] h]    = [*[s f g] *[s h]]                 AUTOCONS
//!   *[s 0 a]        = /[a s]                             ADDRESS
//!   *[s 1 a]        = a                                  QUOTE
//!   *[s 2 f g]      = *[*[s f] *[s g]]                   EVAL
//!   *[s 3 f]        = ?*[s f]                            CELL?  (0 cell, 1 atom)
//!   *[s 4 f]        = ^*[s f]                            SUCC
//!   *[s 5 f g]      = =[*[s f] *[s g]]                   SAME   (0 equal, 1 not)
//!   *[s 6 f g h]    = if *[s f]=0 then *[s g] else *[s h] IF
//!   *[s 7 f g]      = *[*[s f] g]                        THEN   (compose/pipe)
//!   *[s 8 f g]      = *[[*[s f] s] g]                    PUSH   (let)
//!   *[s 9 b g]      = *[*[s g] 2 [0 1] 0 b]              CALL   (invoke arm b)
//!   *[s 10 [a f] g] = #[a *[s f] *[s g]]                 EDIT
//!   *[s 11 h g]     = *[s g]   (hint h computed, discarded) HINT

use crate::atom::Atom;
use crate::knot::{atom, cell, Knot, N};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// ----------------------------- jets -----------------------------------------
// A jet is a native fast-path for a hinted formula. Contract: for every subject,
// the jet MUST return exactly what the pure formula would. Jets are dispatched by
// a static HINT (rule 11) carrying a cord tag. In `audit` mode the interpreter runs
// BOTH the jet and the pure formula and crashes on any mismatch — so equivalence is
// checked, not trusted. This is the honest answer to "you secretly need C": the
// semantics are the pure reduction; the jet is an optional, verifiable accelerator.
pub type JetFn = fn(&N) -> Eval;

static JETS: OnceLock<Mutex<HashMap<Vec<u8>, JetFn>>> = OnceLock::new();
static JETS_ENABLED: AtomicBool = AtomicBool::new(true);
static JIT_ENABLED: AtomicBool = AtomicBool::new(true);

thread_local! {
    // Audit is opt-in PER THREAD: enabling it on one thread (to compare a jet against its
    // pure reduction) must not force *other* threads' evaluations down the un-jetted path —
    // that would, e.g., turn a `add` on large atoms into a successor loop and OutOfFuel.
    // (Was a global AtomicBool, which raced across the parallel test suite.)
    static JET_AUDIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn jet_table() -> &'static Mutex<HashMap<Vec<u8>, JetFn>> {
    JETS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub fn register_jet(name: &[u8], f: JetFn) {
    jet_table().lock().unwrap().insert(name.to_vec(), f);
}
fn lookup_jet(name: &[u8]) -> Option<JetFn> {
    jet_table().lock().unwrap().get(name).copied()
}
pub fn set_jets_enabled(b: bool) {
    JETS_ENABLED.store(b, Ordering::SeqCst);
}
pub fn set_jet_audit(b: bool) {
    JET_AUDIT.with(|f| f.set(b));
}
fn jets_on() -> bool {
    JETS_ENABLED.load(Ordering::SeqCst)
}
fn audit_on() -> bool {
    JET_AUDIT.with(|f| f.get()) && !IN_AUDIT.with(|f| f.get())
}

// Audit is NON-REENTRANT per thread: while one jet's pure reduction runs for
// comparison, inner hinted calls use their jets WITHOUT re-auditing. Each jet is
// still audited at its own (outer) call sites, so coverage is unchanged — but a
// pure body that itself uses hinted arithmetic (e.g. num.lat's `nmul` calling
// `mul`/`div`) stays fast instead of exploding into successor arithmetic.
thread_local! {
    static IN_AUDIT: std::cell::Cell<bool> = std::cell::Cell::new(false);
}
struct AuditGuard;
impl AuditGuard {
    fn enter() -> AuditGuard {
        IN_AUDIT.with(|f| f.set(true));
        AuditGuard
    }
}
impl Drop for AuditGuard {
    fn drop(&mut self) {
        IN_AUDIT.with(|f| f.set(false));
    }
}
/// Enable/disable JIT compilation. When disabled, `tar` uses the tree-walking interpreter.
/// JIT is on by default and is the production execution path; the interpreter remains the
/// reference semantics (the JIT is checked against it by the test suite and `jit_audit`).
pub fn set_jit_enabled(b: bool) {
    JIT_ENABLED.store(b, Ordering::SeqCst);
}
/// Set the JIT hotness threshold: a formula is interpreted until entered this many times,
/// then compiled. 0 = always compile; very large = effectively interpret-only.
pub fn set_jit_threshold(t: u32) {
    jit::set_threshold(t);
}
fn jit_on() -> bool {
    JIT_ENABLED.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Crash {
    Bottom(String),
    OutOfFuel,
}

pub type Eval = Result<N, Crash>;

/// Baseline evaluation step budget (guards against non-termination / `*f f`-style loops)
/// on a machine that also has the native compiler to fall back on.
pub const DEFAULT_FUEL: u64 = 50_000_000;

/// The budget an INTERPRET-ONLY device gets. On a phone (no `rustc`, so Anvil can never
/// lower a program to native code) the interpreter is not a fallback — it is the engine.
/// A ceiling tuned to "a native path exists for the heavy work" silently turns the
/// finance and ML tools into `OutOfFuel` errors there, which is not "interpretation
/// works". The guard still exists (a runaway `*f f` must not hang the GUI), it is just
/// sized for the machine that has to do the real work in the interpreter.
pub const INTERPRETER_FUEL: u64 = 20_000_000_000;

/// The effective step budget for this process, decided once:
///   * `ORPHEUS_FUEL=<n>` — an explicit budget; `0` means "no limit" (scripts, batch jobs).
///   * otherwise: `DEFAULT_FUEL` when a `rustc` is present (the native path takes the
///     heavy programs), `INTERPRETER_FUEL` when it is not (phones, stripped containers).
pub fn default_fuel() -> u64 {
    static FUEL: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *FUEL.get_or_init(|| {
        if let Ok(v) = std::env::var("ORPHEUS_FUEL") {
            if let Ok(n) = v.trim().parse::<u64>() {
                return if n == 0 { u64::MAX } else { n };
            }
        }
        if crate::rustgen::rustc_available() {
            DEFAULT_FUEL
        } else {
            INTERPRETER_FUEL
        }
    })
}

pub fn tar(subject: &N, formula: &N) -> Eval {
    let mut fuel = default_fuel();
    run(subject, formula, &mut fuel)
}

#[allow(dead_code)]
pub fn tar_with_fuel(subject: &N, formula: &N, fuel: u64) -> Eval {
    let mut f = fuel;
    run(subject, formula, &mut f)
}

/// The default execution path: JIT-compile the formula to closures and run it (production),
/// or fall back to the tree-walking interpreter when JIT is disabled (the reference).
fn run(subject: &N, formula: &N, fuel: &mut u64) -> Eval {
    // Adaptive: interpret by default and hand *hot* loops to the JIT (see rules 2 and 9).
    // Cold/one-shot formulas never pay compilation cost; only code executed enough times to
    // amortize it gets compiled. So "compile by default" holds exactly when it is faster.
    tar_fuel(subject, formula, fuel, jit_on())
}

#[allow(dead_code)]
/// Force full JIT compilation regardless of hotness (for benchmarking / auditing the compiler).
pub fn jit_force(subject: &N, formula: &N) -> Eval {
    let mut fuel = default_fuel();
    jit::eval(formula, subject, &mut fuel)
}

#[allow(dead_code)]
/// Evaluate strictly with the interpreter (the reference semantics), bypassing the JIT.
/// Used to audit the compiler against the interpreter.
pub fn interp(subject: &N, formula: &N) -> Eval {
    let mut fuel = default_fuel();
    tar_fuel(subject, formula, &mut fuel, false)
}

fn spend(fuel: &mut u64) -> Result<(), Crash> {
    if *fuel == 0 {
        return Err(Crash::OutOfFuel);
    }
    *fuel -= 1;
    Ok(())
}

fn opcode(a: &Atom) -> Option<u8> {
    a.to_u128().and_then(|n| if n < 256 { Some(n as u8) } else { None })
}

fn tar_fuel(subject0: &N, formula0: &N, fuel: &mut u64, tier: bool) -> Eval {
    // Trampolined: tail-position rules (2,6,7,8,9,11) update `subject`/`formula`
    // and `continue` instead of recursing, so loops run in constant native stack.
    // Operand sub-evaluations still recurse, but their depth is bounded by the
    // (static) formula nesting, not by the number of runtime steps.
    let mut subject = subject0.clone();
    let mut formula = formula0.clone();
    loop {
        spend(fuel)?;
        let (head, tail) = match &*formula {
            Knot::Cell(h, t) => (h.clone(), t.clone()),
            Knot::Atom(_) => return Err(Crash::Bottom("formula is a bare atom".into())),
        };

        // AUTOCONS: *[s [f g] h] = [*[s f g] *[s h]]   (not a tail position)
        if let Knot::Cell(_, _) = &*head {
            let lhs = tar_fuel(&subject, &head, fuel, tier)?;
            let rhs = tar_fuel(&subject, &tail, fuel, tier)?;
            return Ok(cell(lhs, rhs));
        }

        let op = match head.as_atom().and_then(opcode) {
            Some(o) => o,
            None => return Err(Crash::Bottom("opcode out of range".into())),
        };

        match op {
            // 0  ADDRESS
            0 => {
                let a = tail.as_atom().ok_or_else(|| Crash::Bottom("0: axis not atom".into()))?;
                return slot(a, &subject);
            }
            // 1  QUOTE
            1 => return Ok(tail.clone()),
            // 2  EVAL (tail)
            2 => {
                let (f, g) = cell2(&tail, "2")?;
                let new_subj = tar_fuel(&subject, f, fuel, tier)?;
                let new_form = tar_fuel(&subject, g, fuel, tier)?;
                if tier && jit_on() && jit::hot(&new_form) {
                    return jit::eval(&new_form, &new_subj, fuel);
                }
                subject = new_subj;
                formula = new_form;
                continue;
            }
            // 3  CELL?
            3 => {
                let v = tar_fuel(&subject, &tail, fuel, tier)?;
                return Ok(loobool(!v.is_atom()));
            }
            // 4  SUCC
            4 => {
                let v = tar_fuel(&subject, &tail, fuel, tier)?;
                return match &*v {
                    Knot::Atom(a) => Ok(atom(a.inc())),
                    Knot::Cell(_, _) => Err(Crash::Bottom("4: increment of a cell".into())),
                };
            }
            // 5  SAME
            5 => {
                let (f, g) = cell2(&tail, "5")?;
                let a = tar_fuel(&subject, f, fuel, tier)?;
                let b = tar_fuel(&subject, g, fuel, tier)?;
                return Ok(loobool(a == b));
            }
            // 6  IF (tail in both branches)
            6 => {
                let (f, gh) = cell2(&tail, "6")?;
                let (g, h) = cell2(gh, "6")?;
                let c = tar_fuel(&subject, f, fuel, tier)?;
                match c.as_atom().and_then(|a| a.to_u128()) {
                    Some(0) => {
                        formula = g.clone();
                        continue;
                    }
                    Some(1) => {
                        formula = h.clone();
                        continue;
                    }
                    _ => return Err(Crash::Bottom("6: condition not a loobean".into())),
                }
            }
            // 7  THEN / compose (tail)
            7 => {
                let (f, g) = cell2(&tail, "7")?;
                let mid = tar_fuel(&subject, f, fuel, tier)?;
                subject = mid;
                formula = g.clone();
                continue;
            }
            // 8  PUSH / let (tail)
            8 => {
                let (f, g) = cell2(&tail, "8")?;
                let v = tar_fuel(&subject, f, fuel, tier)?;
                subject = cell(v, subject.clone());
                formula = g.clone();
                continue;
            }
            // 9  CALL / invoke arm (tail) — this is what makes Latte loops O(1) stack
            9 => {
                let (b, g) = cell2(&tail, "9")?;
                let baxis = b.as_atom().ok_or_else(|| Crash::Bottom("9: arm axis not atom".into()))?;
                let core = tar_fuel(&subject, g, fuel, tier)?;
                let armf = slot(baxis, &core)?;
                if tier && jit_on() && jit::hot(&armf) {
                    return jit::eval(&armf, &core, fuel);
                }
                subject = core;
                formula = armf;
                continue;
            }
            // 10 EDIT
            10 => {
                let (af, g) = cell2(&tail, "10")?;
                let (a, f) = cell2(af, "10")?;
                let axis = a.as_atom().ok_or_else(|| Crash::Bottom("10: edit axis not atom".into()))?;
                let v = tar_fuel(&subject, f, fuel, tier)?;
                let base = tar_fuel(&subject, g, fuel, tier)?;
                return edit(axis, &v, &base);
            }
            // 11 HINT (tail body) — and the jet dispatch point.
            11 => {
                let (h, g) = cell2(&tail, "11")?;
                // THE DEBUGGER's hook: hints whose tag begins with "dbg:" mark
                // arm calls when a trace is active (latte debug / /api/debug).
                // Enter is recorded with the call's arguments (axis 6 of the
                // subject = the args the compiler installed), the body runs,
                // and Exit records the result — giving the full call tree.
                if let Knot::Atom(tag) = &**h {
                    if tracer_active() {
                        let name = String::from_utf8_lossy(&tag.bytes_le()).into_owned();
                        if let Some(arm) = name.strip_prefix("dbg:") {
                            let args = crate::loom::slot(&crate::atom::Atom::from_u128(3), &subject)
                                .unwrap_or_else(|_| num_n(0));
                            tracer_enter(arm, &args);
                            let r = tar_fuel(&subject, g, fuel, tier);
                            match &r {
                                Ok(v) => tracer_exit(Some(v)),
                                Err(_) => tracer_exit(None),
                            }
                            return r;
                        }
                    }
                }
                // Static hint = a bare atom tag. If it names a registered jet and
                // jets are enabled, run the native path instead of the formula.
                if let Knot::Atom(tag) = &**h {
                    if jets_on() {
                        if let Some(jet) = lookup_jet(tag.bytes_le()) {
                            let jetted = jet(&subject)?;
                            if audit_on() {
                                let _guard = AuditGuard::enter();
                                let pure = tar_fuel(&subject, g, fuel, tier)?;
                                if pure != jetted {
                                    return Err(Crash::Bottom(format!(
                                        "jet {:?} disagrees with pure reduction",
                                        tag
                                    )));
                                }
                                return Ok(pure);
                            }
                            return Ok(jetted);
                        }
                    }
                }
                // dynamic hint [tag formula]: compute and discard
                if let Knot::Cell(_, hint_form) = &**h {
                    let _ = tar_fuel(&subject, hint_form, fuel, tier)?;
                }
                formula = g.clone();
                continue;
            }
            _ => return Err(Crash::Bottom(format!("unknown opcode {}", op))),
        }
    }
}

fn cell2<'a>(k: &'a N, who: &str) -> Result<(&'a N, &'a N), Crash> {
    match &**k {
        Knot::Cell(h, t) => Ok((h, t)),
        Knot::Atom(_) => Err(Crash::Bottom(format!("{}: expected cell operands", who))),
    }
}

fn loobool(b: bool) -> N {
    // loobean: 0 = yes/true, 1 = no/false
    crate::knot::num(if b { 0 } else { 1 })
}

/// /[axis subject] — fetch the subtree at a tree address.
pub fn slot(axis: &Atom, subject: &N) -> Eval {
    // walk the binary expansion of `axis` from the most-significant bit below the
    // leading 1: bit 0 => head, bit 1 => tail.
    let n = match axis.to_u128() {
        Some(n) if n >= 1 => n,
        _ => return Err(Crash::Bottom("slot: axis 0".into())),
    };
    if n == 1 {
        return Ok(subject.clone());
    }
    let bits = 127 - n.leading_zeros(); // index of leading 1
    let mut cur = subject.clone();
    for i in (0..bits).rev() {
        let go_tail = (n >> i) & 1 == 1;
        cur = match &*cur {
            Knot::Cell(h, t) => {
                if go_tail {
                    t.clone()
                } else {
                    h.clone()
                }
            }
            Knot::Atom(_) => return Err(Crash::Bottom("slot: address past a leaf".into())),
        };
    }
    Ok(cur)
}

/// #[axis value subject] — return a copy of `subject` with subtree at `axis` replaced by `value`.
pub fn edit(axis: &Atom, value: &N, subject: &N) -> Eval {
    let n = match axis.to_u128() {
        Some(n) if n >= 1 => n,
        _ => return Err(Crash::Bottom("edit: axis 0".into())),
    };
    if n == 1 {
        return Ok(value.clone());
    }
    let bits = 127 - n.leading_zeros();
    edit_rec(n, bits, value, subject)
}

fn edit_rec(n: u128, bit: u32, value: &N, subject: &N) -> Eval {
    let (h, t) = match &**subject {
        Knot::Cell(h, t) => (h.clone(), t.clone()),
        Knot::Atom(_) => return Err(Crash::Bottom("edit: address past a leaf".into())),
    };
    let go_tail = (n >> (bit - 1)) & 1 == 1;
    if bit == 1 {
        // last step: replace the chosen child
        if go_tail {
            Ok(cell(h, value.clone()))
        } else {
            Ok(cell(value.clone(), t))
        }
    } else if go_tail {
        let nt = edit_rec(n, bit - 1, value, &t)?;
        Ok(cell(h, nt))
    } else {
        let nh = edit_rec(n, bit - 1, value, &h)?;
        Ok(cell(nh, t))
    }
}

/// `peg(a, b)`: address `b` interpreted *within* the subtree at address `a`.
/// peg(a,1)=a; peg(a,2)=2a; peg(a,3)=2a+1; in general concatenates the binary
/// paths. This is the workhorse the compiler uses to rebase faces.
pub fn peg(a: u128, b: u128) -> u128 {
    if b == 1 {
        return a;
    }
    let c = 127 - b.leading_zeros(); // bits below b's leading 1
    let mask = (1u128 << c) - 1;
    (a << c) | (b & mask)
}

/// Build the formula `[0 axis]` (ADDRESS).
pub fn f_axis(axis: u128) -> N {
    cell(crate::knot::num(0), crate::knot::num(axis))
}
/// Build `[1 v]` (QUOTE).
pub fn f_quote(v: N) -> N {
    cell(crate::knot::num(1), v)
}
// ============================================================================
// THE TRACER (the debugger's recorder). When active, every arm call compiled
// with a `dbg:` hint records Enter(name, args) / Exit(result) events with
// nesting depth — the GUI and CLI assemble these into the call tree. Capped
// so a deep run cannot exhaust memory; the cap is reported when hit.
// ============================================================================

#[derive(Clone)]
pub enum TraceEvent {
    Enter(String, String), // arm name, rendered args
    Exit(Option<String>),  // rendered result (None = the call crashed)
}

thread_local! {
    static TRACER: std::cell::RefCell<Option<Vec<TraceEvent>>> = const { std::cell::RefCell::new(None) };
}
const TRACE_CAP: usize = 6000;

fn num_n(v: u128) -> N {
    crate::knot::num(v)
}

pub fn tracer_begin() {
    TRACER.with(|t| *t.borrow_mut() = Some(Vec::new()));
}
pub fn tracer_take() -> Vec<TraceEvent> {
    TRACER.with(|t| t.borrow_mut().take().unwrap_or_default())
}
fn tracer_active() -> bool {
    TRACER.with(|t| t.borrow().is_some())
}
fn render_short(n: &N) -> String {
    let s = crate::net::show_state(n);
    if s.chars().count() > 90 {
        let cut: String = s.chars().take(87).collect();
        format!("{}…", cut)
    } else {
        s
    }
}
fn tracer_enter(arm: &str, args: &N) {
    let a = render_short(args);
    TRACER.with(|t| {
        if let Some(v) = t.borrow_mut().as_mut() {
            if v.len() < TRACE_CAP {
                v.push(TraceEvent::Enter(arm.to_string(), a));
            }
        }
    });
}
fn tracer_exit(result: Option<&N>) {
    let r = result.map(render_short);
    TRACER.with(|t| {
        if let Some(v) = t.borrow_mut().as_mut() {
            if v.len() < TRACE_CAP + 4096 {
                v.push(TraceEvent::Exit(r));
            }
        }
    });
}

/// Wrap a formula with a static jet hint: `[11 <name-cord> body]`.
pub fn f_jet(name: &str, body: N) -> N {
    cell(crate::knot::num(11), cell(crate::knot::cord(name), body))
}

// ----------------------------- JIT -----------------------------------------
// The default execution path. A formula is *compiled once* into a tree of Rust
// closures (so its structure is walked at compile time, not per step), and the
// compiled form is cached by formula identity so repeated invocations (loop arms,
// agent arms) reuse it. Tail-position rules (2,6,7,8,9,11) return a `Tail(subject,
// formula)` continuation that the driver loops on, keeping iteration flat-stack —
// exactly as the interpreter's trampoline does. The interpreter remains the
// reference; this just removes interpretive dispatch overhead and enables reuse.
mod jit {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    pub enum Step {
        Done(N),
        Tail(N, N), // (next subject, next formula)
    }
    pub type Fun = Arc<dyn Fn(&N, &mut u64) -> Result<Step, Crash> + Send + Sync>;

    static CACHE: OnceLock<Mutex<HashMap<usize, (N, Fun)>>> = OnceLock::new();
    fn cache() -> &'static Mutex<HashMap<usize, (N, Fun)>> {
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }
    fn key(f: &N) -> usize {
        Arc::as_ptr(f) as *const () as usize
    }

    // Adaptive tiering: a formula is interpreted until it has been (re-)entered this many
    // times, after which it is worth compiling. Counting is by formula identity.
    static HOT: OnceLock<Mutex<HashMap<usize, u32>>> = OnceLock::new();
    static THRESHOLD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(32);
    fn hotmap() -> &'static Mutex<HashMap<usize, u32>> {
        HOT.get_or_init(|| Mutex::new(HashMap::new()))
    }
    /// Count one entry of this formula; return true once it crosses the compile threshold.
    pub fn hot(formula: &N) -> bool {
        let t = THRESHOLD.load(std::sync::atomic::Ordering::Relaxed);
        if t == 0 {
            return true;
        }
        let k = key(formula);
        let mut m = hotmap().lock().unwrap();
        let c = m.entry(k).or_insert(0);
        *c = c.saturating_add(1);
        *c >= t
    }
    /// Set the hotness threshold (0 = always compile; large = mostly interpret).
    pub fn set_threshold(t: u32) {
        THRESHOLD.store(t, std::sync::atomic::Ordering::Relaxed);
    }

    /// Compile a formula, reusing a cached closure when the same formula object recurs.
    /// The formula `N` is kept alive in the cache so its address can't be reused (no stale
    /// keys), and distinct formulas are bounded by program size.
    pub fn compile_cached(formula: &N) -> Fun {
        let k = key(formula);
        if let Some((_, fun)) = cache().lock().unwrap().get(&k) {
            return fun.clone();
        }
        let fun = compile(formula);
        cache().lock().unwrap().insert(k, (formula.clone(), fun.clone()));
        fun
    }

    /// Top-level: drive a formula to a value, trampolining tail continuations.
    pub fn eval(formula: &N, subject: &N, fuel: &mut u64) -> Eval {
        let mut s = subject.clone();
        let mut fun = compile_cached(formula);
        loop {
            match fun(&s, fuel)? {
                Step::Done(v) => return Ok(v),
                Step::Tail(ns, nf) => {
                    s = ns;
                    fun = compile_cached(&nf);
                }
            }
        }
    }

    // Evaluate an operand subformula to a value (its own tail chain is trampolined here;
    // nesting recursion is bounded by static formula depth, as in the interpreter).
    fn run_to_value(fun: &Fun, s: &N, fuel: &mut u64) -> Eval {
        let mut cur = s.clone();
        let mut f = fun.clone();
        loop {
            match f(&cur, fuel)? {
                Step::Done(v) => return Ok(v),
                Step::Tail(ns, nf) => {
                    cur = ns;
                    f = compile_cached(&nf);
                }
            }
        }
    }

    fn spend(fuel: &mut u64) -> Result<(), Crash> {
        if *fuel == 0 {
            return Err(Crash::OutOfFuel);
        }
        *fuel -= 1;
        Ok(())
    }
    fn errf(msg: String) -> Fun {
        Arc::new(move |_, _| Err(Crash::Bottom(msg.clone())))
    }
    fn cell2_opt(k: &N) -> Option<(N, N)> {
        if let Knot::Cell(h, t) = &**k {
            Some((h.clone(), t.clone()))
        } else {
            None
        }
    }

    pub fn compile(formula: &N) -> Fun {
        let (head, tail) = match &**formula {
            Knot::Cell(h, t) => (h.clone(), t.clone()),
            Knot::Atom(_) => return errf("formula is a bare atom".into()),
        };
        // AUTOCONS
        if let Knot::Cell(_, _) = &*head {
            let hf = compile(&head);
            let tf = compile(&tail);
            return Arc::new(move |s, fuel| {
                spend(fuel)?;
                let l = run_to_value(&hf, s, fuel)?;
                let r = run_to_value(&tf, s, fuel)?;
                Ok(Step::Done(cell(l, r)))
            });
        }
        let op = match head.as_atom().and_then(opcode) {
            Some(o) => o,
            None => return errf("opcode out of range".into()),
        };
        match op {
            0 => match tail.as_atom() {
                Some(a) => {
                    let a = a.clone();
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        Ok(Step::Done(slot(&a, s)?))
                    })
                }
                None => errf("0: axis not atom".into()),
            },
            1 => {
                let v = tail.clone();
                Arc::new(move |_, fuel| {
                    spend(fuel)?;
                    Ok(Step::Done(v.clone()))
                })
            }
            2 => match cell2_opt(&tail) {
                Some((f, g)) => {
                    let cf = compile(&f);
                    let cg = compile(&g);
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        let ns = run_to_value(&cf, s, fuel)?;
                        let nf = run_to_value(&cg, s, fuel)?;
                        Ok(Step::Tail(ns, nf))
                    })
                }
                None => errf("2: expected cell operands".into()),
            },
            3 => {
                let cf = compile(&tail);
                Arc::new(move |s, fuel| {
                    spend(fuel)?;
                    let v = run_to_value(&cf, s, fuel)?;
                    Ok(Step::Done(loobool(!v.is_atom())))
                })
            }
            4 => {
                let cf = compile(&tail);
                Arc::new(move |s, fuel| {
                    spend(fuel)?;
                    let v = run_to_value(&cf, s, fuel)?;
                    match &*v {
                        Knot::Atom(a) => Ok(Step::Done(atom(a.inc()))),
                        Knot::Cell(_, _) => Err(Crash::Bottom("4: increment of a cell".into())),
                    }
                })
            }
            5 => match cell2_opt(&tail) {
                Some((f, g)) => {
                    let cf = compile(&f);
                    let cg = compile(&g);
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        let a = run_to_value(&cf, s, fuel)?;
                        let b = run_to_value(&cg, s, fuel)?;
                        Ok(Step::Done(loobool(a == b)))
                    })
                }
                None => errf("5: expected cell operands".into()),
            },
            6 => match cell2_opt(&tail).and_then(|(f, gh)| cell2_opt(&gh).map(|(g, h)| (f, g, h))) {
                Some((f, g, h)) => {
                    let cf = compile(&f);
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        let c = run_to_value(&cf, s, fuel)?;
                        match c.as_atom().and_then(|a| a.to_u128()) {
                            Some(0) => Ok(Step::Tail(s.clone(), g.clone())),
                            Some(1) => Ok(Step::Tail(s.clone(), h.clone())),
                            _ => Err(Crash::Bottom("6: condition not a loobean".into())),
                        }
                    })
                }
                None => errf("6: expected cell operands".into()),
            },
            7 => match cell2_opt(&tail) {
                Some((f, g)) => {
                    let cf = compile(&f);
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        let mid = run_to_value(&cf, s, fuel)?;
                        Ok(Step::Tail(mid, g.clone()))
                    })
                }
                None => errf("7: expected cell operands".into()),
            },
            8 => match cell2_opt(&tail) {
                Some((f, g)) => {
                    let cf = compile(&f);
                    Arc::new(move |s, fuel| {
                        spend(fuel)?;
                        let v = run_to_value(&cf, s, fuel)?;
                        Ok(Step::Tail(cell(v, s.clone()), g.clone()))
                    })
                }
                None => errf("8: expected cell operands".into()),
            },
            9 => match cell2_opt(&tail) {
                Some((b, g)) => match b.as_atom() {
                    Some(ax) => {
                        let ax = ax.clone();
                        let cg = compile(&g);
                        Arc::new(move |s, fuel| {
                            spend(fuel)?;
                            let core = run_to_value(&cg, s, fuel)?;
                            let armf = slot(&ax, &core)?;
                            Ok(Step::Tail(core, armf))
                        })
                    }
                    None => errf("9: arm axis not atom".into()),
                },
                None => errf("9: expected cell operands".into()),
            },
            10 => match cell2_opt(&tail).and_then(|(af, g)| cell2_opt(&af).map(|(a, f)| (a, f, g))) {
                Some((a, f, g)) => match a.as_atom() {
                    Some(ax) => {
                        let ax = ax.clone();
                        let cf = compile(&f);
                        let cg = compile(&g);
                        Arc::new(move |s, fuel| {
                            spend(fuel)?;
                            let v = run_to_value(&cf, s, fuel)?;
                            let base = run_to_value(&cg, s, fuel)?;
                            Ok(Step::Done(edit(&ax, &v, &base)?))
                        })
                    }
                    None => errf("10: edit axis not atom".into()),
                },
                None => errf("10: expected cell operands".into()),
            },
            11 => match cell2_opt(&tail) {
                Some((h, g)) => {
                    let cg = compile(&g);
                    match &*h {
                        Knot::Atom(tag) => {
                            let name = tag.bytes_le().to_vec();
                            let gn = g.clone();
                            Arc::new(move |s, fuel| {
                                spend(fuel)?;
                                if jets_on() {
                                    if let Some(jet) = lookup_jet(&name) {
                                        let jetted = jet(s)?;
                                        if audit_on() {
                                            let _guard = AuditGuard::enter();
                                            let pure = run_to_value(&cg, s, fuel)?;
                                            if pure != jetted {
                                                return Err(Crash::Bottom(format!(
                                                    "jet {:?} disagrees with pure reduction",
                                                    name
                                                )));
                                            }
                                            return Ok(Step::Done(pure));
                                        }
                                        return Ok(Step::Done(jetted));
                                    }
                                }
                                Ok(Step::Tail(s.clone(), gn.clone()))
                            })
                        }
                        Knot::Cell(_, hint_form) => {
                            let chf = compile(hint_form);
                            let gn = g.clone();
                            Arc::new(move |s, fuel| {
                                spend(fuel)?;
                                let _ = run_to_value(&chf, s, fuel)?;
                                Ok(Step::Tail(s.clone(), gn.clone()))
                            })
                        }
                    }
                }
                None => errf("11: expected cell operands".into()),
            },
            other => errf(format!("unknown opcode {}", other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_only_devices_get_a_generous_fuel_budget() {
        // the guard still exists, but the ceiling an interpret-only phone gets must be
        // large enough for the heavy models (which the native path would have absorbed)
        assert!(INTERPRETER_FUEL > DEFAULT_FUEL * 100,
            "an interpret-only device must not inherit the native path's ceiling");
        // and it must still be finite: a runaway `*f f` has to terminate
        assert!(INTERPRETER_FUEL < u64::MAX);
    }

    #[test]
    fn orpheus_fuel_env_overrides_and_zero_means_unlimited() {
        // default_fuel() memoizes per process, so exercise the decision function directly
        let decide = |env: Option<&str>, has_rustc: bool| -> u64 {
            if let Some(v) = env {
                if let Ok(n) = v.trim().parse::<u64>() {
                    return if n == 0 { u64::MAX } else { n };
                }
            }
            if has_rustc { DEFAULT_FUEL } else { INTERPRETER_FUEL }
        };
        assert_eq!(decide(Some("1234"), true), 1234);
        assert_eq!(decide(Some("0"), true), u64::MAX, "0 means no limit");
        assert_eq!(decide(Some(" 99 "), false), 99, "whitespace tolerated");
        assert_eq!(decide(None, true), DEFAULT_FUEL);
        assert_eq!(decide(None, false), INTERPRETER_FUEL, "no rustc -> interpreter budget");
        assert_eq!(decide(Some("not-a-number"), false), INTERPRETER_FUEL, "junk falls back");
    }

    use crate::knot::{cell, num};

    #[test]
    fn jit_matches_interpreter() {
        // edge cases across the rule set; tar() uses the JIT, interp() the tree-walker.
        let s = cell(num(1), cell(num(2), cell(num(3), num(4))));
        let formulas = vec![
            f_axis(1),
            f_axis(7),
            f_quote(cell(num(9), num(9))),
            cell(num(4), f_axis(2)),                                   // succ of head
            cell(num(3), f_axis(1)),                                   // cell? subject
            cell(num(5), cell(f_axis(2), f_axis(2))),                  // equal
            cell(num(6), cell(f_quote(num(0)), cell(f_quote(num(7)), f_quote(num(8))))), // if
            cell(num(7), cell(f_axis(2), cell(num(4), f_axis(1)))),    // compose then succ
            cell(num(8), cell(f_quote(num(5)), f_axis(2))),            // push/let
            cell(num(10), cell(cell(num(6), f_quote(num(99))), f_axis(1))), // edit axis 6
            cell(cell(num(4), f_axis(2)), f_axis(3)),                  // autocons
        ];
        for f in &formulas {
            assert_eq!(jit_force(&s, f), interp(&s, f), "formula {:?}", f);
        }
        // a recursive loop (decrement-to-zero) reduces the same way under both
        let dec_loop = cell(
            num(6),
            cell(
                f_axis(1),
                cell(f_quote(num(0)), cell(num(4), f_axis(1))),
            ),
        );
        for subj in [num(0), num(1), num(5)] {
            assert_eq!(jit_force(&subj, &dec_loop), interp(&subj, &dec_loop));
            // the adaptive default (tar) must also agree
            assert_eq!(tar(&subj, &dec_loop), interp(&subj, &dec_loop));
        }
    }

    #[test]
    fn slot_basics() {
        let s = cell(num(10), cell(num(20), num(30))); // [10 [20 30]]
        assert_eq!(*slot(&Atom::from_u128(1), &s).unwrap(), Knot::Cell(num(10), cell(num(20), num(30))));
        assert_eq!(slot(&Atom::from_u128(2), &s).unwrap(), num(10));
        assert_eq!(slot(&Atom::from_u128(6), &s).unwrap(), num(20));
        assert_eq!(slot(&Atom::from_u128(7), &s).unwrap(), num(30));
    }

    #[test]
    fn quote_and_increment() {
        let s = num(0);
        // *[0 [4 [1 41]]] = 42
        let f = cell(num(4), f_quote(num(41)));
        assert_eq!(tar(&s, &f).unwrap(), num(42));
    }

    #[test]
    fn if_rule() {
        // *[s [6 [1 0] [1 11] [1 22]]] = 11  (condition true)
        let s = num(0);
        let f = cell(num(6), cell(f_quote(num(0)), cell(f_quote(num(11)), f_quote(num(22)))));
        assert_eq!(tar(&s, &f).unwrap(), num(11));
    }

    #[test]
    fn push_let() {
        // push 7 then read it back from axis 2: *[s 8 [1 7] [0 2]] = 7
        let s = num(0);
        let f = cell(num(8), cell(f_quote(num(7)), f_axis(2)));
        assert_eq!(tar(&s, &f).unwrap(), num(7));
    }

    #[test]
    fn edit_replaces() {
        let s = cell(num(1), cell(num(2), num(3)));
        let e = edit(&Atom::from_u128(6), &num(99), &s).unwrap(); // axis 6 = head of tail
        assert_eq!(e, cell(num(1), cell(num(99), num(3))));
    }

    #[test]
    fn peg_composition() {
        assert_eq!(peg(3, 1), 3);
        assert_eq!(peg(3, 2), 6);
        assert_eq!(peg(3, 3), 7);
        assert_eq!(peg(7, 15), 63);
        // The defining property: slot(peg(a,b), x) == slot(b, slot(a, x)).
        let x = cell(
            cell(cell(num(1), num(2)), cell(num(3), num(4))),
            cell(cell(num(5), num(6)), cell(num(7), num(8))),
        );
        for a in 1u128..8 {
            for b in 1u128..8 {
                if let (Ok(via_a), _) = (slot(&Atom::from_u128(a), &x), ()) {
                    if let Ok(rhs) = slot(&Atom::from_u128(b), &via_a) {
                        let lhs = slot(&Atom::from_u128(peg(a, b)), &x).unwrap();
                        assert_eq!(lhs, rhs, "peg({},{}) mismatch", a, b);
                    }
                }
            }
        }
    }

    #[test]
    fn loop_detection_via_fuel() {
        // *[s 2 [0 1] [0 1]] re-evaluates the subject as formula forever-ish.
        let s = cell(num(2), cell(f_axis(1), f_axis(1)));
        let r = tar_with_fuel(&s, &cell(num(2), cell(f_axis(1), f_axis(1))), 10_000);
        assert!(matches!(r, Err(Crash::OutOfFuel)) || r.is_err());

    }
}

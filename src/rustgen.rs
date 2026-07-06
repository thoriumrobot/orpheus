//! rustgen — an optimizing Latte → Rust compiler ("Anvil").
//!
//! Where the JIT compiles Loom formulas to closures at run time, this is an *ahead-of-time*
//! compiler that emits standalone Rust source for a Latte expression and its whole library
//! closure. The emitted program carries a tiny self-contained noun runtime, so it compiles with
//! a stock `rustc` and runs natively — no dependency on this crate.
//!
//! Optimizations applied:
//!   * constant folding   — literal-only subexpressions are evaluated at compile time;
//!   * native primitives  — the arithmetic/▸comparison jets lower to native Rust ops (a `u128`
//!                          fast path, widening to bignum on demand) instead of interpreting
//!                          their Latte bodies on the VM;
//!   * dead-arm removal   — only arms reachable from `__main` are emitted;
//!   * let → Rust `let`   — sharing is preserved (no recomputation);
//!   * tail calls → loops — `loop … again(..)` becomes a real Rust `loop` with mutable state;
//!   * HOFs stay general  — lambdas compile to reusable native closures (so `map`/`filter`/
//!                          `foldl` and friends compile straight from their Latte definitions).
//!
//! Atoms are arbitrary-precision naturals, matching the interpreter at every width. Arithmetic
//! keeps a `u128` fast path (the overwhelmingly common case) and falls back to base-256 bignum
//! arithmetic — carried in the same `V::Big` little-endian byte vector used for long cords — the
//! moment a value or result exceeds `u128`. So `add`/`sub`/`mul`/`div`/`mod`/`lt`/`inc`/`dec` agree
//! with the interpreter for naturals of any size; in particular the database's bloom filter and
//! hash bitset, which build values hundreds or thousands of bits wide via `shl`/`pow`/`bor`, run
//! natively instead of overflowing `u128` and falling back. The bloom/hash hot path is powers of
//! two (`shl 1 i`, `div … 2`), special-cased to bit/byte shifts so a wide bitset stays fast; only
//! a non-power-of-two big divisor takes the general binary long-division path. Domain errors
//! (overflow's cousin underflow, divide-by-zero, `dec(0)`) still `panic!` the compiled program on
//! exactly the inputs that crash the interpreter, so the two agree on success values *and* on
//! which inputs fail. The cord operations (`bytes`/`frombytes`/`cat`/`catall`/`bytelen`) remain
//! jetted to byte-vector ops; `vbig` keeps the canonical split (<=16 significant bytes in `V::A`,
//! longer in `V::Big`) so equality stays a representation compare.
//!
//! Identifier hygiene: Latte binders are renamed for emission when they collide with a native
//! primitive (`sub`, `div`, …), a Rust keyword (`as`, `type`, `move`, …), or a jetted cord op —
//! see `binder_rename` — so ordinary Latte arms like `zip2 = fn [as bs] …` compile natively
//! instead of forcing a fallback.
//!
//! Build & cache: each program is compiled once by `build_native` (via `rustc`, default
//! `opt-level=0` for fast cold starts — override with `ORPHEUS_OPT`) into a content-addressed
//! binary under `cache_dir`, reused across runs. The cache self-bounds: `evict_to_cap` drops the
//! least-recently-used binaries (mtime, refreshed per run) once it exceeds `cache_cap_bytes`
//! (`ORPHEUS_CACHE_MAX`, default 512 MiB). A program may also read its input from stdin at run
//! time, so one binary serves many inputs (see `run_native_with_input`). With `ORPHEUS_CACHE_SHARED`
//! set, the cache is a read-through/write-back mirror of a shared store (namespaced by toolchain
//! identity), so a program is compiled once across a fleet rather than once per host. Each binary
//! carries a `<name>.sha` integrity sidecar (hash+size): pulls are hash-verified before install and
//! a quick size check guards each run, so corruption or a poisoned store entry self-heals. Lifetime
//! counters (builds/hits/pulls/failures, persisted) back `latte cache metrics`.

use crate::latte::{self, Ast};
use std::collections::{HashMap, HashSet};

type Arm = (String, Vec<String>, Ast);

// the irreducible jets, lowered to native Rust (with their arities)
fn prim_arity(name: &str) -> Option<usize> {
    match name {
        "add" | "sub" | "mul" | "div" | "mod" | "lt" => Some(2),
        "dec" => Some(1),
        _ => None,
    }
}

/// True for any reserved Rust keyword (strict, reserved-for-future, and the 2018+
/// additions).  A Latte binder named `as`, `type`, `move`, … is a perfectly good
/// Latte identifier but cannot be emitted verbatim as a Rust local/parameter, so it
/// must be alpha-renamed for the native backend just like a shadowed primitive.
fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern"
            | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match"
            | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
            | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe"
            | "use" | "where" | "while" | "async" | "await" | "dyn" | "abstract"
            | "become" | "box" | "do" | "final" | "macro" | "override" | "priv"
            | "typeof" | "unsized" | "virtual" | "yield" | "try"
    )
}

/// A binder must be alpha-renamed for native emission if it shadows a primitive
/// (whose name the backend emits as an operator) OR collides with a Rust keyword
/// (which is not a legal Rust identifier).
fn binder_rename(name: &str) -> bool {
    prim_arity(name).is_some() || is_rust_keyword(name) || cord_jet_arity(name).is_some()
}

/// Cord operations the native backend jets to byte-vector ops instead of base-256
/// arithmetic, so cords longer than 16 bytes (which overflow u128) work natively.
/// Like primitives, these names are emitted directly at call sites and their binders
/// are alpha-renamed, so a local of the same name never silently captures the jet.
fn cord_jet_arity(name: &str) -> Option<usize> {
    match name {
        "bytes" | "frombytes" | "catall" | "bytelen" => Some(1),
        "cat" => Some(2),
        _ => None,
    }
}

/// A name the backend handles intrinsically (an arithmetic primitive or a cord jet),
/// rather than by emitting/reaching a library arm of that name.
fn is_native_op(name: &str) -> bool {
    prim_arity(name).is_some() || cord_jet_arity(name).is_some()
}

/// Alpha-rename any binder of a primitive name (add/sub/mul/div/mod/lt/dec) to a
/// fresh non-primitive name, so the native backend — which emits those names as
/// Rust operators and const-folds them with no scope awareness — can still compile
/// a program that uses, say, `sub` or `div` as an ordinary local. `env` maps a
/// currently-shadowed primitive name to its replacement; `ctr` gensyms fresh names.
/// Renaming follows lexical scope: a non-primitive binding of the same name (or a
/// re-binding) shadows the rename, and `let`/`loop` initialisers are rewritten in
/// the OUTER scope (they cannot see the binding they introduce).
fn fresh(n: &str, ctr: &mut usize) -> String {
    let f = format!("{}__shadow{}", n, *ctr);
    *ctr += 1;
    f
}
fn unshadow(ast: &Ast, env: &HashMap<String, String>, ctr: &mut usize, arms: &HashSet<String>) -> Ast {
    let go = |a: &Ast, e: &HashMap<String, String>, c: &mut usize| Box::new(unshadow(a, e, c, arms));
    // A binder must be alpha-renamed when its name is one the backend treats specially at
    // a *use* site rather than a *binding* site: a primitive/jet (emitted as an operator), a
    // Rust keyword (not a legal identifier), OR a top-level arm. The arm case is the subtle
    // one: `free_vars` excludes arm names from a closure's capture set (assuming they are
    // global references), so a local that *shadows* an arm — e.g. a parameter named `field`,
    // which is also an arm in ui.lat — would be dropped from the capture and emit as an
    // unbound variable. Renaming the binder makes the local a distinct name that captures
    // normally; the shadowed arm was unreachable in that scope anyway.
    let needs_rename = |nm: &str| binder_rename(nm) || arms.contains(nm);
    match ast {
        Ast::Var(v) => Ast::Var(env.get(v).cloned().unwrap_or_else(|| v.clone())),
        Ast::Lit(_) | Ast::Tag(_) | Ast::Text(_) | Ast::Nil => ast.clone(),
        Ast::Inc(e) => Ast::Inc(go(e, env, ctr)),
        Ast::Head(e) => Ast::Head(go(e, env, ctr)),
        Ast::Tail(e) => Ast::Tail(go(e, env, ctr)),
        Ast::IsCell(e) => Ast::IsCell(go(e, env, ctr)),
        Ast::Fast(nm, e) => Ast::Fast(nm.clone(), go(e, env, ctr)),
        Ast::Eq(a, b) => Ast::Eq(go(a, env, ctr), go(b, env, ctr)),
        Ast::If(c, a, b) => Ast::If(go(c, env, ctr), go(a, env, ctr), go(b, env, ctr)),
        Ast::Tuple(xs) => Ast::Tuple(xs.iter().map(|x| unshadow(x, env, ctr, arms)).collect()),
        Ast::Again(xs) => Ast::Again(xs.iter().map(|x| unshadow(x, env, ctr, arms)).collect()),
        Ast::Call(f, xs) => {
            // a callee that names a renamed local (a shadowing binder in `env`) must follow
            // the rename so it resolves to the local (vapply), not the shadowed arm/primitive;
            // a genuine arm/primitive call is not in `env` and is left untouched.
            let f2 = env.get(f).cloned().unwrap_or_else(|| f.clone());
            Ast::Call(f2, xs.iter().map(|x| unshadow(x, env, ctr, arms)).collect())
        }
        Ast::Case(s, cases) => Ast::Case(
            go(s, env, ctr),
            cases.iter().map(|(p, e)| (p.clone(), unshadow(e, env, ctr, arms))).collect(),
        ),
        Ast::Let(n, v, b) => {
            let recursive = recursive_let_gate(n, v);
            let mut env2 = env.clone();
            let name = if needs_rename(n) {
                let f = fresh(n, ctr);
                env2.insert(n.clone(), f.clone());
                f
            } else {
                env2.remove(n); // a plain binding shadows any active rename
                n.clone()
            };
            // a SELF-RECURSIVE gate's initialiser references the binding itself,
            // so it renames under env2; an ordinary initialiser is outer-scope
            let v2 = if recursive { unshadow(v, &env2, ctr, arms) } else { unshadow(v, env, ctr, arms) };
            Ast::Let(name, Box::new(v2), Box::new(unshadow(b, &env2, ctr, arms)))
        }
        Ast::Gate(params, body) => {
            let mut env2 = env.clone();
            let params2 = params
                .iter()
                .map(|p| {
                    if needs_rename(p) {
                        let f = fresh(p, ctr);
                        env2.insert(p.clone(), f.clone());
                        f
                    } else {
                        env2.remove(p);
                        p.clone()
                    }
                })
                .collect();
            Ast::Gate(params2, Box::new(unshadow(body, &env2, ctr, arms)))
        }
        Ast::Loop(binds, body) => {
            let mut env2 = env.clone();
            let binds2 = binds
                .iter()
                .map(|(n, v)| {
                    let v2 = unshadow(v, env, ctr, arms); // initialiser in the outer scope
                    let name = if needs_rename(n) {
                        let f = fresh(n, ctr);
                        env2.insert(n.clone(), f.clone());
                        f
                    } else {
                        env2.remove(n);
                        n.clone()
                    };
                    (name, v2)
                })
                .collect();
            Ast::Loop(binds2, Box::new(unshadow(body, &env2, ctr, arms)))
        }
    }
}

fn sanitize(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    s
}

/// Emit a cord/tag literal as a native value: short cords (<=16 bytes) fold to a
/// `V::A(u128)`; longer ones emit a `vbig(vec![..])` byte-vector atom, so string
/// literals of any length compile natively. Trailing zero bytes are dropped to
/// match `vbig`'s canonical form.
fn emit_cord_lit(t: &str) -> String {
    let raw = t.as_bytes();
    let mut end = raw.len();
    while end > 0 && raw[end - 1] == 0 {
        end -= 1;
    }
    let b = &raw[..end];
    if b.len() <= 16 {
        let mut v: u128 = 0;
        for (i, &byte) in b.iter().enumerate() {
            v |= (byte as u128) << (8 * i);
        }
        format!("V::A({}u128)", v)
    } else {
        let bytes: Vec<String> = b.iter().map(|x| format!("{}u8", x)).collect();
        format!("vbig(vec![{}])", bytes.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Constant folding: simplify literal-only subexpressions before emission.
// ---------------------------------------------------------------------------
fn as_lit(a: &Ast) -> Option<u128> {
    if let Ast::Lit(n) = a {
        Some(*n)
    } else {
        None
    }
}

fn fold(ast: &Ast) -> Ast {
    match ast {
        Ast::Inc(e) => {
            let e = fold(e);
            if let Some(n) = as_lit(&e) {
                if let Some(r) = n.checked_add(1) {
                    return Ast::Lit(r);
                }
            }
            Ast::Inc(Box::new(e))
        }
        Ast::Eq(a, b) => {
            let (a, b) = (fold(a), fold(b));
            if let (Some(x), Some(y)) = (as_lit(&a), as_lit(&b)) {
                return Ast::Lit(if x == y { 0 } else { 1 });
            }
            Ast::Eq(Box::new(a), Box::new(b))
        }
        Ast::If(c, t, e) => {
            let c = fold(c);
            let t = fold(t);
            let e = fold(e);
            if let Some(cv) = as_lit(&c) {
                return if cv == 0 { t } else { e };
            }
            Ast::If(Box::new(c), Box::new(t), Box::new(e))
        }
        Ast::Let(n, v, b) => Ast::Let(n.clone(), Box::new(fold(v)), Box::new(fold(b))),
        Ast::Head(e) => Ast::Head(Box::new(fold(e))),
        Ast::Tail(e) => Ast::Tail(Box::new(fold(e))),
        Ast::IsCell(e) => Ast::IsCell(Box::new(fold(e))),
        Ast::Tuple(es) => Ast::Tuple(es.iter().map(fold).collect()),
        Ast::Again(es) => Ast::Again(es.iter().map(fold).collect()),
        Ast::Loop(binds, b) => Ast::Loop(
            binds.iter().map(|(n, e)| (n.clone(), fold(e))).collect(),
            Box::new(fold(b)),
        ),
        Ast::Case(s, arms) => Ast::Case(
            Box::new(fold(s)),
            arms.iter().map(|(p, e)| (p.clone(), fold(e))).collect(),
        ),
        Ast::Fast(n, b) => Ast::Fast(n.clone(), Box::new(fold(b))),
        Ast::Gate(p, b) => Ast::Gate(p.clone(), Box::new(fold(b))),
        Ast::Call(name, args) => {
            let args: Vec<Ast> = args.iter().map(fold).collect();
            // fold the native arithmetic primitives over literal arguments
            if prim_arity(name) == Some(args.len()) {
                let lits: Option<Vec<u128>> = args.iter().map(as_lit).collect();
                if let Some(xs) = lits {
                    if let Some(v) = eval_prim(name, &xs) {
                        return Ast::Lit(v);
                    }
                }
            }
            Ast::Call(name.clone(), args)
        }
        other => other.clone(),
    }
}

fn eval_prim(name: &str, xs: &[u128]) -> Option<u128> {
    match name {
        "add" => xs[0].checked_add(xs[1]),
        "sub" => xs[0].checked_sub(xs[1]),
        "mul" => xs[0].checked_mul(xs[1]),
        "div" => xs[0].checked_div(xs[1]),
        "mod" => xs[0].checked_rem(xs[1]),
        "lt" => Some(if xs[0] < xs[1] { 0 } else { 1 }),
        "dec" => xs[0].checked_sub(1),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Free-variable analysis (for closing over enclosing locals in lambdas).
// ---------------------------------------------------------------------------
/// `let name = fn … in …` where the gate's body mentions `name` (and no
/// parameter shadows it): the SELF-RECURSIVE let-bound gate. The three walkers
/// below (unshadow, free_vars, emit) must all treat `name` as bound INSIDE the
/// initialiser for such a let — mirroring the interpreter, where the gate gets
/// a face for its own name.
fn recursive_let_gate(name: &str, val: &Ast) -> bool {
    if let Ast::Gate(ps, gb) = val {
        !ps.iter().any(|p| p == name) && crate::icomb::mentions(gb, name)
    } else {
        false
    }
}

fn free_vars(ast: &Ast, bound: &HashSet<String>, arms: &HashSet<String>, out: &mut Vec<String>) {
    let see = |n: &str, out: &mut Vec<String>| {
        if !bound.contains(n) && !arms.contains(n) && prim_arity(n).is_none() && cord_jet_arity(n).is_none() && !out.contains(&n.to_string()) {
            out.push(n.to_string());
        }
    };
    match ast {
        Ast::Var(n) => see(n, out),
        Ast::Call(n, args) => {
            see(n, out);
            for a in args {
                free_vars(a, bound, arms, out);
            }
        }
        Ast::Inc(e) | Ast::Head(e) | Ast::Tail(e) | Ast::IsCell(e) | Ast::Fast(_, e) => {
            free_vars(e, bound, arms, out)
        }
        Ast::Eq(a, b) => {
            free_vars(a, bound, arms, out);
            free_vars(b, bound, arms, out);
        }
        Ast::If(c, t, e) => {
            free_vars(c, bound, arms, out);
            free_vars(t, bound, arms, out);
            free_vars(e, bound, arms, out);
        }
        Ast::Let(n, v, b) => {
            let mut b2 = bound.clone();
            b2.insert(n.clone());
            // in a self-recursive gate the name is bound within its own initialiser
            if recursive_let_gate(n, v) {
                free_vars(v, &b2, arms, out);
            } else {
                free_vars(v, bound, arms, out);
            }
            free_vars(b, &b2, arms, out);
        }
        Ast::Tuple(es) | Ast::Again(es) => {
            for e in es {
                free_vars(e, bound, arms, out);
            }
        }
        Ast::Loop(binds, body) => {
            for (_, e) in binds {
                free_vars(e, bound, arms, out);
            }
            let mut b2 = bound.clone();
            for (n, _) in binds {
                b2.insert(n.clone());
            }
            free_vars(body, &b2, arms, out);
        }
        Ast::Case(s, cases) => {
            free_vars(s, bound, arms, out);
            for (_, e) in cases {
                free_vars(e, bound, arms, out);
            }
        }
        Ast::Gate(params, body) => {
            let mut b2 = bound.clone();
            for p in params {
                b2.insert(p.clone());
            }
            free_vars(body, &b2, arms, out);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Codegen
// ---------------------------------------------------------------------------
struct Ctx<'a> {
    arms: &'a HashSet<String>,
    arity: &'a HashMap<String, usize>,
    bound: HashSet<String>,        // in-scope locals (params, lets, loop vars)
    loops: Vec<(usize, Vec<String>)>, // (label id, var names) innermost last
    tmp: usize,
}

impl<'a> Ctx<'a> {
    fn fresh(&mut self) -> usize {
        self.tmp += 1;
        self.tmp
    }
}

fn emit(ast: &Ast, c: &mut Ctx) -> Result<String, String> {
    Ok(match ast {
        Ast::Lit(n) => format!("V::A({}u128)", n),
        Ast::Nil => "V::A(0u128)".to_string(),
        Ast::Tag(t) => emit_cord_lit(t),
        Ast::Text(t) => emit_cord_lit(t),
        Ast::Var(n) => {
            if c.bound.contains(n) {
                format!("{}.clone()", sanitize(n))
            } else if let Some(&k) = c.arity.get(n) {
                // eta-expansion (mirrors the interpreter): an arm as a value is a
                // gate that calls it
                let params: Vec<String> = (0..k).map(|i| format!("__e{}", i)).collect();
                let call = Ast::Call(n.clone(), params.iter().map(|p| Ast::Var(p.clone())).collect());
                let gate = Ast::Gate(params, Box::new(call));
                return emit(&gate, c);
            } else {
                return Err(format!("unbound variable '{}'", n));
            }
        }
        Ast::Tuple(es) => {
            if es.is_empty() {
                "V::A(0u128)".to_string()
            } else {
                let parts: Result<Vec<String>, String> = es.iter().map(|e| emit(e, c)).collect();
                let parts = parts?;
                let mut acc = parts.last().unwrap().clone();
                for p in parts.iter().rev().skip(1) {
                    acc = format!("vcell({}, {})", p, acc);
                }
                acc
            }
        }
        Ast::Inc(e) => format!("vinc({})", emit(e, c)?),
        Ast::Head(e) => format!("vhead({})", emit(e, c)?),
        Ast::Tail(e) => format!("vtail({})", emit(e, c)?),
        Ast::IsCell(e) => format!("viscell({})", emit(e, c)?),
        Ast::Eq(a, b) => format!("vloob(veq(&{}, &{}))", emit(a, c)?, emit(b, c)?),
        Ast::If(cond, t, e) => format!(
            "if viszero(&{}) {{ {} }} else {{ {} }}",
            emit(cond, c)?,
            emit(t, c)?,
            emit(e, c)?
        ),
        Ast::Let(n, v, b) => {
            if recursive_let_gate(n, v) {
                // THE KNOT: a self-recursive gate becomes a closure that pulls
                // itself out of a shared cell tied immediately after creation —
                // the Rust rendering of the interpreter's axis-1 self-face.
                let (params, gbody) = match v.as_ref() {
                    Ast::Gate(p, gb) => (p, gb.as_ref()),
                    _ => unreachable!(),
                };
                let id = c.fresh();
                // captured frees of the gate body: params and the name are bound
                let mut pset: HashSet<String> = params.iter().cloned().collect();
                pset.insert(n.clone());
                let mut fvs = Vec::new();
                free_vars(gbody, &pset, c.arms, &mut fvs);
                let fvs: Vec<String> = fvs.into_iter().filter(|x| c.bound.contains(x)).collect();
                let mut caps = String::new();
                for x in &fvs {
                    caps.push_str(&format!("let {0} = {0}.clone(); ", sanitize(x)));
                }
                let saved = c.bound.clone();
                c.bound = params.iter().cloned().collect();
                c.bound.insert(n.clone());
                for x in &fvs {
                    c.bound.insert(x.clone());
                }
                let mut binds = String::new();
                for (i, p) in params.iter().enumerate() {
                    binds.push_str(&format!("let {} = __args[{}].clone(); ", sanitize(p), i));
                }
                let gs = emit(gbody, c)?;
                // the let-body sees the finished gate
                c.bound = saved;
                let added = c.bound.insert(n.clone());
                let bs = emit(b, c)?;
                if added {
                    c.bound.remove(n);
                }
                format!(
                    "{{ let __knot{id}: std::rc::Rc<std::cell::RefCell<Option<V>>> = std::rc::Rc::new(std::cell::RefCell::new(None));                      let {nm} = {{ {caps}let __k = __knot{id}.clone();                      V::F(std::rc::Rc::new(move |__args: Vec<V>| -> V {{                      let {nm} = __k.borrow().as_ref().expect(\"recursive gate untied\").clone(); {binds}{gs} }})) }};                      *__knot{id}.borrow_mut() = Some({nm}.clone()); {bs} }}",
                    id = id, nm = sanitize(n), caps = caps, binds = binds, gs = gs, bs = bs
                )
            } else {
                let vs = emit(v, c)?;
                let added = c.bound.insert(n.clone());
                let bs = emit(b, c)?;
                if added {
                    c.bound.remove(n);
                }
                format!("{{ let {} = {}; {} }}", sanitize(n), vs, bs)
            }
        }
        Ast::Case(scrut, cases) => {
            let s = emit(scrut, c)?;
            let id = c.fresh();
            let mut out = format!("{{ let __s{} = {}; ", id, s);
            let mut first = true;
            for (pat, body) in cases.iter() {
                if let Some(tag) = pat {
                    let kw = if first { "if" } else { "else if" };
                    // Compare against the Big-aware cord literal so case patterns work for tags of
                    // any length, exactly like long string/tag literals elsewhere — not just those
                    // that fit a u128.
                    out.push_str(&format!(
                        "{} veq(&__s{}, &{}) {{ {} }} ",
                        kw,
                        id,
                        emit_cord_lit(tag),
                        emit(body, c)?
                    ));
                    first = false;
                }
            }
            let default = cases.iter().find(|(p, _)| p.is_none());
            let elsebody = match default {
                Some((_, d)) => emit(d, c)?,
                None => "panic!(\"case: no match\")".to_string(),
            };
            if first {
                out.push_str(&format!("{} }}", elsebody));
            } else {
                out.push_str(&format!("else {{ {} }} }}", elsebody));
            }
            out
        }
        Ast::Loop(binds, body) => {
            let id = c.fresh();
            let mut decls = String::new();
            for (n, init) in binds {
                let is = emit(init, c)?;
                decls.push_str(&format!("let mut {} = {}; ", sanitize(n), is));
            }
            // bind loop vars, push loop frame
            let mut added = Vec::new();
            for (n, _) in binds {
                if c.bound.insert(n.clone()) {
                    added.push(n.clone());
                }
            }
            c.loops.push((id, binds.iter().map(|(n, _)| n.clone()).collect()));
            let bodys = emit(body, c)?;
            c.loops.pop();
            for n in added {
                c.bound.remove(&n);
            }
            format!("{{ {}'l{}: loop {{ break ({}); }} }}", decls, id, bodys)
        }
        Ast::Again(args) => {
            let (id, names) = c
                .loops
                .last()
                .cloned()
                .ok_or("again() outside a loop")?;
            if args.len() != names.len() {
                return Err(format!(
                    "again expects {} argument(s), got {}",
                    names.len(),
                    args.len()
                ));
            }
            let mut out = String::from("{ ");
            let mut tmps = Vec::new();
            for (i, a) in args.iter().enumerate() {
                let id2 = c.fresh();
                out.push_str(&format!("let __a{} = {}; ", id2, emit(a, c)?));
                tmps.push((names[i].clone(), id2));
            }
            for (name, id2) in tmps {
                out.push_str(&format!("{} = __a{}; ", sanitize(&name), id2));
            }
            out.push_str(&format!("continue 'l{}; }}", id));
            out
        }
        Ast::Call(name, args) => {
            // emit args first
            let a: Result<Vec<String>, String> = args.iter().map(|x| emit(x, c)).collect();
            let a = a?;
            if let Some(ar) = prim_arity(name) {
                if a.len() != ar {
                    return Err(format!("'{}' expects {} args, got {}", name, ar, a.len()));
                }
                match name.as_str() {
                    "add" => format!("vadd({}, {})", a[0], a[1]),
                    "sub" => format!("vsub({}, {})", a[0], a[1]),
                    "mul" => format!("vmul({}, {})", a[0], a[1]),
                    "div" => format!("vdiv({}, {})", a[0], a[1]),
                    "mod" => format!("vmod({}, {})", a[0], a[1]),
                    "lt" => format!("vlt({}, {})", a[0], a[1]),
                    "dec" => format!("vdec({})", a[0]),
                    _ => unreachable!(),
                }
            } else if let Some(ar) = cord_jet_arity(name) {
                if a.len() != ar {
                    return Err(format!("'{}' expects {} args, got {}", name, ar, a.len()));
                }
                match name.as_str() {
                    "bytes" => format!("vbytes({})", a[0]),
                    "frombytes" => format!("vfrombytes({})", a[0]),
                    "cat" => format!("vcat({}, {})", a[0], a[1]),
                    "catall" => format!("vcatall({})", a[0]),
                    "bytelen" => format!("vbytelen({})", a[0]),
                    _ => unreachable!(),
                }
            } else if c.arms.contains(name) && !c.bound.contains(name) {
                let want = c.arity.get(name).copied().unwrap_or(a.len());
                if a.len() != want {
                    return Err(format!("'{}' expects {} args, got {}", name, want, a.len()));
                }
                format!("arm_{}({})", sanitize(name), a.join(", "))
            } else if c.bound.contains(name) {
                format!("vapply({}.clone(), vec![{}])", sanitize(name), a.join(", "))
            } else {
                return Err(format!("unknown function or gate '{}'", name));
            }
        }
        Ast::Fast(_, body) => emit(body, c)?,
        Ast::Gate(params, body) => {
            // capture enclosing locals by cloning them into the move-closure
            let pset: HashSet<String> = params.iter().cloned().collect();
            let mut fvs = Vec::new();
            free_vars(body, &pset, c.arms, &mut fvs);
            let fvs: Vec<String> = fvs.into_iter().filter(|v| c.bound.contains(v)).collect();
            let mut caps = String::new();
            for v in &fvs {
                caps.push_str(&format!("let {0} = {0}.clone(); ", sanitize(v)));
            }
            // body sees params + the captured locals
            let saved = c.bound.clone();
            for p in params {
                c.bound.insert(p.clone());
            }
            // inside the closure only params + captured frees are in scope
            c.bound = params.iter().cloned().collect();
            for v in &fvs {
                c.bound.insert(v.clone());
            }
            let mut binds = String::new();
            for (i, p) in params.iter().enumerate() {
                binds.push_str(&format!("let {} = __args[{}].clone(); ", sanitize(p), i));
            }
            let bodys = emit(body, c)?;
            c.bound = saved;
            
            format!(
                "{{ {}V::F(std::rc::Rc::new(move |__args: Vec<V>| -> V {{ {}{} }})) }}",
                caps, binds, bodys
            )
        }
    })
}

const PRELUDE: &str = r#"// ---- generated by the Latte->Rust compiler (Anvil) ----
#![allow(dead_code, unused_variables, unused_parens, unused_braces, non_snake_case)]
use std::rc::Rc;

#[derive(Clone)]
enum V { A(u128), Big(Rc<Vec<u8>>), C(Rc<V>, Rc<V>), F(Rc<dyn Fn(Vec<V>) -> V>) }

#[inline] fn na(v: &V) -> u128 { if let V::A(n) = v { *n } else { panic!("arithmetic on non-u128 atom (cord too long for native arithmetic)") } }
#[inline] fn vcell(a: V, b: V) -> V { V::C(Rc::new(a), Rc::new(b)) }
#[inline] fn vhead(v: V) -> V { if let V::C(h, _) = v { (*h).clone() } else { panic!("head of atom") } }
#[inline] fn vtail(v: V) -> V { if let V::C(_, t) = v { (*t).clone() } else { panic!("tail of atom") } }
#[inline] fn viscell(v: V) -> V { if let V::C(..) = v { V::A(0) } else { V::A(1) } }
#[inline] fn viszero(v: &V) -> bool { matches!(v, V::A(0)) }
#[inline] fn vloob(b: bool) -> V { if b { V::A(0) } else { V::A(1) } }
// Arithmetic keeps a u128 fast path (the common case) and falls back to arbitrary-precision
// natural arithmetic (below) the moment a value or result exceeds u128 — so the native backend
// agrees with the interpreter, which treats atoms as unbounded naturals, at ANY width.
#[inline] fn vinc(a: V) -> V { if let V::A(n) = &a { if let Some(r) = n.checked_add(1) { return V::A(r); } } vadd(a, V::A(1)) }
#[inline] fn vdec(a: V) -> V { vsub(a, V::A(1)) }
#[inline] fn vadd(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { if let Some(r) = x.checked_add(*y) { return V::A(r); } }
    vbig(bn_add(&vatom_bytes(&a), &vatom_bytes(&b)))
}
#[inline] fn vsub(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { return V::A(x.checked_sub(*y).expect("sub underflow")); }
    let (ab, bb) = (vatom_bytes(&a), vatom_bytes(&b));
    if bn_cmp(&ab, &bb) == std::cmp::Ordering::Less { panic!("sub underflow"); }
    vbig(bn_sub(&ab, &bb))
}
#[inline] fn vmul(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { if let Some(r) = x.checked_mul(*y) { return V::A(r); } }
    let (ab, bb) = (vatom_bytes(&a), vatom_bytes(&b));
    // multiply by a power of two (e.g. shl, and pow's squaring of 2) is a bit-shift
    if let Some(k) = bn_pow2(&bb) { return vbig(bn_shl_bits(&ab, k)); }
    if let Some(k) = bn_pow2(&ab) { return vbig(bn_shl_bits(&bb, k)); }
    vbig(bn_mul(&ab, &bb))
}
#[inline] fn vdiv(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { return V::A(x.checked_div(*y).expect("div by zero")); }
    bn_divmod_v(&a, &b).0
}
#[inline] fn vmod(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { return V::A(x.checked_rem(*y).expect("mod by zero")); }
    bn_divmod_v(&a, &b).1
}
#[inline] fn vlt(a: V, b: V) -> V {
    if let (V::A(x), V::A(y)) = (&a, &b) { return vloob(x < y); }
    vloob(bn_cmp(&vatom_bytes(&a), &vatom_bytes(&b)) == std::cmp::Ordering::Less)
}
// An atom too big for u128 (a cord > 16 bytes) is kept as its little-endian bytes. Cords are
// base-256 atoms, so the cord ops (bytes/frombytes/cat/catall/bytelen) are jetted to byte-vector
// operations rather than base-256 arithmetic; this lets the native backend handle cords of ANY
// length without a general bignum. `vbig` normalizes (drops trailing zero bytes) and keeps the
// canonical split: <=16 significant bytes live in V::A, longer ones in V::Big, so the two never
// numerically overlap and equality stays a representation compare.
fn vbig(mut b: Vec<u8>) -> V {
    while b.last() == Some(&0) { b.pop(); }
    if b.len() <= 16 {
        let mut n: u128 = 0;
        for (i, &by) in b.iter().enumerate() { n |= (by as u128) << (8 * i); }
        V::A(n)
    } else { V::Big(Rc::new(b)) }
}
fn vatom_bytes(v: &V) -> Vec<u8> {
    match v {
        V::A(n) => { let mut n = *n; let mut o = Vec::new(); while n > 0 { o.push((n & 0xff) as u8); n >>= 8; } o }
        V::Big(b) => (**b).clone(),
        _ => panic!("bytes of non-atom"),
    }
}
fn vbytes(v: V) -> V { let b = vatom_bytes(&v); let mut out = V::A(0); for &by in b.iter().rev() { out = vcell(V::A(by as u128), out); } out }
fn vfrombytes(list: V) -> V { let mut b = Vec::new(); let mut cur = list; while let V::C(h, t) = cur { b.push((na(&h) & 0xff) as u8); cur = (*t).clone(); } vbig(b) }
fn vcat(a: V, b: V) -> V { let mut x = vatom_bytes(&a); x.extend(vatom_bytes(&b)); vbig(x) }
fn vcatall(list: V) -> V { let mut b = Vec::new(); let mut cur = list; while let V::C(h, t) = cur { b.extend(vatom_bytes(&h)); cur = (*t).clone(); } vbig(b) }
fn vbytelen(v: V) -> V { V::A(vatom_bytes(&v).len() as u128) }
// ---- arbitrary-precision natural arithmetic on little-endian byte vectors ------------------
// These back the arithmetic ops above whenever a value exceeds u128, so native results match the
// interpreter's unbounded-natural semantics. The database's bloom filter / hash bitset works on
// wide values via shl/shr/bit, all of which reduce to powers of two — special-cased here to
// byte/bit shifts so a 4096-bit set stays fast; only a non-power-of-two big divisor takes the
// general (binary long-division) path, which database code never hits.
fn bn_sig(a: &[u8]) -> &[u8] { let mut n = a.len(); while n > 0 && a[n - 1] == 0 { n -= 1; } &a[..n] }
fn bn_trim(mut b: Vec<u8>) -> Vec<u8> { while b.last() == Some(&0) { b.pop(); } b }
fn bn_cmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let (a, b) = (bn_sig(a), bn_sig(b));
    if a.len() != b.len() { return a.len().cmp(&b.len()); }
    for i in (0..a.len()).rev() { if a[i] != b[i] { return a[i].cmp(&b[i]); } }
    std::cmp::Ordering::Equal
}
fn bn_add(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(a.len().max(b.len()) + 1);
    let mut carry = 0u16;
    for i in 0..a.len().max(b.len()) {
        let s = *a.get(i).unwrap_or(&0) as u16 + *b.get(i).unwrap_or(&0) as u16 + carry;
        o.push((s & 0xff) as u8); carry = s >> 8;
    }
    if carry > 0 { o.push(carry as u8); }
    bn_trim(o)
}
fn bn_sub(a: &[u8], b: &[u8]) -> Vec<u8> { // requires a >= b
    let mut o = Vec::with_capacity(a.len());
    let mut borrow = 0i16;
    for i in 0..a.len() {
        let mut d = a[i] as i16 - *b.get(i).unwrap_or(&0) as i16 - borrow;
        if d < 0 { d += 256; borrow = 1; } else { borrow = 0; }
        o.push(d as u8);
    }
    bn_trim(o)
}
fn bn_mul(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() || b.is_empty() { return Vec::new(); }
    let mut o = vec![0u8; a.len() + b.len()];
    for i in 0..a.len() {
        let mut carry = 0u32; let ai = a[i] as u32;
        for j in 0..b.len() {
            let cur = o[i + j] as u32 + ai * b[j] as u32 + carry;
            o[i + j] = (cur & 0xff) as u8; carry = cur >> 8;
        }
        let mut k = i + b.len();
        while carry > 0 { let cur = o[k] as u32 + carry; o[k] = (cur & 0xff) as u8; carry = cur >> 8; k += 1; }
    }
    bn_trim(o)
}
fn bn_pow2(a: &[u8]) -> Option<usize> { // Some(k) iff a == 2^k
    let s = bn_sig(a); if s.is_empty() { return None; }
    let top = s.len() - 1;
    if s[top].count_ones() != 1 { return None; }
    for i in 0..top { if s[i] != 0 { return None; } }
    Some(top * 8 + s[top].trailing_zeros() as usize)
}
fn bn_shl_bits(a: &[u8], k: usize) -> Vec<u8> {
    let s = bn_sig(a); if s.is_empty() { return Vec::new(); }
    let (bsh, bish) = (k / 8, (k % 8) as u32);
    let mut o = vec![0u8; bsh];
    let mut carry = 0u16;
    for &x in s { let v = ((x as u16) << bish) | carry; o.push((v & 0xff) as u8); carry = v >> 8; }
    if carry > 0 { o.push(carry as u8); }
    bn_trim(o)
}
fn bn_shr_bits(a: &[u8], k: usize) -> Vec<u8> {
    let s = bn_sig(a); let (bsh, bish) = (k / 8, (k % 8) as u32);
    if bsh >= s.len() { return Vec::new(); }
    let mut o = Vec::with_capacity(s.len() - bsh);
    for i in bsh..s.len() {
        let lo = (s[i] as u16) >> bish;
        let hi = if bish > 0 && i + 1 < s.len() { (s[i + 1] as u16) << (8 - bish) } else { 0 };
        o.push(((lo | hi) & 0xff) as u8);
    }
    bn_trim(o)
}
fn bn_lowbits(a: &[u8], k: usize) -> Vec<u8> { // a mod 2^k
    let s = bn_sig(a); let (bsh, bish) = (k / 8, (k % 8) as u32);
    let take = bsh + if bish > 0 { 1 } else { 0 };
    let mut o: Vec<u8> = s.iter().take(take).cloned().collect();
    if bish > 0 && o.len() == bsh + 1 { o[bsh] &= ((1u16 << bish) - 1) as u8; }
    bn_trim(o)
}
fn bn_divmod(a: &[u8], b: &[u8]) -> (Vec<u8>, Vec<u8>) {
    if let Some(k) = bn_pow2(b) { return (bn_shr_bits(a, k), bn_lowbits(a, k)); }
    match bn_cmp(a, b) {
        std::cmp::Ordering::Less => return (Vec::new(), bn_sig(a).to_vec()),
        std::cmp::Ordering::Equal => return (vec![1], Vec::new()),
        _ => {}
    }
    let a = bn_sig(a); let nbits = a.len() * 8;
    let mut q = vec![0u8; a.len()];
    let mut r: Vec<u8> = Vec::new();
    for bit in (0..nbits).rev() {
        r = bn_shl_bits(&r, 1);
        if (a[bit / 8] >> (bit % 8)) & 1 == 1 { if r.is_empty() { r.push(1); } else { r[0] |= 1; } }
        if bn_cmp(&r, b) != std::cmp::Ordering::Less { r = bn_sub(&r, b); q[bit / 8] |= 1 << (bit % 8); }
    }
    (bn_trim(q), bn_trim(r))
}
fn bn_divmod_v(a: &V, b: &V) -> (V, V) {
    let bb = vatom_bytes(b);
    if bn_sig(&bb).is_empty() { panic!("div by zero"); }
    let (q, r) = bn_divmod(&vatom_bytes(a), &bb);
    (vbig(q), vbig(r))
}
fn veq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::A(x), V::A(y)) => x == y,
        (V::Big(x), V::Big(y)) => x == y,
        (V::C(h1, t1), V::C(h2, t2)) => veq(h1, h2) && veq(t1, t2),
        _ => false,
    }
}
fn vapply(f: V, args: Vec<V>) -> V { if let V::F(g) = f { g(args) } else { panic!("apply of non-function") } }
// Canonical, unambiguous readout: atoms as decimal, cells as [h t]. Each host (CLI, GUI, REPL)
// parses this back into a noun and re-renders it in its own style.
fn vrender(v: &V) -> String {
    match v {
        V::A(n) => n.to_string(),
        V::Big(b) => { let mut s = String::from("~"); for by in b.iter() { s.push_str(&format!("{:02x}", by)); } s }
        V::C(h, t) => format!("[{} {}]", vrender(h), vrender(t)),
        V::F(_) => "<fn>".to_string(),
    }
}
// Inverse of vrender: read a canonical noun (decimal atoms, ~hex long-cord atoms, [h t]
// cells) supplied at run time on stdin, so one compiled binary can serve many inputs.
fn vparse(s: &str) -> V {
    let t: Vec<char> = s.trim().chars().collect();
    let mut p = 0usize;
    fn skip(t: &[char], p: &mut usize) { while *p < t.len() && t[*p].is_whitespace() { *p += 1; } }
    fn go(t: &[char], p: &mut usize) -> V {
        skip(t, p);
        if *p >= t.len() { return V::A(0); }
        if t[*p] == '[' {
            *p += 1;
            let h = go(t, p);
            let tl = go(t, p);
            skip(t, p);
            if *p < t.len() && t[*p] == ']' { *p += 1; }
            vcell(h, tl)
        } else if t[*p] == '~' {
            *p += 1;
            let st = *p;
            while *p < t.len() && t[*p].is_ascii_hexdigit() { *p += 1; }
            let hb: Vec<char> = t[st..*p].to_vec();
            let mut b = Vec::with_capacity(hb.len() / 2);
            let mut i = 0;
            while i + 2 <= hb.len() {
                let hi = hb[i].to_digit(16).unwrap_or(0);
                let lo = hb[i + 1].to_digit(16).unwrap_or(0);
                b.push((hi * 16 + lo) as u8);
                i += 2;
            }
            vbig(b)
        } else {
            let st = *p;
            while *p < t.len() && t[*p].is_ascii_digit() { *p += 1; }
            let ds: String = t[st..*p].iter().collect();
            V::A(ds.parse::<u128>().unwrap_or(0))
        }
    }
    go(&t, &mut p)
}
"#;

/// Compile a Latte expression (with its library closure) to a standalone Rust program.
pub fn compile_to_rust(expr_src: &str, libs: &[&str]) -> Result<String, String> {
    compile_to_rust_opts(expr_src, libs, None)
}

/// Compile a Latte expression to standalone Rust. When `input_param` is `Some(name)`,
/// `__main` takes that parameter and `main()` reads a canonical noun from stdin, so the
/// SAME binary can be run against many inputs (the program is content-addressed without
/// the input). When `None`, the program is self-contained and `main()` runs it directly.
fn compile_to_rust_opts(
    expr_src: &str,
    libs: &[&str],
    input_param: Option<&str>,
) -> Result<String, String> {
    compile_to_rust_full(expr_src, libs, input_param, false)
}

/// Like `compile_to_rust_opts(.., Some("__in"))`, but `main()` is a *resident loop*: it reads
/// length-framed canonical nouns from stdin, applies `__main` to each, and writes a length-framed
/// result — so one long-lived process can serve many requests, amortizing process startup. Each
/// request is independent (the generated code is pure), so the result is identical to the one-shot
/// binary; this is purely a startup-cost optimization.
fn compile_to_rust_worker(expr_src: &str, libs: &[&str]) -> Result<String, String> {
    compile_to_rust_full(expr_src, libs, Some("__in"), true)
}

fn compile_to_rust_full(
    expr_src: &str,
    libs: &[&str],
    input_param: Option<&str>,
    loop_main: bool,
) -> Result<String, String> {
    let program = latte::gather_program_in(expr_src, libs, input_param.unwrap_or("_"))?;
    // The set of all top-level arm names. A binder (parameter, let, or loop variable) that
    // collides with one of these shadows the arm, and must be alpha-renamed for the native
    // backend, exactly as primitive/keyword collisions are — otherwise a closure that
    // captures such a local drops it (free_vars treats arm names as global) and emits an
    // unbound variable.
    let arm_name_set: HashSet<String> = program.iter().map(|(n, _, _)| n.clone()).collect();
    // A local that shadows a primitive name (e.g. `sub` as a substring index) or an arm name
    // (e.g. `field`, an arm in ui.lat) used to force the whole program onto the interpreter.
    // Instead, alpha-rename such binders — including arm parameters — to fresh names so
    // native compilation still applies.
    let program: Vec<(String, Vec<String>, Ast)> = program
        .into_iter()
        .map(|(n, params, b)| {
            let mut ctr = 0usize;
            let mut env: HashMap<String, String> = HashMap::new();
            let params2 = params
                .iter()
                .map(|p| {
                    if binder_rename(p) || arm_name_set.contains(p) {
                        let f = fresh(p, &mut ctr);
                        env.insert(p.clone(), f.clone());
                        f
                    } else {
                        p.clone()
                    }
                })
                .collect();
            let b2 = unshadow(&b, &env, &mut ctr, &arm_name_set);
            (n, params2, b2)
        })
        .collect();
    // fold constants in every arm
    let program: Vec<Arm> = program
        .into_iter()
        .map(|(n, p, b)| (n, p, fold(&b)))
        .collect();

    let arm_names: HashSet<String> = program.iter().map(|(n, _, _)| n.clone()).collect();
    let mut arity: HashMap<String, usize> = HashMap::new();
    let mut body_of: HashMap<String, &Arm> = HashMap::new();
    for arm in &program {
        arity.insert(arm.0.clone(), arm.1.len());
        body_of.insert(arm.0.clone(), arm);
    }

    // reachability from __main, short-circuiting native primitives
    let mut reachable: HashSet<String> = HashSet::new();
    let mut work = vec!["__main".to_string()];
    while let Some(name) = work.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some((_, _, body)) = body_of.get(&name) {
            let mut calls = Vec::new();
            collect_calls(body, &mut calls);
            for cnm in calls {
                if !is_native_op(&cnm) && arm_names.contains(&cnm) && !reachable.contains(&cnm)
                {
                    work.push(cnm);
                }
            }
        }
    }

    let mut out = String::new();
    out.push_str(PRELUDE);
    out.push('\n');

    // emit each reachable arm as a Rust function
    let mut sorted: Vec<&Arm> = program.iter().filter(|a| reachable.contains(&a.0)).collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, params, body) in sorted {
        let mut c = Ctx {
            arms: &arm_names,
            arity: &arity,
            bound: params.iter().cloned().collect(),
            loops: Vec::new(),
            tmp: 0,
        };
        let psig: Vec<String> = params.iter().map(|p| format!("{}: V", sanitize(p))).collect();
        let bodys = emit(body, &mut c)?;
        out.push_str(&format!(
            "fn arm_{}({}) -> V {{ {} }}\n",
            sanitize(name),
            psig.join(", "),
            bodys
        ));
    }

    let main_src = if loop_main {
        r#"
fn main() {
    use std::io::{Write, BufRead, Read};
    let inp = std::io::stdin();
    let mut inp = inp.lock();
    let out = std::io::stdout();
    let mut out = out.lock();
    let mut header = String::new();
    loop {
        header.clear();
        match inp.read_line(&mut header) { Ok(0) | Err(_) => break, _ => {} }
        let n: usize = match header.trim().parse() { Ok(n) => n, Err(_) => break };
        let mut buf = vec![0u8; n];
        if inp.read_exact(&mut buf).is_err() { break; }
        let s = String::from_utf8_lossy(&buf);
        let r = arm___main(vparse(&s));
        let rendered = vrender(&r);
        if write!(out, "{}\n{}", rendered.len(), rendered).is_err() { break; }
        if out.flush().is_err() { break; }
    }
}
"#
        .to_string()
    } else if input_param.is_some() {
        "\nfn main() {\n    use std::io::Read;\n    let mut s = String::new();\n    let _ = std::io::stdin().read_to_string(&mut s);\n    let r = arm___main(vparse(&s));\n    println!(\"{}\", vrender(&r));\n}\n".to_string()
    } else {
        "\nfn main() {\n    let r = arm___main(V::A(0));\n    println!(\"{}\", vrender(&r));\n}\n".to_string()
    };
    out.push_str(&main_src);
    Ok(out)
}

fn collect_calls(ast: &Ast, out: &mut Vec<String>) {
    match ast {
        Ast::Call(n, args) => {
            out.push(n.clone());
            for a in args {
                collect_calls(a, out);
            }
        }
        Ast::Inc(e) | Ast::Head(e) | Ast::Tail(e) | Ast::IsCell(e) | Ast::Fast(_, e) => {
            collect_calls(e, out)
        }
        Ast::Eq(a, b) => {
            collect_calls(a, out);
            collect_calls(b, out);
        }
        Ast::If(c, t, e) => {
            collect_calls(c, out);
            collect_calls(t, out);
            collect_calls(e, out);
        }
        Ast::Let(_, v, b) => {
            collect_calls(v, out);
            collect_calls(b, out);
        }
        Ast::Tuple(es) | Ast::Again(es) => {
            for e in es {
                collect_calls(e, out);
            }
        }
        Ast::Loop(binds, b) => {
            for (_, e) in binds {
                collect_calls(e, out);
            }
            collect_calls(b, out);
        }
        Ast::Case(s, cs) => {
            collect_calls(s, out);
            for (_, e) in cs {
                collect_calls(e, out);
            }
        }
        Ast::Gate(_, b) => collect_calls(b, out),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Native execution: compile → build (cached by source hash) → run → parse back.
// This is the integration point: every eval surface (CLI, GUI, REPL) runs through here and then
// renders the returned noun in its own style.
// ---------------------------------------------------------------------------

/// Parse the canonical readout emitted by a compiled program (atoms decimal, cells `[h t]`) back
/// into a noun.
pub(crate) fn parse_canon(s: &str) -> Option<crate::knot::N> {
    let toks: Vec<char> = s.trim().chars().collect();
    let mut pos = 0usize;
    fn skip_ws(t: &[char], p: &mut usize) {
        while *p < t.len() && t[*p].is_whitespace() {
            *p += 1;
        }
    }
    fn parse(t: &[char], p: &mut usize) -> Option<crate::knot::N> {
        skip_ws(t, p);
        if *p >= t.len() {
            return None;
        }
        if t[*p] == '[' {
            *p += 1;
            let h = parse(t, p)?;
            let tl = parse(t, p)?;
            skip_ws(t, p);
            if *p >= t.len() || t[*p] != ']' {
                return None;
            }
            *p += 1;
            Some(crate::knot::cell(h, tl))
        } else if t[*p].is_ascii_digit() {
            let start = *p;
            while *p < t.len() && t[*p].is_ascii_digit() {
                *p += 1;
            }
            let n: u128 = t[start..*p].iter().collect::<String>().parse().ok()?;
            Some(crate::knot::num(n))
        } else if t[*p] == '~' {
            // a long-cord atom: ~<little-endian hex bytes>
            *p += 1;
            let start = *p;
            while *p < t.len() && t[*p].is_ascii_hexdigit() {
                *p += 1;
            }
            let hex: String = t[start..*p].iter().collect();
            if hex.len() % 2 != 0 {
                return None;
            }
            let mut bytes = Vec::with_capacity(hex.len() / 2);
            let hb = hex.as_bytes();
            let mut i = 0;
            while i < hb.len() {
                let byte = u8::from_str_radix(std::str::from_utf8(&hb[i..i + 2]).ok()?, 16).ok()?;
                bytes.push(byte);
                i += 2;
            }
            Some(crate::knot::atom(crate::atom::Atom::from_bytes_le(bytes)))
        } else {
            None // e.g. "<fn>" — not a representable noun
        }
    }
    let v = parse(&toks, &mut pos)?;
    Some(v)
}

/// Compile `expr` with Anvil, build it (caching the binary by a hash of the emitted source so
/// repeats are instant), run it, and return the resulting noun — or `None` if any stage fails
/// (so callers fall back to the interpreter). On success the noun is identical to the
/// interpreter's, so callers may render it however they like.
pub fn run_native_noun(expr: &str, libs: &[&str]) -> Option<crate::knot::N> {
    run_native_noun_opts(expr, libs, false)
}

/// Like `run_native_noun`, but `force_rebuild` ignores any cached binary and recompiles.
/// The `rustc` optimization level for native builds. Defaults to `0`: for these
/// one-shot, content-addressed programs the build time dominates the (sub-second)
/// runtime, so opt-level 0 cuts cold-start by ~9x (≈0.7s vs ≈6.3s on the 614-rule
/// Ligurian evolve) while the binary still runs several times faster than the
/// interpreter. Set `ORPHEUS_OPT=2` (or `3`) for hot, long-lived programs where
/// runtime dominates and the one-time build cost is worth it.
fn rustc_opt_level(src: &str) -> String {
    if let Ok(o) = std::env::var("ORPHEUS_OPT") {
        if !o.is_empty() {
            return o;
        }
    }
    // Optimized compilation is the DEFAULT for heavy code, system-wide: this is the single
    // chokepoint every native build flows through (eval, the advisor, the dashboards, the
    // server, the daemon). A large emitted program — the database (LSM + bloom + indexes) or
    // the financial-ML stack (a DB build plus thousands of training iterations) — runs long
    // enough that optimizing is a net win even on the FIRST build: a 4000-iteration logistic
    // fit goes from ~19s to ~5s, so build+run drops too, and every cached run afterward is
    // ~4x faster. `-O1` captures nearly all of `-O2`/`-O3`'s speedup here at the lowest build
    // cost. Trivial programs stay at `-O0`, where the (tiny) build time dominates and there is
    // nothing to optimize. The level is a deterministic function of the source, so it never
    // collides in the content-addressed cache. `ORPHEUS_OPT` overrides for benchmarking.
    if src.len() > 24_000 { "1".to_string() } else { "0".to_string() }
}

/// Compile emitted Rust `src` into `bin` (atomically, via a temp file + rename), under a
/// global build lock so concurrent identical requests compile once. Returns whether `bin`
/// is present and usable afterwards. The single point where the native toolchain is invoked.
/// The CROSS-PROCESS build lock: rustc invocations from every process sharing a
/// cache dir (the server, detached warm children, the anvil daemon, a CLI eval)
/// serialize on one lock file. One compiler at a time keeps a small machine
/// responsive when a widget-heavy page warms a virgin cache — the burst becomes
/// a queue instead of N concurrent rustcs. Stale locks (a build killed mid-rustc)
/// are broken after 10 minutes; the guard removes the file on drop, panics
/// included.
struct BuildFileLock(std::path::PathBuf);
impl Drop for BuildFileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn acquire_build_file_lock() -> Option<BuildFileLock> {
    let path = cache_dir().join("build.lock");
    let _ = std::fs::create_dir_all(cache_dir());
    for _ in 0..3000 {
        // ~10 minutes of patience
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = write!(f, "{}", std::process::id());
                return Some(BuildFileLock(path));
            }
            Err(_) => {
                // break a stale lock: its holder died mid-build
                let stale = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|age| age.as_secs() > 600)
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
    None // could not acquire: proceed unlocked rather than never building
}

fn build_native(src: &str, bin: &std::path::Path, force: bool) -> bool {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CTR: AtomicUsize = AtomicUsize::new(0);
    static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _file_guard = acquire_build_file_lock();
    if !force && bin.exists() {
        return true; // built by another thread OR PROCESS while we waited
    }
    // Read-through: a same-toolchain host may have already built this exact program and published
    // it — to the shared dir, or to the networked registry (signature-verified). Either skips rustc.
    if !force && (shared_pull(bin) || registry_pull(bin)) {
        record_pull();
        evict_to_cap();
        return true;
    }
    let dir = match bin.parent() {
        Some(d) => d.to_path_buf(),
        None => return false,
    };
    let uniq = CTR.fetch_add(1, Ordering::Relaxed);
    let tag = format!("{}-{}", std::process::id(), uniq);
    let rs = dir.join(format!("build-{}.rs", tag));
    let tmp_bin = dir.join(format!("build-{}{}", tag, BIN_EXT));
    if std::fs::write(&rs, src).is_err() {
        return false;
    }
    let build_t0 = std::time::Instant::now();
    let st = std::process::Command::new("rustc")
        .args(["-C", &format!("opt-level={}", rustc_opt_level(src)), "--edition", "2021", "-o"])
        .arg(&tmp_bin)
        .arg(&rs)
        .output();
    let build_ms = build_t0.elapsed().as_millis() as u64;
    let _ = std::fs::remove_file(&rs);
    match st {
        Ok(s) if s.status.success() => {
            let _ = std::fs::rename(&tmp_bin, bin);
            record_build(build_ms);
            write_sidecar(bin); // record bytes hash+size for later integrity checks
            // Write-back: publish so peer hosts can pull instead of rebuilding — to the shared dir
            // and, if configured, the networked registry (signed under the shared key).
            shared_push(bin);
            registry_push(bin);
            // Self-bound the cache: with the new binary in place, evict least-recently-used
            // binaries if we're over the size cap. Done here (under BUILD_LOCK) so eviction
            // never races a concurrent build.
            evict_to_cap();
            true
        }
        Ok(s) => {
            let _ = std::fs::remove_file(&tmp_bin);
            // A program that passes `compile_to_rust` but fails `rustc` is a codegen bug, not a
            // user error — capture the compiler's stderr so it's diagnosable rather than a silent
            // fall-through to the interpreter.
            log_build_failure(bin, &String::from_utf8_lossy(&s.stderr));
            false
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_bin);
            log_build_failure(bin, &format!("could not launch rustc: {}", e));
            false
        }
    }
}

fn build_log_path() -> std::path::PathBuf {
    cache_dir().join("build-errors.log")
}

/// Return the suffix of `s` no longer than `max` bytes, starting at a line boundary (so entries are
/// never cut mid-line). Pure, so the log-trimming policy is unit-testable.
fn tail_from_line_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let start = s.len() - max;
    match s[start..].find('\n') {
        Some(i) => &s[start + i + 1..],
        None => &s[start..],
    }
}

/// Append a build failure (timestamped, with a short stderr excerpt) to the cache's error log,
/// keeping the log bounded. Best-effort; diagnostics must never affect the build result.
fn log_build_failure(bin: &std::path::Path, stderr: &str) {
    const LOG_CAP: usize = 64 * 1024;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Keep only the first dozen lines of stderr — enough to identify the error.
    let excerpt: String = stderr.lines().take(12).collect::<Vec<_>>().join("\n");
    let entry = format!("[t={}] {}\n{}\n----\n", secs, name_of(bin), excerpt);
    let path = build_log_path();
    let mut prior = std::fs::read_to_string(&path).unwrap_or_default();
    prior.push_str(&entry);
    let trimmed = tail_from_line_boundary(&prior, LOG_CAP);
    let _ = std::fs::write(&path, trimmed);
    record_build_failure();
}

/// The tail of the build-error log (most recent failures), up to `max_bytes`. Empty if none.
pub fn build_log_tail(max_bytes: usize) -> String {
    let s = std::fs::read_to_string(build_log_path()).unwrap_or_default();
    tail_from_line_boundary(&s, max_bytes).to_string()
}

/// Static native-compilation diagnosis: `Ok(())` if the program lowers to native code, otherwise
/// `Err(reason)` explaining why it would run on the interpreter instead (e.g. an unsupported
/// construct or an arity error). Fast — this is the `compile_to_rust` front-end, no `rustc`.
pub fn native_check(expr: &str, libs: &[&str]) -> Result<(), String> {
    if !rustc_available() {
        return Err("no rustc on this device (the interpreter + JIT is the engine; cached binaries still run)".into());
    }
    compile_to_rust(expr, libs).map(|_| ())
}

/// Lifetime build/cache counters, persisted in the cache dir so they accumulate across one-shot CLI
/// invocations. Approximate under heavy concurrency (read-modify-write, last-writer-wins) — they are
/// advisory metrics, not exact accounting.
#[derive(Default, Clone, Copy)]
pub struct Metrics {
    pub builds: u64,         // rustc compilations performed
    pub build_ms_total: u64, // cumulative rustc wall time
    pub hits: u64,           // cached binary reused without building
    pub pulls: u64,          // binary fetched from a remote (shared store or registry); build skipped
    pub build_failures: u64, // rustc declined (logged to build-errors.log)
}

impl Metrics {
    pub fn avg_build_ms(&self) -> u64 {
        if self.builds == 0 { 0 } else { self.build_ms_total / self.builds }
    }
    /// Estimated `rustc` time avoided: each hit or pull would otherwise have cost ~one build.
    pub fn est_saved_ms(&self) -> u64 {
        self.avg_build_ms().saturating_mul(self.hits + self.pulls)
    }
}

fn parse_metrics(s: &str) -> Metrics {
    let mut m = Metrics::default();
    for line in s.lines() {
        let mut it = line.split_whitespace();
        if let (Some(k), Some(v)) = (it.next(), it.next()) {
            let v: u64 = v.parse().unwrap_or(0);
            match k {
                "builds" => m.builds = v,
                "build_ms_total" => m.build_ms_total = v,
                "hits" => m.hits = v,
                "pulls" => m.pulls = v,
                "build_failures" => m.build_failures = v,
                _ => {}
            }
        }
    }
    m
}

fn format_metrics(m: &Metrics) -> String {
    format!(
        "builds {}\nbuild_ms_total {}\nhits {}\npulls {}\nbuild_failures {}\n",
        m.builds, m.build_ms_total, m.hits, m.pulls, m.build_failures
    )
}

fn metrics_path() -> std::path::PathBuf {
    cache_dir().join("metrics")
}

// ---------------------------------------------------------------------------
// The PROFILER: measured engine selection.
//
// The adaptive policy's structural heuristic (`worth_compiling`) guesses from
// the AST; the profiler REPLACES the guess with a measurement wherever one
// exists. Every interpreter run of a program is timed and recorded (keyed by a
// hash of the program text + its library scope, persisted in the cache dir so
// measurements span one-shot CLI runs). A program whose measured interpreter
// time crosses the threshold is compiled automatically before its next run —
// through the resident daemon when one is up (no stall), else synchronously.
// `latte profile "<expr>"` runs both engines, reports the measured speedup,
// and states the decision the adaptive engine will now take.
//
// Threshold: ORPHEUS_PROFILE_NS (default 1.5ms). Below it, interpreting is so
// cheap that a build could never pay for itself; above it, the native run's
// severalfold speedup compounds across repeats. Entries are advisory — a stale
// timing can only mis-schedule a build, never change a result.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct Profile {
    pub interp_ns: u64, // latest measured interpreter wall time
    pub native_ns: u64, // latest measured native wall time (0 = never measured)
    pub runs: u64,      // interpreter runs recorded
    pub dist: bool,     // profiler detected a distributable (data-parallel) shape
}

pub fn profile_threshold_ns() -> u64 {
    std::env::var("ORPHEUS_PROFILE_NS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_500_000)
}

fn profile_path() -> std::path::PathBuf {
    cache_dir().join("profile.tsv")
}

fn profile_key(expr: &str, libs: &[&str]) -> String {
    let mut ls: Vec<&str> = libs.to_vec();
    ls.sort_unstable();
    let seed = format!("{}\u{1}{}", expr, ls.join(","));
    crate::sha3::hex(&crate::sha3::sha3_256(seed.as_bytes()))[..32].to_string()
}

fn profile_load() -> std::collections::HashMap<String, Profile> {
    let mut m = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(profile_path()) {
        for line in s.lines() {
            let mut it = line.split('\t');
            if let (Some(k), Some(i), Some(n), Some(r)) = (it.next(), it.next(), it.next(), it.next()) {
                m.insert(
                    k.to_string(),
                    Profile {
                        interp_ns: i.parse().unwrap_or(0),
                        native_ns: n.parse().unwrap_or(0),
                        runs: r.parse().unwrap_or(0),
                        // 5th column (older stores lack it): distributable flag
                        dist: it.next().map(|d| d == "1").unwrap_or(false),
                    },
                );
            }
        }
    }
    m
}

/// Keep the on-disk profile store bounded: it is rewritten whole on change, so
/// letting one line per distinct expression accumulate forever would slowly
/// turn every recorded measurement into an O(n) disk write. Past the cap,
/// single-run entries far below the compile threshold go first (they encode no
/// decision); if that is not enough, the coldest half is dropped wholesale.
fn prune_profiles(m: &mut std::collections::HashMap<String, Profile>) {
    const CAP: usize = 4096;
    if m.len() <= CAP {
        return;
    }
    let thresh = profile_threshold_ns();
    m.retain(|_, p| p.runs > 1 || p.interp_ns * 4 >= thresh);
    if m.len() > CAP {
        let mut runs: Vec<u64> = m.values().map(|p| p.runs).collect();
        runs.sort_unstable();
        let median = runs[runs.len() / 2];
        m.retain(|_, p| p.runs >= median);
    }
}

fn profile_store(m: &std::collections::HashMap<String, Profile>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let _ = std::fs::create_dir_all(cache_dir());
    let mut s = String::new();
    for (k, p) in m {
        s.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            k,
            p.interp_ns,
            p.native_ns,
            p.runs,
            if p.dist { 1 } else { 0 }
        ));
    }
    let p = profile_path();
    let tmp = p.with_file_name(format!("profile-{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    if std::fs::write(&tmp, s).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// The stored measurement for a program, if any.
pub fn profile_lookup(expr: &str, libs: &[&str]) -> Option<Profile> {
    profile_load().get(&profile_key(expr, libs)).copied()
}

/// Record an interpreter timing (exponential smoothing so one outlier run
/// does not swing the decision; the store stays one small line per program).
pub fn profile_record_interp(expr: &str, libs: &[&str], ns: u64) {
    let key = profile_key(expr, libs);
    let mut m = profile_load();
    let before = m.get(&key).map(|e| e.interp_ns);
    let e = m.entry(key).or_default();
    e.interp_ns = if e.runs == 0 { ns } else { (e.interp_ns * 3 + ns) / 4 };
    e.runs += 1;
    // Write back only when the smoothed value MOVED (>=5% or first sighting):
    // the store is rewritten whole, and an interactive session measures many
    // runs whose smoothing changes nothing decision-relevant.
    let moved = match before {
        None => true,
        Some(b) => {
            let d = if e.interp_ns > b { e.interp_ns - b } else { b - e.interp_ns };
            d * 20 >= b.max(1)
        }
    };
    if moved {
        prune_profiles(&mut m);
        profile_store(&m);
    }
}

/// Record a native timing for the same program.
pub fn profile_record_native(expr: &str, libs: &[&str], ns: u64) {
    let key = profile_key(expr, libs);
    let mut m = profile_load();
    let e = m.entry(key).or_default();
    e.native_ns = if e.native_ns == 0 { ns } else { (e.native_ns * 3 + ns) / 4 };
    profile_store(&m);
}

/// Record that the profiler detected a distributable (data-parallel) shape in
/// this program — the adaptive engine's distribution decision reads it back.
pub fn profile_record_dist(expr: &str, libs: &[&str]) {
    let key = profile_key(expr, libs);
    let mut m = profile_load();
    let e = m.entry(key).or_default();
    if !e.dist {
        e.dist = true;
        profile_store(&m);
    }
}

/// `latte profile "<expr>"` — run the program on BOTH engines, measure, persist,
/// and report the decision the adaptive engine will now take for it.
pub fn profile_report(expr: &str, libs: &[&str]) -> Result<String, String> {
    // Warm the library scope first: the first run_with_libs in a process pays one-time
    // library gathering/compilation, which would otherwise be billed to the expression.
    let _ = crate::latte::run_with_libs("0", libs);
    // Measure the per-call scope baseline (linking the standard scope around a trivial
    // body). Subtracting it prices the EXPRESSION, not the scope — otherwise, in a large
    // scope, even `(add 2 3)` would look worth compiling.
    let tb = std::time::Instant::now();
    let _ = crate::latte::run_with_libs("0", libs);
    let base_ns = tb.elapsed().as_nanos() as u64;
    // best-of-three: on a virtualized core a single run carries milliseconds of
    // scheduler noise; the minimum is the standard low-noise estimator
    let mut interp_total = u64::MAX;
    let mut iv = crate::latte::run_with_libs(expr, libs)?;
    for _ in 0..3 {
        let t0 = std::time::Instant::now();
        iv = crate::latte::run_with_libs(expr, libs)?;
        interp_total = interp_total.min(t0.elapsed().as_nanos() as u64);
    }
    let interp_ns = interp_total.saturating_sub(base_ns).max(1);
    profile_record_interp(expr, libs, interp_ns);
    let (native_line, native_ns) = {
        let t1 = std::time::Instant::now();
        match run_native_noun(expr, libs) {
            Some(nv) => {
                let ns = t1.elapsed().as_nanos() as u64;
                profile_record_native(expr, libs, ns);
                if nv != iv {
                    return Err("engines DISAGREE — please report this program (latte anvil shrink)".into());
                }
                // warm run: the binary is now cached, so time a second, build-free run
                let t2 = std::time::Instant::now();
                let _ = run_native_cached(expr, libs);
                let warm_ns = t2.elapsed().as_nanos() as u64;
                profile_record_native(expr, libs, warm_ns);
                (format!(
                    "  native      {:>10.3} ms cold (build+run)   {:>10.3} ms warm (cached)",
                    ns as f64 / 1e6,
                    warm_ns as f64 / 1e6
                ), warm_ns)
            }
            None => (
                if rustc_available() {
                    "  native      — (outside the native subset; interpreter is the engine)".into()
                } else {
                    "  native      — (no rustc on this device; the interpreter + JIT is the engine)".to_string()
                },
                0,
            ),
        }
    };
    let threshold = profile_threshold_ns();
    let decision = if native_ns == 0 {
        "interpret (no native path)".to_string()
    } else if interp_ns >= threshold {
        format!(
            "compile — measured interpreter time {:.3} ms ≥ {:.1} ms threshold ({:.1}x faster warm)",
            interp_ns as f64 / 1e6,
            threshold as f64 / 1e6,
            interp_ns as f64 / native_ns.max(1) as f64
        )
    } else {
        format!(
            "interpret — measured {:.3} ms < {:.1} ms threshold (a build would not pay)",
            interp_ns as f64 / 1e6,
            threshold as f64 / 1e6
        )
    };
    // Distribution: the profiler also detects data-parallel shapes — the
    // measured time then drives the DEFAULT distribute-or-stay-local decision
    // exactly as it drives compile-or-interpret (src/dist.rs).
    let dist_lines = match crate::dist::profile_note(expr, libs, interp_ns) {
        Some(note) => {
            profile_record_dist(expr, libs);
            format!("\n{}", note)
        }
        None => String::new(),
    };
    Ok(format!(
        "profile: {}\n  interpreter {:>10.3} ms (expression; scope baseline {:.3} ms subtracted)\n{}\n  adaptive decision: {}{}\n  (persisted — the adaptive engine uses this measurement from now on)",
        expr,
        interp_ns as f64 / 1e6,
        base_ns as f64 / 1e6,
        native_line,
        decision,
        dist_lines
    ))
}

/// The current counters (zeroes if none recorded yet).
pub fn metrics_snapshot() -> Metrics {
    parse_metrics(&std::fs::read_to_string(metrics_path()).unwrap_or_default())
}

fn metrics_bump(f: impl FnOnce(&mut Metrics)) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let _ = std::fs::create_dir_all(cache_dir());
    let mut m = metrics_snapshot();
    f(&mut m);
    let p = metrics_path();
    let tmp = p.with_file_name(format!(
        "metrics-{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::write(&tmp, format_metrics(&m)).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn record_build(ms: u64) {
    metrics_bump(|m| {
        m.builds += 1;
        m.build_ms_total += ms;
    });
}
fn record_hit() {
    metrics_bump(|m| m.hits += 1);
}
fn record_pull() {
    metrics_bump(|m| m.pulls += 1);
}
fn record_build_failure() {
    metrics_bump(|m| m.build_failures += 1);
}


/// Make a filesystem-safe path component (alphanumerics and `.`/`-`/`_` kept, everything else
/// becomes `-`). Pure, so the toolchain-id construction is unit-testable.
fn sanitize_component(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect()
}

/// A stable identity for the local toolchain — `rustc` release plus host target triple — derived
/// once from `rustc -vV`. Compiled binaries are only portable between hosts that share this id, so
/// the shared store namespaces every artifact under it; a host never pulls a binary it can't run.
/// Is a `rustc` on PATH? Probed once per process. On devices without a
/// toolchain — an Android phone running a sideloaded binary, a stripped
/// container — the native engine stands down cleanly: already-cached binaries
/// still run, new builds are skipped without an error per call, and the
/// interpreter + JIT (pure Rust closures, any CPU) answer everything.
pub fn rustc_available() -> bool {
    static AVAIL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAIL.get_or_init(|| {
        std::process::Command::new("rustc")
            .arg("-vV")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// (Builds use `opt-level=N` with no `target-cpu=native`, so codegen stays portable across CPUs of
/// the same triple.)
pub fn toolchain_id() -> &'static str {
    static TID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TID.get_or_init(|| {
        let (mut release, mut host) = (String::new(), String::new());
        if let Ok(o) = std::process::Command::new("rustc").arg("-vV").output() {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Some(v) = line.strip_prefix("release: ") {
                        release = v.trim().to_string();
                    } else if let Some(v) = line.strip_prefix("host: ") {
                        host = v.trim().to_string();
                    }
                }
            }
        }
        if release.is_empty() {
            release = "unknown".into();
        }
        if host.is_empty() {
            host = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
        }
        sanitize_component(&format!("rustc-{}-{}", release, host))
    })
    .as_str()
}

/// The shared artifact store directory, if `ORPHEUS_CACHE_SHARED` is set. A read-through/write-back
/// mirror of the local cache that several hosts (CI runners, a team, a second machine) can share so
/// each distinct program is compiled by `rustc` once across the fleet rather than once per host.
pub fn shared_store_dir() -> Option<std::path::PathBuf> {
    match std::env::var("ORPHEUS_CACHE_SHARED") {
        Ok(s) if !s.trim().is_empty() => Some(std::path::PathBuf::from(s.trim())),
        _ => None,
    }
}

/// Where `bin` lives in the shared store: `<store>/<toolchain-id>/<binary-filename>`.
fn shared_slot(bin: &std::path::Path) -> Option<std::path::PathBuf> {
    Some(shared_store_dir()?.join(toolchain_id()).join(bin.file_name()?))
}

/// Read-through: if a matching-toolchain binary exists in the shared store, copy it into the local
/// cache at `bin` (atomically) and report success — no `rustc`. The pulled bytes are verified
/// against the pulled sidecar before install, so a corrupt or poisoned store entry is rejected
/// (falling through to a local build) rather than infecting this host. Best-effort throughout.
fn shared_pull(bin: &std::path::Path) -> bool {
    let slot = match shared_slot(bin) {
        Some(s) if s.exists() => s,
        _ => return false,
    };
    let dir = match bin.parent() {
        Some(d) => d,
        None => return false,
    };
    let tmp = dir.join(format!("pull-{}-{}", std::process::id(), name_of(bin)));
    if std::fs::copy(&slot, &tmp).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    // Pull the sidecar alongside and verify the pulled bytes against it. A store entry without a
    // sidecar is unverifiable — reject it, since the whole point here is integrity across hosts.
    let tmp_sc = sidecar_path(&tmp);
    let slot_sc = sidecar_path(&slot);
    let verified = std::fs::copy(&slot_sc, &tmp_sc).is_ok() && verify_file(&tmp) == Integrity::Ok;
    if !verified {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&tmp_sc);
        return false;
    }
    let _ = std::fs::rename(&tmp_sc, sidecar_path(bin));
    std::fs::rename(&tmp, bin).is_ok()
}

/// Write-back: publish a freshly built `bin` (and its sidecar) to the shared store (atomically,
/// best-effort) so other same-toolchain hosts can pull and verify it instead of rebuilding.
fn shared_push(bin: &std::path::Path) {
    let slot = match shared_slot(bin) {
        Some(s) => s,
        None => return,
    };
    let parent = match slot.parent() {
        Some(p) => p,
        None => return,
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    // Binary first, then sidecar — a puller that sees the binary will also find its sidecar.
    let tmp = parent.join(format!("push-{}-{}", std::process::id(), name_of(bin)));
    if std::fs::copy(bin, &tmp).is_ok() {
        if std::fs::rename(&tmp, &slot).is_ok() {
            let tmp_sc = sidecar_path(&tmp);
            if std::fs::copy(sidecar_path(bin), &tmp_sc).is_ok() {
                let _ = std::fs::rename(&tmp_sc, sidecar_path(&slot));
            }
        }
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn name_of(p: &std::path::Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("x")
}

/// Pull `bin` from the networked registry (if `ORPHEUS_REGISTRY`/`_KEY` are set): GET it, verify
/// its MAC under the shared key, and install atomically. An artifact whose signature does not
/// verify is **rejected** (never installed), so a tampered or unauthenticated response can't poison
/// the host. Namespaced by toolchain id, like the filesystem store. Best-effort.
fn registry_pull(bin: &std::path::Path) -> bool {
    let url = match crate::registry::registry_url() {
        Some(u) => u,
        None => return false,
    };
    let key = match crate::registry::registry_key() {
        Some(k) => k,
        None => return false,
    };
    let full = format!("{}/{}/{}", url, toolchain_id(), name_of(bin));
    let (status, headers, body) = match crate::registry::http_get(&full) {
        Some(r) => r,
        None => return false,
    };
    if status != 200 {
        return false;
    }
    let claimed = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-orpheus-mac"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    if !crate::registry::mac_eq(claimed, &crate::registry::mac_hex(&key, &body)) {
        return false; // unauthenticated / tampered — refuse to install
    }
    let dir = match bin.parent() {
        Some(d) => d,
        None => return false,
    };
    let tmp = dir.join(format!("net-{}-{}", std::process::id(), name_of(bin)));
    if std::fs::write(&tmp, &body).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    if std::fs::rename(&tmp, bin).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    write_sidecar(bin);
    true
}

/// Publish a freshly built `bin` to the registry (if configured), MAC'd under the shared key so the
/// server (and future pullers) can authenticate it. Best-effort.
fn registry_push(bin: &std::path::Path) {
    let (url, key) = match (crate::registry::registry_url(), crate::registry::registry_key()) {
        (Some(u), Some(k)) => (u, k),
        _ => return,
    };
    if let Ok(bytes) = std::fs::read(bin) {
        let mac = crate::registry::mac_hex(&key, &bytes);
        let full = format!("{}/{}/{}", url, toolchain_id(), name_of(bin));
        let _ = crate::registry::http_put(&full, &[("x-orpheus-mac", &mac)], &bytes);
    }
}

/// Update a cached binary's mtime to now, marking it recently used for LRU eviction.
/// Best-effort: a binary currently being executed by another process may refuse the
/// write-open (ETXTBSY) — that's fine, we simply skip the touch.
fn touch(path: &std::path::Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

pub fn run_native_noun_opts(expr: &str, libs: &[&str], force_rebuild: bool) -> Option<crate::knot::N> {
    // Content-addressed: a stable sha3 of the emitted source is the cache key. Identical code →
    // identical key → the existing binary is reused (no recompile); any change to the expression
    // or to a library arm it actually reaches changes the emitted source → new key → a rebuild.
    // The key comes from the memo (native_key) so a WARM run does no codegen at all; the source
    // itself is generated only when a build is actually needed.
    let key = native_key(expr, libs)?;
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let bin = dir.join(format!("e{}{}", &key[..32], BIN_EXT));
    if force_rebuild || !bin.exists() {
        if !rustc_available() {
            return None; // no toolchain on this device: interpreter+JIT answer
        }
        let src = compile_to_rust(expr, libs).ok()?;
        if !build_native(&src, &bin, force_rebuild) {
            return None;
        }
    } else {
        record_hit(); // existing binary reused, no build — and no codegen either
    }
    touch(&bin); // mark recently used for LRU eviction
    let o = std::process::Command::new(&bin).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&o.stdout);
    parse_canon(&out)
}

/// Serialize a noun to the canonical readout `vparse` expects (small atoms as decimal,
/// long-cord atoms as `~hex`, cells as `[h t]`).
pub(crate) fn noun_to_canon(n: &crate::knot::N) -> String {
    match &**n {
        crate::knot::Knot::Atom(a) => {
            if let Some(u) = a.to_u128() {
                u.to_string()
            } else {
                let mut s = String::from("~");
                for by in a.bytes_le() {
                    s.push_str(&format!("{:02x}", by));
                }
                s
            }
        }
        crate::knot::Knot::Cell(h, t) => format!("[{} {}]", noun_to_canon(h), noun_to_canon(t)),
    }
}

/// Compile `expr` ONCE into a native binary that takes its input at run time (the
/// expression references the bound parameter `__in`), then run it against `input`,
/// which is piped in on stdin. The binary is content-addressed by the PROGRAM, not the
/// input, so a heavy program (e.g. the 614-rule Ligurian `evolve`) is built a single
/// time and reused across every input — turning a per-input `rustc` build into one
/// build amortized over all inputs. Returns `None` if any stage declines (caller falls
/// back to the interpreter).
/// A resident native worker: a long-lived child running the loop-mode binary, which serves many
/// requests over a length-framed stdin/stdout protocol — so repeated heavy calls (an agent fold, a
/// db transition) amortize process startup instead of paying it per call. Killed on drop.
struct Worker {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}
impl Worker {
    fn spawn(bin: &std::path::Path) -> Option<Worker> {
        let mut child = std::process::Command::new(bin)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = std::io::BufReader::new(child.stdout.take()?);
        Some(Worker { child, stdin, stdout })
    }
    /// One request/response round-trip. Returns None on any protocol/IO error (caller discards us).
    fn request(&mut self, canon: &str) -> Option<crate::knot::N> {
        use std::io::{BufRead, Read, Write};
        write!(self.stdin, "{}\n{}", canon.len(), canon).ok()?;
        self.stdin.flush().ok()?;
        let mut header = String::new();
        if self.stdout.read_line(&mut header).ok()? == 0 {
            return None; // worker exited
        }
        let n: usize = header.trim().parse().ok()?;
        let mut buf = vec![0u8; n];
        self.stdout.read_exact(&mut buf).ok()?;
        parse_canon(&String::from_utf8_lossy(&buf))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn worker_pool() -> &'static std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Vec<Worker>>>
{
    static POOL: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Vec<Worker>>>,
    > = std::sync::OnceLock::new();
    POOL.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Run `expr` against `input` on a resident worker. Compiles the loop-mode binary on first use
/// (cached + memoized like the one-shot path), checks out an idle worker (or spawns one), does one
/// round-trip, and returns it to the pool on success. Any failure returns None so the caller falls
/// back to the one-shot path — correctness is never at risk.
fn run_via_worker(expr: &str, input: &crate::knot::N, libs: &[&str]) -> Option<crate::knot::N> {
    let memo_key = format!("worker\u{0}{}\u{0}{}", expr, libs.join("\u{0}"));
    let gen = crate::latte::lib_generation();
    let bin = {
        let cached = native_input_memo()
            .lock()
            .unwrap()
            .get(&memo_key)
            .filter(|(g, p)| *g == gen && p.exists())
            .map(|(_, p)| p.clone());
        match cached {
            Some(p) => p,
            None => {
                let src = compile_to_rust_worker(expr, libs).ok()?;
                let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
                let dir = cache_dir();
                let _ = std::fs::create_dir_all(&dir);
                let b = dir.join(format!("w{}{}", &key[..32], BIN_EXT));
                if !b.exists() && !build_native(&src, &b, false) {
                    return None;
                }
                native_input_memo()
                    .lock()
                    .unwrap()
                    .insert(memo_key, (gen, b.clone()));
                b
            }
        }
    };
    touch(&bin);
    // check out an idle worker for this binary, or spawn a fresh one
    let mut w = {
        let mut pool = worker_pool().lock().unwrap();
        pool.get_mut(&bin).and_then(|v| v.pop())
    }
    .or_else(|| Worker::spawn(&bin))?;
    let canon = noun_to_canon(input);
    match w.request(&canon) {
        Some(out) => {
            let mut pool = worker_pool().lock().unwrap();
            let total: usize = pool.values().map(|v| v.len()).sum();
            let v = pool.entry(bin).or_default();
            // keep a small per-binary pool, and bound the total resident workers so a suite that
            // compiles many distinct programs can't spawn an unbounded number of processes.
            if v.len() < 2 && total < 16 {
                v.push(w); // return for reuse (else dropped -> killed)
            }
            Some(out)
        }
        None => None, // w dropped here (killed); caller falls back to one-shot
    }
}

pub fn run_native_with_input(
    expr: &str,
    input: &crate::knot::N,
    libs: &[&str],
    force_rebuild: bool,
) -> Option<crate::knot::N> {
    use std::io::Write;
    // Primary path: a resident worker amortizes process startup across calls. On any failure it
    // returns None and we fall through to the one-shot spawn below — never a correctness risk.
    if !force_rebuild {
        if let Some(n) = run_via_worker(expr, input, libs) {
            return Some(n);
        }
    }
    // Fast path: a hot caller (an agent fold, a db transition) runs the SAME program over and
    // over. Re-deriving the cache key means re-gathering every library and re-emitting the whole
    // Rust source on each call — pure waste once the binary exists. Memoize (expr, libs) -> binary,
    // guarded by the library generation so any lib (re)registration invalidates stale entries.
    let bin: std::path::PathBuf;
    let memo_key = format!("{}\u{0}{}", expr, libs.join("\u{0}"));
    let gen = crate::latte::lib_generation();
    let memo_hit = if force_rebuild {
        None
    } else {
        native_input_memo()
            .lock()
            .unwrap()
            .get(&memo_key)
            .filter(|(g, p)| *g == gen && p.exists())
            .map(|(_, p)| p.clone())
    };
    if let Some(p) = memo_hit {
        record_hit();
        bin = p;
    } else {
        let src = compile_to_rust_opts(expr, libs, Some("__in")).ok()?;
        let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
        let dir = cache_dir();
        let _ = std::fs::create_dir_all(&dir);
        let b = dir.join(format!("i{}{}", &key[..32], BIN_EXT));
        if force_rebuild || !b.exists() {
            if !build_native(&src, &b, force_rebuild) {
                return None;
            }
        } else {
            record_hit();
        }
        native_input_memo()
            .lock()
            .unwrap()
            .insert(memo_key, (gen, b.clone()));
        bin = b;
    }
    touch(&bin); // mark recently used for LRU eviction
    let mut child = std::process::Command::new(&bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(noun_to_canon(input).as_bytes()).ok()?;
    }
    let o = child.wait_with_output().ok()?;
    if !o.status.success() {
        return None;
    }
    parse_canon(&String::from_utf8_lossy(&o.stdout))
}

/// In-process memo: (expr + libs) -> (lib generation, cached binary path). Lets a hot caller skip
/// re-gathering libraries and re-emitting Rust just to recompute a cache key it already knows.
fn native_input_memo(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, (u64, std::path::PathBuf)>> {
    static MEMO: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, (u64, std::path::PathBuf)>>,
    > = std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Run `expr` natively **only if its binary is already cached** — never triggers a `rustc`
/// build. The daemon-aware fast path uses this: a cache hit runs in-process immediately, while
/// a miss defers compilation to the resident server (`anvild`) rather than stalling the caller.
/// (Re-deriving the source is cheap — it's code generation, not compilation.)
/// (expr, libs, lib_generation) -> the binary-cache key. Computing the key
/// requires generating the program's full Rust source (the key is its hash) —
/// tens of milliseconds of codegen for a large scope, paid on EVERY warm-cache
/// run just to find the binary. The memo makes the warm path a lookup + spawn.
/// Keyed by lib_generation, so a `def` or library edit invalidates it wholesale.
/// Best-effort, fire-and-forget `latte cache warm "<expr>"` in a child process,
/// deduplicated per expression for this process's lifetime. Used by the adaptive
/// engine when no compile daemon is running: the caller's answer comes from the
/// interpreter NOW; the native binary arrives for later runs.
fn spawn_detached_warm(expr: &str) {
    use std::sync::{Mutex, OnceLock};
    type HashSet<T> = std::collections::HashSet<T>;
    // NEVER from a test build: under `cargo test`, current_exe() is the TEST
    // HARNESS — re-executing it with CLI words as "arguments" runs the suite
    // again, whose tests reach this function again: a fork bomb. (Learned the
    // hard way: a facet-test run took the host to load average ~540.)
    if cfg!(test) {
        return;
    }
    // Defense in depth: a spawned child must never spawn grandchildren, no
    // matter what code path it takes — the marker travels in its environment.
    if std::env::var_os("ORPHEUS_NO_SPAWN").is_some() {
        return;
    }
    if !rustc_available() {
        return; // no toolchain on this device: a warm build could never land
    }
    static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let inflight = INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()));
    if !inflight.lock().unwrap().insert(expr.to_string()) {
        return; // already warming
    }
    let spawned = std::env::current_exe().ok().and_then(|exe| {
        std::process::Command::new(exe)
            .args(["cache", "warm", expr])
            .env("ORPHEUS_NO_SPAWN", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
    });
    if spawned.is_none() {
        // the spawn itself failed (fork limits, missing exe): allow a retry later
        inflight.lock().unwrap().remove(expr);
    }
}

/// Sweep leftovers of INTERRUPTED builds from the cache dir, once per process.
/// A build killed mid-rustc (a reaped daemon child, Ctrl-C, a crashed host)
/// leaves `build-*.rs` sources and `*.rcgu.o` object shards behind — never a
/// correctness issue (binaries are written atomically by rename), but debris
/// that accumulates. Anything of those shapes older than an hour is dead.
fn sweep_stale_intermediates() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = cache_dir();
        let hour = std::time::Duration::from_secs(3600);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                let stale_shape = (name.starts_with("build-") && (name.ends_with(".rs") || name.contains(".rcgu.")))
                    || name.ends_with(".rcgu.o");
                if !stale_shape {
                    continue;
                }
                let old = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|age| age > hour)
                    .unwrap_or(false);
                if old {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    });
}

fn native_key_memo() -> &'static std::sync::Mutex<std::collections::HashMap<(String, String, u64), String>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<(String, String, u64), String>>> =
        std::sync::OnceLock::new();
    M.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// A fingerprint of THIS BUILD of the system: the sha3 of the running
/// executable, computed once per process (~a few ms for a ~3 MB binary).
/// Every library source is embedded in the executable, so editing any shipped
/// .lat and rebuilding necessarily changes the fingerprint — which is what
/// makes the on-disk key memo safe: the same (expression, scope) under
/// different library code can never hash to the same memo line.
fn build_fingerprint() -> &'static str {
    static F: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        // (path, byte length, mtime in ns) of the running executable — one stat,
        // no content read. Every rebuild rewrites the binary and therefore its
        // mtime (nanosecond resolution) and usually its length, so two different
        // builds cannot share a fingerprint in practice; a byte-identical restore
        // with a fresh mtime merely misses the memo and regenerates, never lies.
        let id = std::env::current_exe().ok().and_then(|p| {
            let md = std::fs::metadata(&p).ok()?;
            let mtime = md
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos();
            Some(format!("{}\u{1}{}\u{1}{}", p.display(), md.len(), mtime))
        });
        match id {
            Some(seed) => crate::sha3::hex(&crate::sha3::sha3_256(seed.as_bytes()))[..16].to_string(),
            None => "no-exe".into(), // cannot fingerprint: the caller will skip persistence
        }
    })
}

/// The persisted half of the key memo (cache_dir/nkeys.tsv): only entries whose
/// scope is entirely BUILT-IN libraries at generation 0 are stored, so a line can
/// never describe a runtime-registered module whose content the fingerprint
/// cannot see. This is exactly the one-shot CLI case, where it matters most —
/// without it, every warm `latte eval` paid one full codegen just to find its
/// cached binary.
fn disk_key_path() -> std::path::PathBuf {
    cache_dir().join("nkeys.tsv")
}

fn disk_key_memo() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    M.get_or_init(|| {
        let mut m = std::collections::HashMap::new();
        if let Ok(s) = std::fs::read_to_string(disk_key_path()) {
            for line in s.lines() {
                if let Some((h, k)) = line.split_once('\t') {
                    m.insert(h.to_string(), k.to_string());
                }
            }
        }
        // a rebuilt executable orphans every line (the fingerprint changed):
        // when the file has clearly outgrown usefulness, start it over
        if m.len() > 65_536 {
            m.clear();
            let _ = std::fs::remove_file(disk_key_path());
        }
        std::sync::Mutex::new(m)
    })
}

fn native_key(expr: &str, libs: &[&str]) -> Option<String> {
    let gen = crate::latte::lib_generation();
    let mk = (expr.to_string(), libs.join(","), gen);
    if let Some(k) = native_key_memo().lock().unwrap().get(&mk) {
        return Some(k.clone());
    }
    // Persist only generation-0 scopes: with no runtime-registered modules, every
    // library in the set is embedded in the executable, and the executable IS the
    // fingerprint — so a memo line can never describe code the fingerprint missed.
    let persistable = gen == 0 && build_fingerprint() != "no-exe";
    let disk_hash = if persistable {
        let seed = format!("{}\u{1}{}\u{1}{}", expr, libs.join(","), build_fingerprint());
        Some(crate::sha3::hex(&crate::sha3::sha3_256(seed.as_bytes()))[..32].to_string())
    } else {
        None
    };
    if let Some(h) = &disk_hash {
        if let Some(k) = disk_key_memo().lock().unwrap().get(h) {
            native_key_memo().lock().unwrap().insert(mk, k.clone());
            return Some(k.clone());
        }
    }
    let src = compile_to_rust(expr, libs).ok()?;
    let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
    {
        let mut memo = native_key_memo().lock().unwrap();
        if memo.len() >= 4096 {
            memo.clear();
        }
        memo.insert(mk, key.clone());
    }
    if let Some(h) = disk_hash {
        let mut d = disk_key_memo().lock().unwrap();
        if d.len() < 65536 && d.insert(h.clone(), key.clone()).is_none() {
            let _ = std::fs::create_dir_all(cache_dir());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(disk_key_path()) {
                use std::io::Write;
                let _ = writeln!(f, "{}\t{}", h, key);
            }
        }
    }
    Some(key)
}

pub fn run_native_cached(expr: &str, libs: &[&str]) -> Option<crate::knot::N> {
    sweep_stale_intermediates();
    let key = native_key(expr, libs)?;
    let bin = cache_dir().join(format!("e{}{}", &key[..32], BIN_EXT));
    if !bin.exists() {
        return None;
    }
    if !quick_ok(&bin) {
        purge_binary(&bin); // truncated/corrupt — drop it so the next path rebuilds
        return None;
    }
    run_native_noun_opts(expr, libs, false)
}

/// The default execution policy for evaluating a Latte program, and the one in-process callers
/// should reach for: **heavy code runs compiled, light code runs interpreted.** In order:
///   1. a cached native binary always wins — no `rustc`, and this is where repeated heavy code lands;
///   2. a cold program that looks substantial is compiled now (and cached for reuse);
///   3. anything else — a trivial one-shot, or a program the native backend declines — runs on the
///      interpreter, the always-correct fallback (the two engines agree by construction, enforced by
///      the differential fuzzer), so a fallback is never wrong.
/// This centralizes the native-first policy that `latte eval`, the SCA engine, and the GUI console
/// each used to hand-roll.
pub fn run_adaptive(expr: &str, libs: &[&str]) -> Result<crate::knot::N, String> {
    // Distribution decision FIRST: when workers are connected and the program
    // has a distributable shape (explicit `dmap`, or a `map`/`predict_all`
    // the profiler has measured past the distribution threshold), the work is
    // split across the connected instances by default. `maybe_distribute`
    // returns None to mean "stay local" — the common case costs one string
    // prefix check.
    if let Some(r) = crate::dist::maybe_distribute(expr, libs) {
        return r;
    }
    let t_nat = std::time::Instant::now();
    if let Some(n) = run_native_cached(expr, libs) {
        let ns = t_nat.elapsed().as_nanos() as u64;
        if ns >= 200_000 {
            profile_record_native(expr, libs, ns); // keep the measured pair fresh
        }
        return Ok(n);
    }
    // Decide whether to compile: a MEASUREMENT from the profile store beats the structural
    // heuristic — a program the profiler has SEEN interpret slowly is compiled regardless of
    // what its AST looks like, and one measured to be trivial is not compiled even if it
    // contains a loop. With no measurement yet, fall back to the structural guess.
    let should_compile = match profile_lookup(expr, libs) {
        Some(p) if p.interp_ns > 0 => p.interp_ns >= profile_threshold_ns(),
        _ => worth_compiling(expr),
    };
    if should_compile {
        // Prefer a resident compile daemon: it builds in the background (so this call never stalls
        // on rustc) and dedups, and it now builds with *this* call's library scope so the binary it
        // produces is exactly the one a later call will find cached. Only with no daemon running do
        // we compile synchronously here (still better than interpreting heavy code repeatedly).
        if crate::anvild::warm_bg(expr, libs) {
            // building in the background — answer this one call on the interpreter
        } else if cfg!(test) {
            // Under `cargo test` there is no daemon and no self to re-exec (the
            // test harness is not the CLI), so keep the old synchronous build:
            // heavy differential tests depend on getting a native run here.
            if let Some(n) = run_native_noun(expr, libs) {
                return Ok(n);
            }
        } else {
            // No daemon: spawn a DETACHED self-warm instead of building here.
            // A synchronous rustc build used to stall this very call for seconds —
            // a virgin-cache /learn render paid ~10 builds back to back (~36 s).
            // Now the interpreter answers immediately and the binary lands for the
            // next run. Deduplicated per expression within this process.
            spawn_detached_warm(expr);
        }
    }
    // Interpreter fallback — and the PROFILING moment: measure this run and remember it.
    // If the measurement crosses the threshold, the program has just proven that it is worth
    // compiling, so trigger a background build now; the *next* run finds the binary warm.
    // This closes the loop: slow-when-interpreted code is detected automatically and compiled
    // automatically, with no annotation and no structural guesswork.
    let t0 = std::time::Instant::now();
    let out = crate::latte::run_with_libs(expr, libs);
    let took = t0.elapsed().as_nanos() as u64;
    // only pay the (small) profile write for runs slow enough to ever matter —
    // sub-0.2ms programs could never cross the compile threshold
    if out.is_ok() && took >= 200_000 {
        profile_record_interp(expr, libs, took);
        if !should_compile && took >= profile_threshold_ns() {
            let _ = crate::anvild::warm_bg(expr, libs); // best-effort; daemon may be absent
        }
    }
    out
}

/// Heuristic: is this program substantial enough that compiling it is likely to pay for the build?
/// True when the AST contains iteration, a closure, or a call to a (possibly recursive) library arm;
/// false for a bare literal or a tree of cheap primitives over literals like `(add 1 2)`. The cost
/// of a wrong guess is bounded — over-compiling a trivial program wastes one cached build, while a
/// mis-judged light program merely interprets — so a cheap structural heuristic suffices.
pub fn worth_compiling(expr: &str) -> bool {
    match latte::parse(expr) {
        Ok(ast) => ast_is_heavy(&ast),
        Err(_) => false, // doesn't parse → won't compile anyway
    }
}

fn ast_is_heavy(a: &Ast) -> bool {
    use Ast::*;
    match a {
        Lit(_) | Tag(_) | Text(_) | Nil | Var(_) => false,
        Tuple(xs) => xs.iter().any(ast_is_heavy),
        Inc(e) | Head(e) | Tail(e) | IsCell(e) | Fast(_, e) => ast_is_heavy(e),
        Eq(x, y) => ast_is_heavy(x) || ast_is_heavy(y),
        If(c, t, e) => ast_is_heavy(c) || ast_is_heavy(t) || ast_is_heavy(e),
        Let(_, v, b) => ast_is_heavy(v) || ast_is_heavy(b),
        Case(s, arms) => ast_is_heavy(s) || arms.iter().any(|(_, e)| ast_is_heavy(e)),
        Gate(_, b) => ast_is_heavy(b),
        // iteration is heavy by definition
        Loop(..) | Again(..) => true,
        // a call to a cheap primitive is light unless an argument is heavy; a call to a library arm
        // (non-primitive, possibly recursive) is worth compiling
        Call(name, args) => !is_native_op(name) || args.iter().any(ast_is_heavy),
    }
}
/// already cached, never building.
pub fn run_native_with_input_cached(
    expr: &str,
    input: &crate::knot::N,
    libs: &[&str],
) -> Option<crate::knot::N> {
    let src = compile_to_rust_opts(expr, libs, Some("__in")).ok()?;
    let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
    let bin = cache_dir().join(format!("i{}{}", &key[..32], BIN_EXT));
    if !bin.exists() {
        return None;
    }
    if !quick_ok(&bin) {
        purge_binary(&bin);
        return None;
    }
    run_native_with_input(expr, input, libs, false)
}

#[cfg(windows)]
const BIN_EXT: &str = ".exe";
#[cfg(not(windows))]
const BIN_EXT: &str = "";

/// A persistent, per-user cache directory for compiled programs, so a binary built once survives
/// across runs (and reboots). Honours `ORPHEUS_CACHE`, then the platform cache home, falling back
/// to the temp dir.
pub fn cache_dir() -> std::path::PathBuf {
    use std::path::PathBuf;
    if let Ok(c) = std::env::var("ORPHEUS_CACHE") {
        if !c.is_empty() {
            return PathBuf::from(c);
        }
    }
    if let Ok(x) = std::env::var("XDG_CACHE_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("orpheus").join("anvil");
        }
    }
    if cfg!(windows) {
        if let Ok(l) = std::env::var("LOCALAPPDATA") {
            if !l.is_empty() {
                return PathBuf::from(l).join("orpheus").join("anvil");
            }
        }
    } else if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            let p = PathBuf::from(h).join(".cache").join("orpheus").join("anvil");
            // On a bare Android shell (adb) HOME is `/` and not writable —
            // prove the directory can exist before adopting it, else fall
            // through to the temp dir so caches and the worker registry work.
            if p.exists() || std::fs::create_dir_all(&p).is_ok() {
                return p;
            }
        }
    }
    std::env::temp_dir().join("orpheus-anvil")
}

/// Whether a cache-dir entry is a compiled program binary (`e…`/`i…`), as opposed to a
/// transient `build-*.rs`/`build-*` artifact from an in-flight compile.
fn is_cached_binary(name: &str) -> bool {
    (name.starts_with('e') || name.starts_with('i'))
        && !name.starts_with("build-")
        && !name.ends_with(SIDECAR_EXT)
}

/// Integrity sidecar suffix. For a binary `eABC…`, `eABC….sha` holds `"<sha3hex> <size>"` written
/// by whoever produced the binary. The cache is content-addressed by *source*, which guarantees the
/// binary was built from the expected program; the sidecar additionally guarantees the binary's
/// *bytes* are intact — catching truncation, bit-rot, an interrupted copy, or a poisoned shared
/// store entry, none of which the source hash can see.
const SIDECAR_EXT: &str = ".sha";

fn sidecar_path(bin: &std::path::Path) -> std::path::PathBuf {
    let mut s = bin.as_os_str().to_os_string();
    s.push(SIDECAR_EXT);
    std::path::PathBuf::from(s)
}

/// The sidecar line for a binary's bytes: `"<sha3hex> <len>"`. Pure, so it is unit-testable.
fn sidecar_contents(bytes: &[u8]) -> String {
    format!("{} {}", crate::sha3::hex(&crate::sha3::sha3_256(bytes)), bytes.len())
}

/// Parse a sidecar line into `(sha3hex, size)`. Pure and total.
fn parse_sidecar(s: &str) -> Option<(String, u64)> {
    let mut it = s.split_whitespace();
    let hash = it.next()?.to_string();
    let size: u64 = it.next()?.parse().ok()?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((hash, size))
}

/// Write `bin`'s integrity sidecar (atomically). Best-effort.
fn write_sidecar(bin: &std::path::Path) {
    if let Ok(bytes) = std::fs::read(bin) {
        let sc = sidecar_path(bin);
        let tmp = sidecar_path(&bin.with_file_name(format!("w-{}-{}", std::process::id(), name_of(bin))));
        if std::fs::write(&tmp, sidecar_contents(&bytes)).is_ok() {
            let _ = std::fs::rename(&tmp, &sc);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Integrity {
    Ok,
    Corrupt,
    NoSidecar,
}

/// Cheap pre-run check: if a sidecar exists, the binary's *size* must match (catches truncation /
/// interrupted writes at near-zero cost — no hashing). Missing sidecar ⇒ can't tell ⇒ allow.
fn quick_ok(bin: &std::path::Path) -> bool {
    let sc = sidecar_path(bin);
    let txt = match std::fs::read_to_string(&sc) {
        Ok(t) => t,
        Err(_) => return true, // no sidecar to check against
    };
    match (parse_sidecar(&txt), std::fs::metadata(bin)) {
        (Some((_, size)), Ok(m)) => m.len() == size,
        _ => true,
    }
}

/// Full verification: hash the binary and compare to its sidecar.
fn verify_file(bin: &std::path::Path) -> Integrity {
    let txt = match std::fs::read_to_string(sidecar_path(bin)) {
        Ok(t) => t,
        Err(_) => return Integrity::NoSidecar,
    };
    let (hash, size) = match parse_sidecar(&txt) {
        Some(v) => v,
        None => return Integrity::Corrupt, // malformed sidecar ⇒ treat as untrustworthy
    };
    match std::fs::read(bin) {
        Ok(bytes) if bytes.len() as u64 == size
            && crate::sha3::hex(&crate::sha3::sha3_256(&bytes)) == hash =>
        {
            Integrity::Ok
        }
        _ => Integrity::Corrupt,
    }
}

/// Remove a binary and its sidecar together (self-heal a corrupt entry; next use rebuilds).
fn purge_binary(bin: &std::path::Path) {
    let _ = std::fs::remove_file(bin);
    let _ = std::fs::remove_file(sidecar_path(bin));
}

/// Number of cached program binaries and their total size in bytes.
pub fn cache_stats() -> (usize, u64) {
    let mut count = 0usize;
    let mut bytes = 0u64;
    if let Ok(rd) = std::fs::read_dir(cache_dir()) {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if is_cached_binary(name) {
                    count += 1;
                    if let Ok(m) = e.metadata() {
                        bytes += m.len();
                    }
                }
            }
        }
    }
    (count, bytes)
}

/// Remove every cached program binary (and any stale build temporaries). Returns the
/// number of binaries removed. The cache is purely derived, so this is always safe;
/// programs simply rebuild on next use.
pub fn cache_clear() -> usize {
    let mut removed = 0usize;
    if let Ok(rd) = std::fs::read_dir(cache_dir()) {
        for e in rd.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if is_cached_binary(name) {
                    if std::fs::remove_file(e.path()).is_ok() {
                        removed += 1;
                    }
                } else if name.starts_with("build-")
                    || name.ends_with(SIDECAR_EXT)
                    || name.starts_with("pull-")
                    || name.starts_with("push-")
                    || name.starts_with("w-")
                    || name == "metrics"
                    || name.starts_with("metrics-")
                {
                    let _ = std::fs::remove_file(e.path()); // sidecars, metrics + stale in-flight artifacts
                }
            }
        }
    }
    removed
}

/// Audit cached binaries against their integrity sidecars. Returns `(ok, corrupt, no_sidecar)`.
/// With `repair`, corrupt binaries (and any binary with a malformed/missing-but-mismatched sidecar)
/// are purged so the next use rebuilds them. Full hashing, so this is an explicit/on-demand check
/// rather than something the hot run path pays for.
pub fn cache_verify(repair: bool) -> (usize, usize, usize) {
    let (mut ok, mut corrupt, mut no_sc) = (0usize, 0usize, 0usize);
    let dir = cache_dir();
    let mut bins = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if e.file_name().to_str().map(is_cached_binary).unwrap_or(false) {
                bins.push(e.path());
            }
        }
    }
    for bin in bins {
        match verify_file(&bin) {
            Integrity::Ok => ok += 1,
            Integrity::NoSidecar => no_sc += 1,
            Integrity::Corrupt => {
                corrupt += 1;
                if repair {
                    purge_binary(&bin);
                }
            }
        }
    }
    (ok, corrupt, no_sc)
}

/// The cache size cap in bytes. `ORPHEUS_CACHE_MAX` sets it in MiB; `0` disables eviction
/// (unbounded). Default 512 MiB — comfortably more than typical use (one shared binary per
/// distinct program) yet small enough to keep a heavy multi-program workload from filling disk.
pub fn cache_cap_bytes() -> u64 {
    match std::env::var("ORPHEUS_CACHE_MAX") {
        Ok(s) => s.trim().parse::<u64>().unwrap_or(512).saturating_mul(1 << 20),
        Err(_) => 512 << 20,
    }
}

/// Pure LRU eviction planner: given `(path, size, mtime)` for each cached binary and a byte
/// `cap`, return the paths to remove — oldest (least-recently-used) first — so the remaining
/// total fits under `cap`. Returns empty when `cap == 0` (disabled) or already within cap.
/// Filesystem-free and deterministic, so it can be unit-tested directly.
fn plan_evictions(
    mut entries: Vec<(std::path::PathBuf, u64, std::time::SystemTime)>,
    cap: u64,
) -> Vec<std::path::PathBuf> {
    let total: u64 = entries.iter().map(|(_, s, _)| *s).sum();
    if cap == 0 || total <= cap {
        return Vec::new();
    }
    entries.sort_by_key(|(_, _, m)| *m); // least-recently-used first
    let mut to_free = total - cap;
    let mut out = Vec::new();
    for (path, size, _) in entries {
        if to_free == 0 {
            break;
        }
        out.push(path);
        to_free = to_free.saturating_sub(size);
    }
    out
}

/// Enforce the cache size cap by removing least-recently-used binaries. Recency is the file
/// mtime, refreshed on each use by `touch`. Best-effort: removing a binary another process is
/// executing is safe on Unix (the inode lives until that process exits) and merely fails
/// elsewhere. Intended to be called from `build_native` under the build lock.
fn evict_to_cap() {
    let cap = cache_cap_bytes();
    if cap == 0 {
        return;
    }
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(cache_dir()) {
        for e in rd.flatten() {
            let is_bin = e.file_name().to_str().map(is_cached_binary).unwrap_or(false);
            if !is_bin {
                continue;
            }
            if let Ok(m) = e.metadata() {
                let mtime = m.modified().unwrap_or(std::time::UNIX_EPOCH);
                entries.push((e.path(), m.len(), mtime));
            }
        }
    }
    for path in plan_evictions(entries, cap) {
        purge_binary(&path); // binary + its sidecar
    }
}


/// and the runtime-input binary (so `latte eval` and the input-fed callers are both
/// warm). Returns Ok(true) if a build happened, Ok(false) if already warm, Err on a
/// compile failure.
pub fn warm_native(expr: &str, libs: &[&str]) -> Result<bool, String> {
    let mut built = false;
    for (input_param, prefix) in [(None, 'e'), (Some("__in"), 'i')] {
        let src = compile_to_rust_opts(expr, libs, input_param)?;
        let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
        let dir = cache_dir();
        let _ = std::fs::create_dir_all(&dir);
        let bin = dir.join(format!("{}{}{}", prefix, &key[..32], BIN_EXT));
        if !bin.exists() {
            if !build_native(&src, &bin, false) {
                return Err(format!("native compile failed for {} mode", prefix));
            }
            built = true;
        }
    }
    Ok(built)
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn profile_key_distinguishes_program_and_scope() {
        let k1 = profile_key("(add 1 2)", &["std"]);
        let k2 = profile_key("(add 1 3)", &["std"]);
        let k3 = profile_key("(add 1 2)", &["std", "num"]);
        assert_ne!(k1, k2, "different programs, different keys");
        assert_ne!(k1, k3, "different scopes, different keys");
        // scope order must not matter
        assert_eq!(profile_key("x", &["a", "b"]), profile_key("x", &["b", "a"]));
    }

    #[test]
    fn profile_record_and_lookup_roundtrip_with_smoothing() {
        // never evaluated — the store is text-keyed; a unique key per run keeps the
        // test idempotent across invocations (the store persists in the cache dir)
        let expr = format!(
            "(profile-test-roundtrip-{}-{:?})",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap()
        );
        let expr = expr.as_str();
        let libs = ["std"];
        profile_record_interp(expr, &libs, 8_000_000);
        let p = profile_lookup(expr, &libs).expect("recorded");
        assert_eq!(p.interp_ns, 8_000_000);
        assert_eq!(p.runs, 1);
        // smoothing: (3*8ms + 4ms) / 4 = 7ms
        profile_record_interp(expr, &libs, 4_000_000);
        let p = profile_lookup(expr, &libs).expect("still there");
        assert_eq!(p.interp_ns, 7_000_000);
        assert_eq!(p.runs, 2);
        profile_record_native(expr, &libs, 1_000_000);
        let p = profile_lookup(expr, &libs).expect("native recorded");
        assert_eq!(p.native_ns, 1_000_000);
        // a measured slow program decides "compile" regardless of AST shape
        assert!(p.interp_ns >= profile_threshold_ns());
    }
}

#[cfg(test)]
mod tests {
    use super::compile_to_rust;
    use super::worth_compiling;
    use crate::latte;

    #[test]
    fn resident_worker_serves_many_requests() {
        // The loop-mode worker must handle multiple framed requests on one process and agree with
        // the one-shot path. Exercises: compile loop binary, spawn, two round-trips, pool reuse.
        use crate::knot::{cell, num};
        let expr = "(add (head __in) (tail __in))";
        let a = super::run_via_worker(expr, &cell(num(5), num(7)), &["std"])
            .expect("worker round-trip 1");
        assert_eq!(a, num(12));
        let b = super::run_via_worker(expr, &cell(num(10), num(20)), &["std"])
            .expect("worker round-trip 2 (reused process)");
        assert_eq!(b, num(30));
        // worker result must equal the one-shot native path
        let c = super::run_native_with_input(expr, &cell(num(2), num(3)), &["std"], false)
            .expect("one-shot");
        assert_eq!(c, num(5));
    }

    #[test]
    fn heaviness_gate_picks_compile_for_real_work_only() {
        // Trivial: a bare literal or a tree of cheap primitives over literals → interpret.
        assert!(!worth_compiling("42"));
        assert!(!worth_compiling("(add 1 2)"));
        assert!(!worth_compiling("(add (mul 2 3) (sub 9 4))"));
        assert!(!worth_compiling("(let x = 5 in (add x 1))"));
        // Substantial: iteration, a call to a library arm, or a closure-driven HOF → compile.
        assert!(worth_compiling("(map (fn [x] -> (mul x x)) (range 5))"));
        assert!(worth_compiling("(foldl (fn [a v] -> (add a v)) 0 (range 9))"));
        assert!(worth_compiling("(fib 30)"));
        assert!(worth_compiling(
            "(loop with [i = 3, acc = 0] : if (i == 0) then acc else again((dec i), (add acc i)) end)"
        ));
        // Doesn't parse → not worth compiling (would only fail rustc).
        assert!(!worth_compiling("(((("));
    }

    #[test]
    fn native_case_handles_long_tags() {
        // Regression: a `case` whose pattern tag exceeds 16 bytes used to exceed the u128 tag
        // backend and fall back; it must now compile natively and agree with the interpreter,
        // just like a long tag used as a literal.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        let t = "%this_is_a_very_long_tag_name_well_over_sixteen_bytes";
        let expr = format!("(case {t} of {t} -> 111 ; _ -> 222 end)", t = t);
        // the front-end no longer rejects the long tag
        assert!(super::compile_to_rust(&expr, &refs).is_ok(), "long-tag case should lower natively");
        // native (when rustc is available) agrees with the interpreter and takes the match arm
        if let Some(n) = super::run_native_noun(&expr, &refs) {
            let interp = crate::latte::run_with_libs(&expr, &refs).expect("interp runs");
            assert_eq!(super::noun_to_canon(&n), super::noun_to_canon(&interp));
            assert_eq!(n.as_atom().and_then(|a| a.to_u128()), Some(111));
        }
    }

    #[test]
    fn native_closure_captures_arm_shadowing_local() {
        // Regression: a closure that captures a local whose name coincides with a library arm
        // used to drop the capture (free_vars treats arm names as global references) and emit
        // an unbound variable, forcing the whole program onto the interpreter. Binders that
        // shadow an arm are now alpha-renamed like shadowed primitives, so such closures
        // compile natively and agree with the interpreter.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();

        // synthetic: capture a local named `len` (a std arm) inside a map closure
        let e1 = "(let len = 7 in (map (fn [e] -> (add e len)) [1 [2 [3 0]]]))";
        assert!(super::compile_to_rust(e1, &refs).is_ok(), "arm-shadowing capture should lower natively");
        if let Some(n) = super::run_native_noun(e1, &refs) {
            let interp = crate::latte::run_with_libs(e1, &refs).expect("interp runs");
            assert_eq!(super::noun_to_canon(&n), super::noun_to_canon(&interp));
        }

        // real code: db_select's scan path filters with a closure capturing its `field` and
        // `value` parameters (`field` is itself an arm in ui.lat) — exactly the form that
        // previously could not be compiled.
        let e2 = "(db_pluck 1 (db_select (db_orders 0) 2 250 %o0 %o9))";
        assert!(super::compile_to_rust(e2, &refs).is_ok(), "db_select closure should lower natively");
        if let Some(n) = super::run_native_noun(e2, &refs) {
            let interp = crate::latte::run_with_libs(e2, &refs).expect("interp runs");
            assert_eq!(super::noun_to_canon(&n), super::noun_to_canon(&interp));
        }
    }

    #[test]
    fn native_bignum_arithmetic_exceeds_u128() {
        // Regression: native atoms are u128, but the database's bloom filter / hash bitset builds
        // values far wider than that (a 4096-bit set via shl/pow/bor). Those arithmetic ops used
        // to overflow u128 at run time, panic, and silently fall back to the interpreter (which
        // then exhausted its fuel on heavy programs). The native backend now carries
        // arbitrary-precision naturals, so each of these compiles, runs natively, and agrees with
        // the interpreter — exercising mul (shl/pow), add, borrow-sub, div, mod, and bitwise ops.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        let cases = [
            "(pow 2 4000)",                           // ~500-byte result, via repeated squaring
            "(add (pow 2 200) (pow 2 200))",          // wide add with carry
            "(sub (pow 2 200) 1)",                    // wide borrow across many bytes
            "(mul (pow 2 100) (pow 2 100))",          // wide * wide
            "(div (pow 2 200) (pow 2 100))",          // wide / wide (power-of-two fast path)
            "(mod (pow 2 200) 1000000007)",           // wide mod small (general division)
            "(bor (shl 1 200) 5)",                    // bitwise OR on a wide value
            "(band (sub (pow 2 256) 1) (shl 1 200))", // bitwise AND on wide values
            "(bit (shl 1 4000) 4000)",                // bit test high in a 4096-bit value
        ];
        for e in cases {
            assert!(super::compile_to_rust(e, &refs).is_ok(), "{} should lower natively", e);
            if let Some(n) = super::run_native_noun(e, &refs) {
                let interp = crate::latte::run_with_libs(e, &refs).expect("interp runs");
                assert_eq!(
                    super::noun_to_canon(&n),
                    super::noun_to_canon(&interp),
                    "native and interpreter disagree on {}",
                    e
                );
            }
        }
    }

    #[test]
    fn native_compiles_full_bond_model() {
        // The fully-featured bond model — a ~230-month feature build over TEN fixed-income
        // factors (now including the Cochrane-Piazzesi tent and the Cieslak-Povala cycle),
        // plus a 4000-iteration logistic fit and out-of-sample evaluation — exceeds the
        // interpreter's default fuel budget, but compiles and runs natively (the adaptive
        // opt-level optimizes a program this large). It must lower to native code and produce the
        // known report [ train test baseline ] = 98.6% / 69.6% / 63.3% (signed fractions x1000);
        // on the smoothed teaching series the two literature factors cost ~1pt out-of-sample
        // versus the eight-factor row while keeping a +6.3pt edge over baseline — the factor
        // set is chosen for research fidelity, and the advisor reports whatever the live edge is.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        assert!(super::compile_to_rust("(report 0)", &refs).is_ok(), "the bond model should lower natively");
        if let Some(n) = super::run_native_noun("(report 0)", &refs) {
            assert_eq!(super::noun_to_canon(&n), "[[0 986] [[0 696] [[0 633] 0]]]");
        }
    }

    #[test]
    fn native_compiles_higher_order_idioms() {
        // Valid functional Latte — closures passed to HOFs, a gate bound to a name and applied,
        // a gate parameter applied twice, a comparator closure, case dispatch, and closures that
        // capture a local whose name collides with a library arm — must all (a) lower to native
        // code and (b) produce the SAME noun as the interpreter. This guards the native backend's
        // completeness: a future change that drops any of these back to the interpreter trips (a),
        // and one that miscompiles any of them trips (b).
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        let cases = [
            // composed HOFs: foldl over a mapped list
            "(foldl (fn [a v] -> (add a v)) 0 (map (fn [x] -> (mul x x)) [1 [2 [3 0]]]))",
            // a gate bound to a let and then applied through a HOF
            "(let g = (fn [x] -> (add x 1)) in (map g [1 [2 [3 0]]]))",
            // a higher-order gate PARAMETER, applied twice inside the body
            "(let f = (fn [k x] -> (k (k x))) in (f (fn [y] -> (add y 1)) 10))",
            // a closure that itself calls another HOF
            "(filter (fn [e] -> (member e [2 [4 0]])) [1 [2 [3 [4 0]]]])",
            // a comparator closure
            "(sortby (fn [a b] -> (lt a b)) [3 [1 [2 0]]])",
            // case dispatch
            "(case %xx of %xx -> 1 ; _ -> 2 end)",
            // closures capturing a local whose name is also a library arm (the shadowing fix):
            "(let len = 7 in (map (fn [e] -> (add e len)) [1 [2 [3 0]]]))",
            "(let field = 1 in (foldl (fn [acc e] -> if ((head e) == field) then (tail e) else acc) 0 [[1 9] 0]))",
            "(let nadd = 5 in (foldl (fn [a x] -> (add a (add x nadd))) 0 [1 [2 [3 0]]]))",
        ];
        for expr in cases {
            assert!(
                super::compile_to_rust(expr, &refs).is_ok(),
                "valid HOF idiom should lower natively (regressed to interpreter): {}",
                expr
            );
            if let Some(n) = super::run_native_noun(expr, &refs) {
                let interp = crate::latte::run_with_libs(expr, &refs).expect("interp runs");
                assert_eq!(
                    super::noun_to_canon(&n),
                    super::noun_to_canon(&interp),
                    "native result diverged from the interpreter: {}",
                    expr
                );
            }
        }
    }

    #[test]
    fn native_and_interp_agree_on_rejected_programs() {
        // The invariant: "native accepts iff the interpreter accepts" — the two engines must
        // never silently drift. HISTORY: this test originally pinned bare arm names in value
        // position (`(foldl add 0 …)`) as REJECTED on both sides, with instructions that
        // introducing first-class functions is a language change both engines must adopt
        // together, eta-expanding on both sides, updating this test deliberately. That change
        // has now been made deliberately: arms ETA-EXPAND to gates on the interpreter AND the
        // native backend (see Ast::Var in latte::gen and in emit), so the former rejects are
        // now ACCEPTS — and the drift check for them is stronger: both engines must also agree
        // on the VALUE. What remains rejected on both: wrong arity and unbound names.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        let rejected = [
            "(add 1)",                 // partial application: arity is still checked
            "(map zzz9 [1 [2 0]])",    // an unbound name is not an arm; nothing to expand
        ];
        for expr in rejected {
            let interp_ok = crate::latte::run_with_libs(expr, &refs).is_ok();
            let native_ok = super::compile_to_rust(expr, &refs).is_ok();
            assert!(!interp_ok, "precondition: interpreter should reject this program: {}", expr);
            assert_eq!(
                native_ok, interp_ok,
                "native and interpreter must agree on acceptance (no silent divergence): {}",
                expr
            );
        }
        // the deliberately-adopted eta-expansion: both engines accept AND agree on values
        let now_accepted = [
            ("(foldl add 0 [1 [2 [3 0]]])", "6"),
            ("(map dec [3 [4 [5 0]]])", "[2 [3 [4 0]]]"),
        ];
        for (expr, want) in now_accepted {
            let iv = crate::latte::run_with_libs(expr, &refs).expect(expr);
            assert_eq!(crate::serve::render_noun(&iv), want, "interpreter value: {}", expr);
            let nv = super::run_native_noun_opts(expr, &refs, false).expect(expr);
            assert_eq!(crate::serve::render_noun(&nv), want, "native value: {}", expr);
        }
    }

    #[test]
    fn native_runtime_falls_back_gracefully_to_interpreter() {
        // Arithmetic on a cord longer than 16 bytes exceeds the u128 the native backend uses for
        // atoms; the emitted program lowers fine but panics at run time on such an atom, so the
        // engine falls back to the interpreter — the always-correct reference enforced by the
        // differential fuzzer. The user-visible result via run_adaptive (native-or-fallback) must
        // therefore equal the interpreter's, even though the native run itself cannot complete.
        // This pins the safety net: the boundary stays correct, never a wrong native answer.
        let libs = latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        let expr = "(add %aaaaaaaaaaaaaaaaaaaa 1)"; // a 20-byte cord atom, + 1
        let interp = crate::latte::run_with_libs(expr, &refs).expect("interp runs");
        // it lowers (the fallback is a *runtime* event, not a compile-time rejection)
        assert!(super::compile_to_rust(expr, &refs).is_ok(), "long-cord arithmetic should still lower");
        // and the adaptive path returns the interpreter's value
        let adaptive = super::run_adaptive(expr, &refs).expect("adaptive returns a value via fallback");
        assert_eq!(
            super::noun_to_canon(&adaptive),
            super::noun_to_canon(&interp),
            "adaptive (native-or-fallback) must match the interpreter on long-cord arithmetic"
        );
    }

    #[test]
    fn metrics_roundtrip_and_derivations() {
        use super::{format_metrics, parse_metrics, Metrics};
        let m = Metrics { builds: 4, build_ms_total: 2000, hits: 10, pulls: 2, build_failures: 1 };
        // serialize → parse is identity
        let back = parse_metrics(&format_metrics(&m));
        assert_eq!((back.builds, back.build_ms_total, back.hits, back.pulls, back.build_failures),
                   (4, 2000, 10, 2, 1));
        assert_eq!(m.avg_build_ms(), 500); // 2000/4
        assert_eq!(m.est_saved_ms(), 6000); // (10+2)*500
        // robust to missing/garbage lines and to no builds (no divide-by-zero)
        let z = parse_metrics("hits 7\njunk\nbuilds notanum\n");
        assert_eq!((z.hits, z.builds), (7, 0));
        assert_eq!(z.avg_build_ms(), 0);
        assert_eq!(z.est_saved_ms(), 0);
    }

    #[test]
    fn build_log_tail_keeps_whole_lines() {
        use super::tail_from_line_boundary;
        let log = "line1\nline2\nline3\n";
        // fits within max → unchanged
        assert_eq!(tail_from_line_boundary(log, 1000), log);
        // tight cap → drop oldest whole lines, never a partial line
        let t = tail_from_line_boundary(log, 12);
        assert!(t.ends_with("line3\n"));
        assert!(!t.contains("line1")); // oldest dropped
        for piece in t.split('\n').filter(|s| !s.is_empty()) {
            assert!(log.contains(&format!("{}\n", piece)), "no partial lines: {:?}", piece);
        }
    }

    #[test]
    fn sidecar_roundtrip_and_validation() {
        use super::{parse_sidecar, sidecar_contents};
        let line = sidecar_contents(b"hello world");
        let (hash, size) = parse_sidecar(&line).expect("well-formed");
        assert_eq!(size, 11);
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        // identical bytes ⇒ identical sidecar; one different byte ⇒ different hash
        assert_eq!(sidecar_contents(b"hello world"), line);
        assert_ne!(sidecar_contents(b"hello worle"), line);
        // malformed sidecars are rejected, not panicked on
        assert!(parse_sidecar("").is_none());
        assert!(parse_sidecar("deadbeef 10").is_none()); // hash too short
        assert!(parse_sidecar("nothex_nothex_nothex_nothex_nothex_nothex_nothex_nothex_nothex_xx 5").is_none());
        assert!(parse_sidecar(&format!("{} notanumber", "a".repeat(64))).is_none());
    }

    #[test]
    fn toolchain_id_components_are_filesystem_safe() {
        use super::sanitize_component;
        assert_eq!(sanitize_component("1.75.0"), "1.75.0");
        assert_eq!(sanitize_component("x86_64-unknown-linux-gnu"), "x86_64-unknown-linux-gnu");
        // spaces, parens, slashes → '-', so a tid is always a single safe path component
        assert_eq!(sanitize_component("rustc 1.75 (abc 2023)"), "rustc-1.75--abc-2023-");
        assert_eq!(sanitize_component("a/b\\c:d"), "a-b-c-d");
        assert!(!super::toolchain_id().is_empty());
    }

    #[test]
    fn cache_eviction_plan_is_lru_and_bounded() {
        use super::plan_evictions;
        use std::path::PathBuf;
        use std::time::{Duration, UNIX_EPOCH};
        let at = |secs| UNIX_EPOCH + Duration::from_secs(secs);
        // Three 100-byte binaries, cap 150 → must free ≥150, evicting oldest-mtime first.
        let entries = vec![
            (PathBuf::from("new"), 100u64, at(3)),
            (PathBuf::from("old"), 100, at(1)),
            (PathBuf::from("mid"), 100, at(2)),
        ];
        assert_eq!(
            plan_evictions(entries, 150),
            vec![PathBuf::from("old"), PathBuf::from("mid")],
            "evicts least-recently-used first until under cap"
        );
        // Already within cap → nothing removed.
        assert!(plan_evictions(vec![(PathBuf::from("a"), 50, at(1))], 100).is_empty());
        // cap 0 disables eviction entirely.
        assert!(plan_evictions(vec![(PathBuf::from("a"), 999, at(1))], 0).is_empty());
    }

    fn libs() -> Vec<String> {
        latte::all_libs()
    }
    fn interp(e: &str) -> String {
        let l = libs();
        let r: Vec<&str> = l.iter().map(|s| s.as_str()).collect();
        crate::net::show_state(&latte::run_with_libs(e, &r).unwrap())
    }
    fn native(e: &str, _idx: usize) -> String {
        let l = libs();
        let r: Vec<&str> = l.iter().map(|s| s.as_str()).collect();
        let n = super::run_native_noun(e, &r).expect("native run");
        crate::net::show_state(&n)
    }

    #[test]
    fn latte_to_rust_matches_interpreter() {
        let cases = [
            "(add 2 3)",
            "(div (mul 7 (add 3 4)) 2)",
            "(foldl (fn [a b] -> (add a b)) 0 (range 10))",
            "(map (fn [x] -> (mul x x)) [1 [2 [3 0]]])",
            "(let k = 10 in (map (fn [x] -> (add x k)) [1 [2 [3 0]]]))",
            "(len (legal (initial 0)))",
            "(choose (initial 0))",
            // a local that shadows the `sub` primitive must be alpha-renamed and still
            // match the interpreter (the value 9, not a primitive subtraction)
            "(let sub = 9 in (add sub 1))",
            // shadowing inside a lambda and a loop, with the real primitive used nearby
            "(let div = 100 in (sub div (mul 2 3)))",
            "(foldl (fn [sub x] -> (add sub x)) 0 [10 [20 [30 0]]])",
        ];
        for (i, e) in cases.iter().enumerate() {
            assert_eq!(interp(e), native(e, i), "mismatch on {}", e);
        }
    }

    #[test]
    fn canon_parser_roundtrips() {
        // the glue that lets every host render the compiler's output in its own style
        let n = super::parse_canon("[1 [2 [3 0]]]").unwrap();
        assert_eq!(crate::net::show_state(&n), "[1 [2 [3 0]]]");
        let a = super::parse_canon("  42\n").unwrap();
        assert_eq!(crate::net::show_state(&a), "42");
        assert!(super::parse_canon("<fn>").is_none());
    }

    #[test]
    fn constant_folding_happens() {
        // a literal-only expression should fold to a single atom in the emitted Rust
        let src = compile_to_rust("(add (mul 2 3) 4)", &["std"]).unwrap();
        assert!(src.contains("fn arm___main(_: V) -> V { V::A(10u128) }"), "expected fold to 10:\n{}", src);
    }

    // run the interpreter, returning Some(value) on success and None if it crashed
    fn interp_opt(e: &str) -> Option<String> {
        let l = libs();
        let r: Vec<&str> = l.iter().map(|s| s.as_str()).collect();
        latte::run_with_libs(e, &r).ok().map(|v| crate::net::show_state(&v))
    }
    // compile+run, returning Some(value) on a clean exit and None if the program failed
    fn native_opt(e: &str, _idx: usize) -> Option<String> {
        let l = libs();
        let r: Vec<&str> = l.iter().map(|s| s.as_str()).collect();
        super::run_native_noun(e, &r).map(|n| crate::net::show_state(&n))
    }

    #[test]
    fn boundary_failure_modes_match() {
        // Anvil must agree with the interpreter on values. The native backend now carries
        // arbitrary-precision naturals (not just u128), so values past the old u128 boundary
        // succeed on BOTH engines and must produce the same noun — never a silent wrap. Domain
        // errors (underflow, divide by zero, dec(0)) must still fail on both.
        let cases: &[(&str, Expect)] = &[
            ("(mul 65536 65536)", Expect::BothOk),
            ("(div 100 7)", Expect::BothOk),
            ("(add 340282366920938463463374607431768211454 1)", Expect::BothOk), // == u128::MAX
            ("(add 340282366920938463463374607431768211455 1)", Expect::BothOk), // u128::MAX + 1 = 2^128
            ("(mul 18446744073709551616 18446744073709551616)", Expect::BothOk), // 2^64 * 2^64 = 2^128
            ("(pow 2 600)", Expect::BothOk), // far past u128, via repeated squaring
            ("(sub 3 5)", Expect::BothFail),
            ("(div 5 0)", Expect::BothFail),
            ("(dec 0)", Expect::BothFail),
        ];
        #[derive(Clone, Copy, PartialEq)]
        enum Expect {
            BothOk,
            BothFail,
        }
        for (i, (e, expect)) in cases.iter().enumerate() {
            let iv = interp_opt(e);
            let nv = native_opt(e, i);
            match expect {
                Expect::BothOk => {
                    assert!(iv.is_some() && nv.is_some(), "{} should succeed on both: interp={:?} native={:?}", e, iv, nv);
                    assert_eq!(iv, nv, "value disagreement on {}", e);
                }
                Expect::BothFail => {
                    assert!(iv.is_none() && nv.is_none(), "{} should fail on both: interp={:?} native={:?}", e, iv, nv);
                }
            }
        }
    }
}

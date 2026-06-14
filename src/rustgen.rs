//! rustgen — an optimizing Latte → Rust compiler ("Anvil").
//!
//! Where the JIT compiles Loom formulas to closures at run time, this is an *ahead-of-time*
//! compiler that emits standalone Rust source for a Latte expression and its whole library
//! closure. The emitted program carries a tiny self-contained noun runtime, so it compiles with
//! a stock `rustc` and runs natively — no dependency on this crate.
//!
//! Optimizations applied:
//!   * constant folding   — literal-only subexpressions are evaluated at compile time;
//!   * native primitives  — the arithmetic/▸comparison jets lower to native Rust `u128` ops
//!                          instead of interpreting their Latte bodies on the VM;
//!   * dead-arm removal   — only arms reachable from `__main` are emitted;
//!   * let → Rust `let`   — sharing is preserved (no recomputation);
//!   * tail calls → loops — `loop … again(..)` becomes a real Rust `loop` with mutable state;
//!   * HOFs stay general  — lambdas compile to reusable native closures (so `map`/`filter`/
//!                          `foldl` and friends compile straight from their Latte definitions).
//!
//! Atoms are represented as `u128` with checked arithmetic. This is not a divergence from the
//! interpreter: the interpreter's own arithmetic jets are u128-bounded too — `operands()` rejects
//! atoms that exceed u128, and `jet_add`/`jet_sub`/`jet_mul`/`jet_div`/`jet_mod`/`jet_dec` all use
//! checked ops that crash on overflow/underflow/zero-divisor. Anvil mirrors each of these failure
//! modes exactly (a checked op that would crash the interpreter `panic!`s the compiled program on
//! the same inputs), so the two agree on success values *and* on which inputs fail. The single
//! theoretical edge is a cord/tag literal longer than 16 bytes (which cannot fit u128); every tag
//! used anywhere in the system is at most 5 bytes, so this never arises in practice.

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
fn unshadow(ast: &Ast, env: &HashMap<String, String>, ctr: &mut usize) -> Ast {
    let go = |a: &Ast, e: &HashMap<String, String>, c: &mut usize| Box::new(unshadow(a, e, c));
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
        Ast::Tuple(xs) => Ast::Tuple(xs.iter().map(|x| unshadow(x, env, ctr)).collect()),
        Ast::Again(xs) => Ast::Again(xs.iter().map(|x| unshadow(x, env, ctr)).collect()),
        Ast::Call(f, xs) => Ast::Call(f.clone(), xs.iter().map(|x| unshadow(x, env, ctr)).collect()),
        Ast::Case(s, arms) => Ast::Case(
            go(s, env, ctr),
            arms.iter().map(|(p, e)| (p.clone(), unshadow(e, env, ctr))).collect(),
        ),
        Ast::Let(n, v, b) => {
            let v2 = unshadow(v, env, ctr); // initialiser is in the outer scope
            let mut env2 = env.clone();
            let name = if prim_arity(n).is_some() {
                let f = fresh(n, ctr);
                env2.insert(n.clone(), f.clone());
                f
            } else {
                env2.remove(n); // a plain binding shadows any active rename
                n.clone()
            };
            Ast::Let(name, Box::new(v2), Box::new(unshadow(b, &env2, ctr)))
        }
        Ast::Gate(params, body) => {
            let mut env2 = env.clone();
            let params2 = params
                .iter()
                .map(|p| {
                    if prim_arity(p).is_some() {
                        let f = fresh(p, ctr);
                        env2.insert(p.clone(), f.clone());
                        f
                    } else {
                        env2.remove(p);
                        p.clone()
                    }
                })
                .collect();
            Ast::Gate(params2, Box::new(unshadow(body, &env2, ctr)))
        }
        Ast::Loop(binds, body) => {
            let mut env2 = env.clone();
            let binds2 = binds
                .iter()
                .map(|(n, v)| {
                    let v2 = unshadow(v, env, ctr); // initialiser in the outer scope
                    let name = if prim_arity(n).is_some() {
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
            Ast::Loop(binds2, Box::new(unshadow(body, &env2, ctr)))
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

fn cord_to_u128(t: &str) -> Result<u128, String> {
    let b = t.as_bytes();
    if b.len() > 16 {
        return Err(format!("tag %{} too long for the u128 atom backend", t));
    }
    let mut v: u128 = 0;
    for (i, &byte) in b.iter().enumerate() {
        v |= (byte as u128) << (8 * i);
    }
    Ok(v)
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
fn free_vars(ast: &Ast, bound: &HashSet<String>, arms: &HashSet<String>, out: &mut Vec<String>) {
    let see = |n: &str, out: &mut Vec<String>| {
        if !bound.contains(n) && !arms.contains(n) && prim_arity(n).is_none() && !out.contains(&n.to_string()) {
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
            free_vars(v, bound, arms, out);
            let mut b2 = bound.clone();
            b2.insert(n.clone());
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
        Ast::Tag(t) => format!("V::A({}u128)", cord_to_u128(t)?),
        Ast::Text(t) => format!("V::A({}u128)", cord_to_u128(t)?),
        Ast::Var(n) => {
            if c.bound.contains(n) {
                format!("{}.clone()", sanitize(n))
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
            let vs = emit(v, c)?;
            let added = c.bound.insert(n.clone());
            let bs = emit(b, c)?;
            if added {
                c.bound.remove(n);
            }
            format!("{{ let {} = {}; {} }}", sanitize(n), vs, bs)
        }
        Ast::Case(scrut, cases) => {
            let s = emit(scrut, c)?;
            let id = c.fresh();
            let mut out = format!("{{ let __s{} = {}; ", id, s);
            let mut first = true;
            for (pat, body) in cases.iter() {
                if let Some(tag) = pat {
                    let kw = if first { "if" } else { "else if" };
                    out.push_str(&format!(
                        "{} veq(&__s{}, &V::A({}u128)) {{ {} }} ",
                        kw,
                        id,
                        cord_to_u128(tag)?,
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
enum V { A(u128), C(Rc<V>, Rc<V>), F(Rc<dyn Fn(Vec<V>) -> V>) }

#[inline] fn na(v: &V) -> u128 { if let V::A(n) = v { *n } else { panic!("expected atom, got cell/fn") } }
#[inline] fn vcell(a: V, b: V) -> V { V::C(Rc::new(a), Rc::new(b)) }
#[inline] fn vhead(v: V) -> V { if let V::C(h, _) = v { (*h).clone() } else { panic!("head of atom") } }
#[inline] fn vtail(v: V) -> V { if let V::C(_, t) = v { (*t).clone() } else { panic!("tail of atom") } }
#[inline] fn viscell(v: V) -> V { if let V::C(..) = v { V::A(0) } else { V::A(1) } }
#[inline] fn viszero(v: &V) -> bool { matches!(v, V::A(0)) }
#[inline] fn vloob(b: bool) -> V { if b { V::A(0) } else { V::A(1) } }
#[inline] fn vinc(a: V) -> V { V::A(na(&a).checked_add(1).expect("inc overflow")) }
#[inline] fn vdec(a: V) -> V { V::A(na(&a).checked_sub(1).expect("dec underflow")) }
#[inline] fn vadd(a: V, b: V) -> V { V::A(na(&a).checked_add(na(&b)).expect("add overflow")) }
#[inline] fn vsub(a: V, b: V) -> V { V::A(na(&a).checked_sub(na(&b)).expect("sub underflow")) }
#[inline] fn vmul(a: V, b: V) -> V { V::A(na(&a).checked_mul(na(&b)).expect("mul overflow")) }
#[inline] fn vdiv(a: V, b: V) -> V { V::A(na(&a).checked_div(na(&b)).expect("div by zero")) }
#[inline] fn vmod(a: V, b: V) -> V { V::A(na(&a).checked_rem(na(&b)).expect("mod by zero")) }
#[inline] fn vlt(a: V, b: V) -> V { vloob(na(&a) < na(&b)) }
fn veq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::A(x), V::A(y)) => x == y,
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
        V::C(h, t) => format!("[{} {}]", vrender(h), vrender(t)),
        V::F(_) => "<fn>".to_string(),
    }
}
"#;

/// Compile a Latte expression (with its library closure) to a standalone Rust program.
pub fn compile_to_rust(expr_src: &str, libs: &[&str]) -> Result<String, String> {
    let program = latte::gather_program(expr_src, libs)?;
    // A local that shadows a primitive name (e.g. `sub` as a substring index) used to
    // force the whole program onto the interpreter. Instead, alpha-rename such binders
    // — including arm parameters — to fresh names so native compilation still applies.
    let program: Vec<(String, Vec<String>, Ast)> = program
        .into_iter()
        .map(|(n, params, b)| {
            let mut ctr = 0usize;
            let mut env: HashMap<String, String> = HashMap::new();
            let params2 = params
                .iter()
                .map(|p| {
                    if prim_arity(p).is_some() {
                        let f = fresh(p, &mut ctr);
                        env.insert(p.clone(), f.clone());
                        f
                    } else {
                        p.clone()
                    }
                })
                .collect();
            let b2 = unshadow(&b, &env, &mut ctr);
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
                if prim_arity(&cnm).is_none() && arm_names.contains(&cnm) && !reachable.contains(&cnm)
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

    out.push_str("\nfn main() {\n    let r = arm___main(V::A(0));\n    println!(\"{}\", vrender(&r));\n}\n");
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
fn parse_canon(s: &str) -> Option<crate::knot::N> {
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
pub fn run_native_noun_opts(expr: &str, libs: &[&str], force_rebuild: bool) -> Option<crate::knot::N> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let src = compile_to_rust(expr, libs).ok()?;
    // Content-addressed: a stable sha3 of the emitted source is the cache key. Identical code →
    // identical key → the existing binary is reused (no recompile); any change to the expression
    // or to a library arm it actually reaches changes the emitted source → new key → a rebuild.
    let key = crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()));
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let bin = dir.join(format!("e{}{}", &key[..32], BIN_EXT));
    if force_rebuild || !bin.exists() {
        // Serialize builds (they're one-time and cached); concurrent identical requests then
        // compile once rather than each spawning their own rustc. Running stays concurrent.
        static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if force_rebuild || !bin.exists() {
            let uniq = CTR.fetch_add(1, Ordering::Relaxed);
            let tag = format!("{}-{}-{}", std::process::id(), uniq, &key[..16]);
            let rs = dir.join(format!("build-{}.rs", tag));
            let tmp_bin = dir.join(format!("build-{}{}", tag, BIN_EXT));
            std::fs::write(&rs, &src).ok()?;
            let st = std::process::Command::new("rustc")
                .args(["-O", "--edition", "2021", "-o"])
                .arg(&tmp_bin)
                .arg(&rs)
                .output()
                .ok()?;
            let _ = std::fs::remove_file(&rs);
            if !st.status.success() {
                let _ = std::fs::remove_file(&tmp_bin);
                return None;
            }
            let _ = std::fs::rename(&tmp_bin, &bin);
        }
    }
    let o = std::process::Command::new(&bin).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&o.stdout);
    parse_canon(&out)
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
            return PathBuf::from(h).join(".cache").join("orpheus").join("anvil");
        }
    }
    std::env::temp_dir().join("orpheus-anvil")
}

#[cfg(test)]
mod tests {
    use super::compile_to_rust;
    use crate::latte;

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
        // Anvil must agree with the interpreter on values whenever BOTH succeed,
        // and must never silently wrap: past the u128 boundary the native
        // backend DECLINES (None -> interpreter fallback) while the interpreter's
        // big-number jets carry on. Domain errors (underflow, divide by zero,
        // dec(0)) must fail on both.
        let cases: &[(&str, Expect)] = &[
            ("(mul 65536 65536)", Expect::BothOk),
            ("(div 100 7)", Expect::BothOk),
            ("(add 340282366920938463463374607431768211454 1)", Expect::BothOk), // == u128::MAX
            ("(add 340282366920938463463374607431768211455 1)", Expect::InterpOnly), // big jets carry on
            ("(mul 18446744073709551616 18446744073709551616)", Expect::InterpOnly),
            ("(sub 3 5)", Expect::BothFail),
            ("(div 5 0)", Expect::BothFail),
            ("(dec 0)", Expect::BothFail),
        ];
        #[derive(Clone, Copy, PartialEq)]
        enum Expect {
            BothOk,
            BothFail,
            InterpOnly, // interpreter succeeds (arbitrary precision); native declines
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
                Expect::InterpOnly => {
                    assert!(iv.is_some(), "{} should succeed on the interpreter (big jets)", e);
                    assert!(nv.is_none(), "{} must DECLINE on the u128-bound native backend, not wrap", e);
                }
            }
        }
    }
}

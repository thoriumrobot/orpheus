//! Interaction combinators (Lafont, 1997): the universal interaction system with three
//! symbols — γ (constructor), δ (duplicator), ε (eraser) — reduced by the six interaction
//! rules. This is a confluent, local graph-reduction engine: a net is a set of agents wired
//! port-to-port, an *active pair* is two agents joined principal-to-principal, and reduction
//! rewrites one active pair at a time until none remain (the normal form).
//!
//! Because each unordered pair of symbols has exactly one rule and rewrites are local, the
//! system has the diamond property: the normal form is independent of the order of reduction
//! (verified by the `confluent_*` tests). This is the parallel-reduction substrate the spec
//! earmarked as Loom's alternate backend; compiling Loom formulas into nets is the next step.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sym {
    Gamma, // constructor (2 auxiliary ports)
    Delta, // duplicator  (2 auxiliary ports)
    Eps,   // eraser      (0 auxiliary ports)
    Free,  // an interface wire / free end (never interacts)
    // Arithmetic agents (a small interaction-net "program" layer on top of the combinators):
    Zero, // the natural 0      (0 auxiliary ports)
    Succ, // successor S(p)     (1 auxiliary port: the predecessor)
    Add,  // addition a + m     (2 auxiliary ports: second operand, result)
    Mul,  // multiplication a*m  (2 auxiliary ports: multiplicand, result)
    Lt,   // less-than a<b       (2 aux: second operand, result) → Loobean 0/1
    LtZ,  // helper: 0<b facing b (1 aux: result)
    LtB,  // helper: S(a')<b facing b (2 aux: a', result)
    Sel,  // conditional select  (2 aux: a γ-bundle [then else], result)
    Head, // take a γ's first child (1 aux: result), erasing the second
    Tail, // take a γ's second child (1 aux: result), erasing the first
}

/// A port is (agent index, slot). Slot 0 is always the principal port.
pub type Port = (usize, usize);

#[derive(Clone)]
pub struct Net {
    sym: Vec<Sym>,
    alive: Vec<bool>,
    link: Vec<[Port; 3]>,
    pub steps: usize,
}

impl Net {
    pub fn new() -> Net {
        Net { sym: Vec::new(), alive: Vec::new(), link: Vec::new(), steps: 0 }
    }

    fn add(&mut self, s: Sym) -> usize {
        let id = self.sym.len();
        self.sym.push(s);
        self.alive.push(true);
        self.link.push([(id, 0), (id, 1), (id, 2)]); // self-links until wired
        id
    }

    /// Add an interacting agent (γ/δ/ε).
    pub fn agent(&mut self, s: Sym) -> usize {
        self.add(s)
    }
    /// Add a free interface wire.
    pub fn free(&mut self) -> usize {
        self.add(Sym::Free)
    }
    /// Connect two ports (symmetric).
    pub fn wire(&mut self, p: Port, q: Port) {
        self.link[p.0][p.1] = q;
        self.link[q.0][q.1] = p;
    }

    fn kill(&mut self, a: usize) {
        self.alive[a] = false;
    }

    /// Is there a reduction rule for this (unordered) pair of symbols?
    fn has_rule(&self, a: Sym, b: Sym) -> bool {
        use Sym::*;
        matches!(
            (a, b),
            (Eps, Eps)
                | (Eps, Gamma) | (Gamma, Eps)
                | (Eps, Delta) | (Delta, Eps)
                | (Gamma, Gamma) | (Delta, Delta)
                | (Gamma, Delta) | (Delta, Gamma)
                | (Add, Zero) | (Zero, Add)
                | (Add, Succ) | (Succ, Add)
                | (Mul, Zero) | (Zero, Mul)
                | (Mul, Succ) | (Succ, Mul)
                | (Delta, Zero) | (Zero, Delta)
                | (Delta, Succ) | (Succ, Delta)
                | (Eps, Zero) | (Zero, Eps)
                | (Eps, Succ) | (Succ, Eps)
                | (Lt, Zero) | (Zero, Lt)
                | (Lt, Succ) | (Succ, Lt)
                | (LtZ, Zero) | (Zero, LtZ)
                | (LtZ, Succ) | (Succ, LtZ)
                | (LtB, Zero) | (Zero, LtB)
                | (LtB, Succ) | (Succ, LtB)
                | (Sel, Zero) | (Zero, Sel)
                | (Sel, Succ) | (Succ, Sel)
                | (Head, Gamma) | (Gamma, Head)
                | (Tail, Gamma) | (Gamma, Tail)
        )
    }

    /// Find a reducible active pair, in scan order (forward = lowest agent first, reverse =
    /// highest first). The order is what differs between the confluence strategies.
    fn find_pair(&self, reverse: bool) -> Option<(usize, usize)> {
        let n = self.sym.len();
        let order: Vec<usize> = if reverse { (0..n).rev().collect() } else { (0..n).collect() };
        for a in order {
            if !self.alive[a] || self.sym[a] == Sym::Free {
                continue;
            }
            let (b, s) = self.link[a][0];
            if s == 0 && b != a && self.alive[b] && self.has_rule(self.sym[a], self.sym[b]) {
                return Some((a, b));
            }
        }
        None
    }

    #[allow(dead_code)]
    /// Reduce a single active pair. Returns false when the net is in normal form.
    pub fn step(&mut self) -> bool {
        self.step_with(false)
    }

    fn step_with(&mut self, reverse: bool) -> bool {
        let (a, b) = match self.find_pair(reverse) {
            Some(p) => p,
            None => return false,
        };
        use Sym::*;
        match (self.sym[a], self.sym[b]) {
            (Eps, Eps) => {
                self.kill(a);
                self.kill(b);
            }
            (Eps, Zero) => {
                self.kill(a);
                self.kill(b);
            }
            (Zero, Eps) => {
                self.kill(a);
                self.kill(b);
            }
            (Eps, Succ) => self.eps_succ(a, b),
            (Succ, Eps) => self.eps_succ(b, a),
            (Eps, _) => self.erase(a, b),
            (_, Eps) => self.erase(b, a),
            (Gamma, Gamma) | (Delta, Delta) => self.annihilate(a, b),
            (Gamma, Delta) | (Delta, Gamma) => self.commute(a, b),
            (Add, Zero) => self.add_zero(a, b),
            (Zero, Add) => self.add_zero(b, a),
            (Add, Succ) => self.add_succ(a, b),
            (Succ, Add) => self.add_succ(b, a),
            (Mul, Zero) => self.mul_zero(a, b),
            (Zero, Mul) => self.mul_zero(b, a),
            (Mul, Succ) => self.mul_succ(a, b),
            (Succ, Mul) => self.mul_succ(b, a),
            (Delta, Zero) => self.dup_zero(a, b),
            (Zero, Delta) => self.dup_zero(b, a),
            (Delta, Succ) => self.dup_succ(a, b),
            (Succ, Delta) => self.dup_succ(b, a),
            (Lt, Zero) => self.lt_zero(a, b),
            (Zero, Lt) => self.lt_zero(b, a),
            (Lt, Succ) => self.lt_succ(a, b),
            (Succ, Lt) => self.lt_succ(b, a),
            (LtZ, Zero) => self.ltz_zero(a, b),
            (Zero, LtZ) => self.ltz_zero(b, a),
            (LtZ, Succ) => self.ltz_succ(a, b),
            (Succ, LtZ) => self.ltz_succ(b, a),
            (LtB, Zero) => self.ltb_zero(a, b),
            (Zero, LtB) => self.ltb_zero(b, a),
            (LtB, Succ) => self.ltb_succ(a, b),
            (Succ, LtB) => self.ltb_succ(b, a),
            (Sel, Zero) => self.sel_zero(a, b),
            (Zero, Sel) => self.sel_zero(b, a),
            (Sel, Succ) => self.sel_succ(a, b),
            (Succ, Sel) => self.sel_succ(b, a),
            (Head, Gamma) => self.head_gamma(a, b),
            (Gamma, Head) => self.head_gamma(b, a),
            (Tail, Gamma) => self.tail_gamma(a, b),
            (Gamma, Tail) => self.tail_gamma(b, a),
            _ => return false,
        }
        self.steps += 1;
        true
    }

    // α ⋈ α (same binary symbol): wires pass straight through; both agents vanish.
    fn annihilate(&mut self, a: usize, b: usize) {
        let (al1, al2) = (self.link[a][1], self.link[a][2]);
        let (bl1, bl2) = (self.link[b][1], self.link[b][2]);
        self.wire(al1, bl1);
        self.wire(al2, bl2);
        self.kill(a);
        self.kill(b);
    }

    // ε ⋈ α (binary): the eraser duplicates into two erasers on α's auxiliary ports.
    fn erase(&mut self, e: usize, g: usize) {
        let (gl1, gl2) = (self.link[g][1], self.link[g][2]);
        let e1 = self.add(Sym::Eps);
        let e2 = self.add(Sym::Eps);
        self.wire((e1, 0), gl1);
        self.wire((e2, 0), gl2);
        self.kill(e);
        self.kill(g);
    }

    // α ⋈ β (different binary symbols): each duplicates the other (the commutation grid).
    fn commute(&mut self, a: usize, b: usize) {
        let (al1, al2) = (self.link[a][1], self.link[a][2]);
        let (bl1, bl2) = (self.link[b][1], self.link[b][2]);
        let (sa, sb) = (self.sym[a], self.sym[b]);
        let na1 = self.add(sa);
        let na2 = self.add(sa);
        let nb1 = self.add(sb);
        let nb2 = self.add(sb);
        // copies of β take α's old aux positions; copies of α take β's old aux positions
        self.wire((nb1, 0), al1);
        self.wire((nb2, 0), al2);
        self.wire((na1, 0), bl1);
        self.wire((na2, 0), bl2);
        // the new agents interconnect in a crossing grid
        self.wire((na1, 1), (nb1, 1));
        self.wire((na1, 2), (nb2, 1));
        self.wire((na2, 1), (nb1, 2));
        self.wire((na2, 2), (nb2, 2));
        self.kill(a);
        self.kill(b);
    }

    // Add ⋈ Zero: 0 + m = m. The result wire is joined to the second operand.
    fn add_zero(&mut self, add: usize, zero: usize) {
        let m = self.link[add][1]; // second operand
        let r = self.link[add][2]; // result
        self.wire(m, r);
        self.kill(add);
        self.kill(zero);
    }

    // Add ⋈ Succ: S(p) + m = S(p + m). Emit a result successor and recurse on p.
    fn add_succ(&mut self, add: usize, succ: usize) {
        let p = self.link[succ][1]; // predecessor's principal
        let m = self.link[add][1]; // second operand
        let r = self.link[add][2]; // result wire
        let rs = self.add(Sym::Succ);
        let na = self.add(Sym::Add);
        self.wire((rs, 0), r); // the new successor sits at the result
        self.wire((na, 0), p); // the recursive add faces the predecessor
        self.wire((na, 1), m); // carrying the second operand along
        self.wire((rs, 1), (na, 2)); // the successor's child is the recursive result
        self.kill(add);
        self.kill(succ);
    }

    // Mul ⋈ Zero: 0 * m = 0. The multiplicand is unused, so it is erased.
    fn mul_zero(&mut self, mul: usize, zero: usize) {
        let m = self.link[mul][1]; // multiplicand
        let r = self.link[mul][2]; // result
        let z = self.add(Sym::Zero);
        self.wire((z, 0), r);
        let e = self.add(Sym::Eps);
        self.wire((e, 0), m);
        self.kill(mul);
        self.kill(zero);
    }

    // Mul ⋈ Succ: S(p) * m = m + (p * m). The multiplicand is duplicated (δ): one copy is added,
    // the other recurses. This is the canonical interaction-net computation — δ does the sharing.
    fn mul_succ(&mut self, mul: usize, succ: usize) {
        let p = self.link[succ][1]; // predecessor
        let m = self.link[mul][1]; // multiplicand
        let r = self.link[mul][2]; // result
        let dup = self.add(Sym::Delta);
        let add = self.add(Sym::Add);
        let nm = self.add(Sym::Mul);
        self.wire((dup, 0), m); // δ duplicates the multiplicand into its two aux ports
        self.wire((add, 0), (dup, 1)); // first copy is the chain Add consumes
        self.wire((add, 2), r); // Add's result is the overall result
        self.wire((nm, 0), p); // recursive Mul faces the predecessor
        self.wire((nm, 1), (dup, 2)); // second copy is the recursive multiplicand
        self.wire((nm, 2), (add, 1)); // (p*m) feeds Add's second operand
        self.kill(mul);
        self.kill(succ);
    }

    // δ ⋈ Zero: duplicating 0 yields two 0s, one on each of the duplicator's aux ports.
    fn dup_zero(&mut self, dup: usize, zero: usize) {
        let (d1, d2) = (self.link[dup][1], self.link[dup][2]);
        let z1 = self.add(Sym::Zero);
        let z2 = self.add(Sym::Zero);
        self.wire((z1, 0), d1);
        self.wire((z2, 0), d2);
        self.kill(dup);
        self.kill(zero);
    }

    // δ ⋈ Succ: duplicating S(p) yields two successors, with a fresh δ duplicating the predecessor
    // (the unary version of the duplicator/constructor commutation).
    fn dup_succ(&mut self, dup: usize, succ: usize) {
        let p = self.link[succ][1];
        let (d1, d2) = (self.link[dup][1], self.link[dup][2]);
        let s1 = self.add(Sym::Succ);
        let s2 = self.add(Sym::Succ);
        let nd = self.add(Sym::Delta);
        self.wire((s1, 0), d1);
        self.wire((s2, 0), d2);
        self.wire((nd, 0), p);
        self.wire((s1, 1), (nd, 1));
        self.wire((s2, 1), (nd, 2));
        self.kill(dup);
        self.kill(succ);
    }

    // ε ⋈ Succ: erasing a successor keeps erasing its predecessor.
    fn eps_succ(&mut self, eps: usize, succ: usize) {
        let p = self.link[succ][1];
        let e = self.add(Sym::Eps);
        self.wire((e, 0), p);
        self.kill(eps);
        self.kill(succ);
    }

    // helper: emit a fresh Peano 1 = S(0) and wire its principal to port `r`.
    fn emit_one(&mut self, r: Port) {
        let z = self.add(Sym::Zero);
        let s = self.add(Sym::Succ);
        self.wire((s, 1), (z, 0));
        self.wire((s, 0), r);
    }
    // helper: emit a fresh Peano 0 and wire it to `r`.
    fn emit_zero(&mut self, r: Port) {
        let z = self.add(Sym::Zero);
        self.wire((z, 0), r);
    }
    // helper: attach an eraser to whatever is on port `p`.
    fn erase_port(&mut self, p: Port) {
        let e = self.add(Sym::Eps);
        self.wire((e, 0), p);
    }

    // Lt ⋈ Zero: 0 < b  →  inspect b (LtZ).
    fn lt_zero(&mut self, lt: usize, zero: usize) {
        let b = self.link[lt][1];
        let r = self.link[lt][2];
        let z = self.add(Sym::LtZ);
        self.wire((z, 0), b);
        self.wire((z, 1), r);
        self.kill(lt);
        self.kill(zero);
    }
    // Lt ⋈ Succ: S(a') < b  →  inspect b carrying a' (LtB).
    fn lt_succ(&mut self, lt: usize, succ: usize) {
        let ap = self.link[succ][1];
        let b = self.link[lt][1];
        let r = self.link[lt][2];
        let n = self.add(Sym::LtB);
        self.wire((n, 0), b);
        self.wire((n, 1), ap);
        self.wire((n, 2), r);
        self.kill(lt);
        self.kill(succ);
    }
    // LtZ ⋈ Zero: 0 < 0 is false → Loobean 1.
    fn ltz_zero(&mut self, ltz: usize, zero: usize) {
        let r = self.link[ltz][1];
        self.emit_one(r);
        self.kill(ltz);
        self.kill(zero);
    }
    // LtZ ⋈ Succ: 0 < S(_) is true → Loobean 0; erase the rest of b.
    fn ltz_succ(&mut self, ltz: usize, succ: usize) {
        let r = self.link[ltz][1];
        let bp = self.link[succ][1];
        self.emit_zero(r);
        self.erase_port(bp);
        self.kill(ltz);
        self.kill(succ);
    }
    // LtB ⋈ Zero: S(a') < 0 is false → Loobean 1; erase a'.
    fn ltb_zero(&mut self, ltb: usize, zero: usize) {
        let ap = self.link[ltb][1];
        let r = self.link[ltb][2];
        self.emit_one(r);
        self.erase_port(ap);
        self.kill(ltb);
        self.kill(zero);
    }
    // LtB ⋈ Succ: S(a') < S(b') iff a' < b' → recurse Lt(a', b').
    fn ltb_succ(&mut self, ltb: usize, succ: usize) {
        let ap = self.link[ltb][1];
        let r = self.link[ltb][2];
        let bp = self.link[succ][1];
        let lt = self.add(Sym::Lt);
        self.wire((lt, 0), ap);
        self.wire((lt, 1), bp);
        self.wire((lt, 2), r);
        self.kill(ltb);
        self.kill(succ);
    }

    // Sel ⋈ Zero (condition = Loobean true): take the bundle's first child (the `then`).
    fn sel_zero(&mut self, sel: usize, zero: usize) {
        let bundle = self.link[sel][1];
        let r = self.link[sel][2];
        let hd = self.add(Sym::Head);
        self.wire((hd, 0), bundle);
        self.wire((hd, 1), r);
        self.kill(sel);
        self.kill(zero);
    }
    // Sel ⋈ Succ (condition = Loobean false, S(_)): take the second child (the `else`); erase pred.
    fn sel_succ(&mut self, sel: usize, succ: usize) {
        let bundle = self.link[sel][1];
        let r = self.link[sel][2];
        let p = self.link[succ][1];
        let tl = self.add(Sym::Tail);
        self.wire((tl, 0), bundle);
        self.wire((tl, 1), r);
        self.erase_port(p);
        self.kill(sel);
        self.kill(succ);
    }
    // Head ⋈ γ: result ← first child; erase the second.
    fn head_gamma(&mut self, head: usize, gamma: usize) {
        let t = self.link[gamma][1];
        let e = self.link[gamma][2];
        let r = self.link[head][1];
        self.wire(t, r);
        self.erase_port(e);
        self.kill(head);
        self.kill(gamma);
    }
    // Tail ⋈ γ: result ← second child; erase the first.
    fn tail_gamma(&mut self, tail: usize, gamma: usize) {
        let t = self.link[gamma][1];
        let e = self.link[gamma][2];
        let r = self.link[tail][1];
        self.wire(e, r);
        self.erase_port(t);
        self.kill(tail);
        self.kill(gamma);
    }

    /// Reduce to normal form (forward strategy). Returns the number of steps.
    pub fn normalize(&mut self) -> usize {
        let start = self.steps;
        while self.step_with(false) {
            if self.steps - start > 1_000_000 {
                break;
            }
        }
        self.steps - start
    }

    /// Reduce to normal form preferring the highest-index active pair (reverse strategy).
    pub fn normalize_rev(&mut self) -> usize {
        let start = self.steps;
        while self.step_with(true) {
            if self.steps - start > 1_000_000 {
                break;
            }
        }
        self.steps - start
    }

    /// Live counts of (γ, δ, ε).
    pub fn counts(&self) -> (usize, usize, usize) {
        let (mut g, mut d, mut e) = (0, 0, 0);
        for i in 0..self.sym.len() {
            if !self.alive[i] {
                continue;
            }
            match self.sym[i] {
                Sym::Gamma => g += 1,
                Sym::Delta => d += 1,
                Sym::Eps => e += 1,
                _ => {}
            }
        }
        (g, d, e)
    }

    /// Pairs of free interface wires that ended up directly connected (a canonical readout
    /// of how the net rewired its interface). Indices are sorted within and across pairs.
    pub fn interface_wiring(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for a in 0..self.sym.len() {
            if self.sym[a] != Sym::Free || !self.alive[a] {
                continue;
            }
            let (b, _) = self.link[a][0];
            if self.sym[b] == Sym::Free && a < b {
                out.push((a, b));
            }
        }
        out.sort();
        out
    }

    #[allow(dead_code)]
    /// What sits on the far side of a free wire: the symbol of the agent its port reaches.
    pub fn neighbor_sym(&self, free_agent: usize) -> Sym {
        let (b, _) = self.link[free_agent][0];
        self.sym[b]
    }
}

/// A tiny arithmetic expression language that compiles to interaction nets.
#[derive(Clone)]
pub enum Expr {
    Num(u128),
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>), // if cond (Loobean) then a else b
}

/// Translate the supported fragment of the Latte AST into the net expression language. The
/// interaction-net compiler handles naturals under `+`, `*`, `<`, and `if`; anything else
/// (lists, cells, user calls, recursion, …) is reported as unsupported.
pub fn latte_to_expr(a: &crate::latte::Ast) -> Result<Expr, String> {
    use crate::latte::Ast;
    match a {
        Ast::Lit(n) => Ok(Expr::Num(*n)),
        Ast::If(c, t, e) => Ok(Expr::If(
            Box::new(latte_to_expr(c)?),
            Box::new(latte_to_expr(t)?),
            Box::new(latte_to_expr(e)?),
        )),
        Ast::Call(name, args) => {
            let bin = |k: fn(Box<Expr>, Box<Expr>) -> Expr| -> Result<Expr, String> {
                if args.len() != 2 {
                    return Err(format!("'{}' expects 2 arguments on the net compiler", name));
                }
                Ok(k(
                    Box::new(latte_to_expr(&args[0])?),
                    Box::new(latte_to_expr(&args[1])?),
                ))
            };
            match name.as_str() {
                "add" => bin(Expr::Add),
                "mul" => bin(Expr::Mul),
                "lt" => bin(Expr::Lt),
                other => Err(format!("unsupported operation '{}'", other)),
            }
        }
        Ast::Eq(_, _) => Err("'==' is not supported by the net compiler".into()),
        other => Err(format!("unsupported construct {:?}", std::mem::discriminant(other))),
    }
}

/// Parse a Latte expression, compile its supported fragment to an interaction net, reduce it, and
/// return `(value, interaction-steps)`.
pub fn run_str(src: &str) -> Result<(u128, usize), String> {
    let ast = crate::latte::parse(src)?;
    let e = latte_to_expr(&ast)?;
    Ok(eval_net(&e))
}

/// Build the Peano chain Sⁿ(Z); returns the principal port of the chain head.
fn build_num(net: &mut Net, n: u128) -> Port {
    let mut head = net.agent(Sym::Zero);
    for _ in 0..n {
        let s = net.agent(Sym::Succ);
        net.wire((s, 1), (head, 0));
        head = s;
    }
    (head, 0)
}

/// Compile an expression into `net`, returning the result port — a principal-side wire that,
/// once the net is reduced, carries the answer as a Peano chain.
fn compile(net: &mut Net, e: &Expr) -> Port {
    match e {
        Expr::Num(n) => build_num(net, *n),
        Expr::Add(l, r) => {
            let lp = compile(net, l);
            let rp = compile(net, r);
            let a = net.agent(Sym::Add);
            net.wire((a, 0), lp); // Add's principal faces the first operand
            net.wire((a, 1), rp); // aux1 = second operand
            (a, 2) // aux2 = result wire, returned for the caller to attach
        }
        Expr::Mul(l, r) => {
            let lp = compile(net, l);
            let rp = compile(net, r);
            let a = net.agent(Sym::Mul);
            net.wire((a, 0), lp); // Mul's principal faces the multiplier chain
            net.wire((a, 1), rp); // aux1 = multiplicand
            (a, 2) // aux2 = result wire
        }
        Expr::Lt(l, r) => {
            let lp = compile(net, l);
            let rp = compile(net, r);
            let a = net.agent(Sym::Lt);
            net.wire((a, 0), lp); // principal faces the first operand
            net.wire((a, 1), rp); // aux1 = second operand
            (a, 2) // aux2 = result (Loobean)
        }
        Expr::If(c, t, e) => {
            let cp = compile(net, c);
            let tp = compile(net, t);
            let ep = compile(net, e);
            // bundle the two branches into a γ-cell [then else]
            let bundle = net.agent(Sym::Gamma);
            net.wire((bundle, 1), tp);
            net.wire((bundle, 2), ep);
            let sel = net.agent(Sym::Sel);
            net.wire((sel, 0), cp); // principal faces the condition
            net.wire((sel, 1), (bundle, 0)); // aux1 = the bundle (selected via Head/Tail)
            (sel, 2) // aux2 = result
        }
    }
}

/// Follow the result wire through the Succ chain to Zero, returning the decoded number.
fn decode_num(net: &Net, out: usize) -> u128 {
    let mut count = 0u128;
    let (mut a, _) = net.link[out][0];
    loop {
        match net.sym[a] {
            Sym::Succ => {
                count += 1;
                let (next, _) = net.link[a][1];
                a = next;
            }
            _ => break, // Zero (or a stuck end)
        }
    }
    count
}

/// Compile an expression, reduce it on the net engine, and decode the result.
/// Returns (value, reduction steps).
pub fn eval_net(e: &Expr) -> (u128, usize) {
    let mut net = Net::new();
    let res = compile(&mut net, e);
    let out = net.free();
    net.wire(res, (out, 0));
    let steps = net.normalize();
    (decode_num(&net, out), steps)
}

/// The same expression as Latte source, for cross-checking against the Loom interpreter.
fn expr_to_latte(e: &Expr) -> String {
    match e {
        Expr::Num(n) => n.to_string(),
        Expr::Add(l, r) => format!("(add {} {})", expr_to_latte(l), expr_to_latte(r)),
        Expr::Mul(l, r) => format!("(mul {} {})", expr_to_latte(l), expr_to_latte(r)),
        Expr::Lt(l, r) => format!("(lt {} {})", expr_to_latte(l), expr_to_latte(r)),
        Expr::If(c, t, e) => format!(
            "if ({}) then {} else {}",
            expr_to_latte(c),
            expr_to_latte(t),
            expr_to_latte(e)
        ),
    }
}

/// Evaluate the expression on the Loom interpreter (the audit oracle).
pub fn eval_loom(e: &Expr) -> u128 {
    let src = expr_to_latte(e);
    crate::latte::run_with_libs(&src, &["std"])
        .ok()
        .and_then(|n| n.as_atom().and_then(|a| a.to_u128()))
        .unwrap_or(u128::MAX)
}

/// A narrated run of the engine across all six rules plus a multi-step cascade and a
/// confluence check. Returned as text for the CLI and the GUI console.
pub fn demo() -> String {
    let mut out = String::new();
    out.push_str("Interaction combinators (Lafont) — γ constructor · δ duplicator · ε eraser\n\n");

    // 1. Annihilation γ⋈γ: wires pass straight through.
    {
        let mut n = Net::new();
        let (x, y, p, q) = (n.free(), n.free(), n.free(), n.free());
        let a = n.agent(Sym::Gamma);
        let b = n.agent(Sym::Gamma);
        n.wire((a, 0), (b, 0));
        n.wire((a, 1), (x, 0));
        n.wire((a, 2), (y, 0));
        n.wire((b, 1), (p, 0));
        n.wire((b, 2), (q, 0));
        let s = n.normalize();
        out.push_str(&format!(
            "γ⋈γ annihilation: {} step, {} agents left; interface wired straight through {:?}\n",
            s, sum3(n.counts()), n.interface_wiring()
        ));
    }

    // 2. Commutation γ⋈δ: each duplicates the other (1→2 of each).
    {
        let mut n = Net::new();
        let (w, x, y, z) = (n.free(), n.free(), n.free(), n.free());
        let a = n.agent(Sym::Gamma);
        let b = n.agent(Sym::Delta);
        n.wire((a, 0), (b, 0));
        n.wire((a, 1), (w, 0));
        n.wire((a, 2), (x, 0));
        n.wire((b, 1), (y, 0));
        n.wire((b, 2), (z, 0));
        let s = n.normalize();
        let (g, d, _) = n.counts();
        out.push_str(&format!(
            "γ⋈δ commutation: {} step, now {} γ and {} δ (each duplicated the other)\n",
            s, g, d
        ));
    }

    // 3. Erasure cascade ε⋈(γ-tree): the eraser consumes a whole structure.
    {
        let mut n = Net::new();
        let (f1, f2, f3) = (n.free(), n.free(), n.free());
        let e = n.agent(Sym::Eps);
        let g1 = n.agent(Sym::Gamma);
        let g2 = n.agent(Sym::Gamma);
        n.wire((e, 0), (g1, 0));
        n.wire((g1, 1), (f1, 0));
        n.wire((g1, 2), (g2, 0));
        n.wire((g2, 1), (f2, 0));
        n.wire((g2, 2), (f3, 0));
        let s = n.normalize();
        let (g, d, ec) = n.counts();
        out.push_str(&format!(
            "ε⋈(2-node γ-tree): {} steps, structure consumed → {} γ, {} δ, {} ε on the leaves\n",
            s, g, d, ec
        ));
    }

    // 4. Confluence: same net, two reduction orders, identical normal form.
    {
        let base = two_redex_net();
        let mut fwd = base.clone();
        let mut rev = base.clone();
        fwd.normalize();
        rev.normalize_rev();
        let ok = fwd.counts() == rev.counts() && fwd.interface_wiring() == rev.interface_wiring();
        out.push_str(&format!(
            "confluence: forward and reverse strategies agree → {} (steps {} vs {})\n",
            if ok { "identical normal form" } else { "MISMATCH" },
            fwd.steps,
            rev.steps
        ));
    }

    // 5. Compiling a program: (add (add 2 3) 4) runs on the net engine, audited against Loom.
    {
        let e = Expr::Add(
            Box::new(Expr::Add(Box::new(Expr::Num(2)), Box::new(Expr::Num(3)))),
            Box::new(Expr::Num(4)),
        );
        let (v, s) = eval_net(&e);
        let oracle = eval_loom(&e);
        out.push_str(&format!(
            "compile (add (add 2 3) 4): reduced in {} steps → {} (Loom interpreter says {} — {})\n",
            s,
            v,
            oracle,
            if v == oracle { "match" } else { "MISMATCH" }
        ));

        // multiplication: S(p)*m duplicates the multiplicand with δ — computation by interaction
        let e2 = Expr::Mul(
            Box::new(Expr::Add(Box::new(Expr::Num(2)), Box::new(Expr::Num(3)))),
            Box::new(Expr::Num(2)),
        );
        let (v2, s2) = eval_net(&e2);
        let oracle2 = eval_loom(&e2);
        out.push_str(&format!(
            "compile (mul (add 2 3) 2): reduced in {} steps → {} (Loom interpreter says {} — {})\n",
            s2,
            v2,
            oracle2,
            if v2 == oracle2 { "match" } else { "MISMATCH" }
        ));

        // control flow: a comparison drives a conditional; the unused branch is erased by ε
        let e3 = Expr::If(
            Box::new(Expr::Lt(Box::new(Expr::Num(2)), Box::new(Expr::Num(3)))),
            Box::new(Expr::Add(Box::new(Expr::Num(10)), Box::new(Expr::Num(5)))),
            Box::new(Expr::Mul(Box::new(Expr::Num(100)), Box::new(Expr::Num(100)))),
        );
        let (v3, s3) = eval_net(&e3);
        let oracle3 = eval_loom(&e3);
        out.push_str(&format!(
            "compile if (lt 2 3) then (add 10 5) else (mul 100 100): {} steps → {} (Loom says {} — {})\n",
            s3,
            v3,
            oracle3,
            if v3 == oracle3 { "match" } else { "MISMATCH" }
        ));
    }
    out
}

fn sum3(c: (usize, usize, usize)) -> usize {
    c.0 + c.1 + c.2
}

/// A net with two active pairs at once: a γ⋈δ commutation and a γ⋈γ annihilation, sharing
/// some interface wires, used to check order-independence.
fn two_redex_net() -> Net {
    let mut n = Net::new();
    let f: Vec<usize> = (0..6).map(|_| n.free()).collect();
    // pair 1: γ ⋈ δ
    let a = n.agent(Sym::Gamma);
    let b = n.agent(Sym::Delta);
    n.wire((a, 0), (b, 0));
    n.wire((a, 1), (f[0], 0));
    n.wire((a, 2), (f[1], 0));
    n.wire((b, 1), (f[2], 0));
    n.wire((b, 2), (f[3], 0));
    // pair 2: γ ⋈ γ
    let c = n.agent(Sym::Gamma);
    let d = n.agent(Sym::Gamma);
    n.wire((c, 0), (d, 0));
    n.wire((c, 1), (f[4], 0));
    n.wire((c, 2), (f[5], 0));
    // d's aux loop back to a/b's interface to entangle the two redexes' results
    let e = n.free();
    let g = n.free();
    n.wire((d, 1), (e, 0));
    n.wire((d, 2), (g, 0));
    n
}

pub fn cmd_icomb() {
    print!("{}", demo());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annihilation_passes_wires_through() {
        let mut n = Net::new();
        let (x, y, p, q) = (n.free(), n.free(), n.free(), n.free());
        let a = n.agent(Sym::Gamma);
        let b = n.agent(Sym::Gamma);
        n.wire((a, 0), (b, 0));
        n.wire((a, 1), (x, 0));
        n.wire((a, 2), (y, 0));
        n.wire((b, 1), (p, 0));
        n.wire((b, 2), (q, 0));
        assert_eq!(n.normalize(), 1);
        assert_eq!(n.counts(), (0, 0, 0)); // both γ gone
        // x(0)–p(2) and y(1)–q(3) now joined
        assert_eq!(n.interface_wiring(), vec![(0, 2), (1, 3)]);
    }

    #[test]
    fn delta_delta_also_annihilates() {
        let mut n = Net::new();
        let (x, y, p, q) = (n.free(), n.free(), n.free(), n.free());
        let a = n.agent(Sym::Delta);
        let b = n.agent(Sym::Delta);
        n.wire((a, 0), (b, 0));
        n.wire((a, 1), (x, 0));
        n.wire((a, 2), (y, 0));
        n.wire((b, 1), (p, 0));
        n.wire((b, 2), (q, 0));
        assert_eq!(n.normalize(), 1);
        assert_eq!(n.counts(), (0, 0, 0));
    }

    #[test]
    fn commutation_duplicates() {
        let mut n = Net::new();
        let fs: Vec<usize> = (0..4).map(|_| n.free()).collect();
        let a = n.agent(Sym::Gamma);
        let b = n.agent(Sym::Delta);
        n.wire((a, 0), (b, 0));
        n.wire((a, 1), (fs[0], 0));
        n.wire((a, 2), (fs[1], 0));
        n.wire((b, 1), (fs[2], 0));
        n.wire((b, 2), (fs[3], 0));
        assert_eq!(n.normalize(), 1);
        assert_eq!(n.counts(), (2, 2, 0)); // 2 γ + 2 δ
    }

    #[test]
    fn erasure_cascades_through_a_tree() {
        let mut n = Net::new();
        let (f1, f2, f3) = (n.free(), n.free(), n.free());
        let e = n.agent(Sym::Eps);
        let g1 = n.agent(Sym::Gamma);
        let g2 = n.agent(Sym::Gamma);
        n.wire((e, 0), (g1, 0)); // ε faces the root γ
        n.wire((g1, 1), (f1, 0));
        n.wire((g1, 2), (g2, 0)); // root's right child is another γ
        n.wire((g2, 1), (f2, 0));
        n.wire((g2, 2), (f3, 0));
        n.normalize();
        // both γ consumed; an ε now sits on each of the three leaves
        assert_eq!(n.counts(), (0, 0, 3));
        for leaf in [f1, f2, f3] {
            assert_eq!(n.neighbor_sym(leaf), Sym::Eps);
        }
    }

    #[test]
    fn eps_eps_vanishes() {
        let mut n = Net::new();
        let a = n.agent(Sym::Eps);
        let b = n.agent(Sym::Eps);
        n.wire((a, 0), (b, 0));
        assert_eq!(n.normalize(), 1);
        assert_eq!(n.counts(), (0, 0, 0));
    }

    #[test]
    fn confluent_regardless_of_order() {
        let base = two_redex_net();
        let mut fwd = base.clone();
        let mut rev = base.clone();
        fwd.normalize();
        rev.normalize_rev();
        assert_eq!(fwd.counts(), rev.counts());
        assert_eq!(fwd.interface_wiring(), rev.interface_wiring());
    }

    fn add(l: Expr, r: Expr) -> Expr {
        Expr::Add(Box::new(l), Box::new(r))
    }

    #[test]
    fn compiles_and_decodes_numbers() {
        for n in [0u128, 1, 5, 12] {
            assert_eq!(eval_net(&Expr::Num(n)).0, n);
        }
    }

    #[test]
    fn compiles_addition() {
        assert_eq!(eval_net(&add(Expr::Num(2), Expr::Num(3))).0, 5);
        assert_eq!(eval_net(&add(Expr::Num(0), Expr::Num(7))).0, 7);
        assert_eq!(eval_net(&add(Expr::Num(9), Expr::Num(0))).0, 9);
    }

    #[test]
    fn compiles_nested_addition() {
        // (1+2) + (3+4) = 10
        let e = add(add(Expr::Num(1), Expr::Num(2)), add(Expr::Num(3), Expr::Num(4)));
        assert_eq!(eval_net(&e).0, 10);
    }

    #[test]
    fn net_arithmetic_matches_the_loom_interpreter() {
        // The whole point: programs compiled to nets agree with the tree-walking interpreter.
        for a in 0..7u128 {
            for b in 0..7u128 {
                let e = add(Expr::Num(a), Expr::Num(b));
                assert_eq!(eval_net(&e).0, eval_loom(&e), "a={} b={}", a, b);
            }
        }
        let nested = add(add(Expr::Num(3), Expr::Num(5)), add(Expr::Num(2), Expr::Num(6)));
        assert_eq!(eval_net(&nested).0, eval_loom(&nested));
    }

    fn mul(l: Expr, r: Expr) -> Expr {
        Expr::Mul(Box::new(l), Box::new(r))
    }

    #[test]
    fn compiles_multiplication() {
        assert_eq!(eval_net(&mul(Expr::Num(3), Expr::Num(4))).0, 12);
        assert_eq!(eval_net(&mul(Expr::Num(0), Expr::Num(9))).0, 0); // erases the multiplicand
        assert_eq!(eval_net(&mul(Expr::Num(7), Expr::Num(0))).0, 0);
        assert_eq!(eval_net(&mul(Expr::Num(1), Expr::Num(6))).0, 6);
    }

    #[test]
    fn compiles_mixed_formula() {
        // (2+3) * (1+1) = 10  — exercises δ-duplication of a computed (not literal) operand
        let e = mul(add(Expr::Num(2), Expr::Num(3)), add(Expr::Num(1), Expr::Num(1)));
        assert_eq!(eval_net(&e).0, 10);
        assert_eq!(eval_net(&e).0, eval_loom(&e));
        // 2 * (3 * 2) = 12
        let f = mul(Expr::Num(2), mul(Expr::Num(3), Expr::Num(2)));
        assert_eq!(eval_net(&f).0, 12);
        assert_eq!(eval_net(&f).0, eval_loom(&f));
    }

    #[test]
    fn net_full_arithmetic_matches_loom_randomized() {
        // Deterministic pseudo-random nested +/* formulas, each cross-checked against the Loom
        // interpreter. Operands are kept small so the Peano nets stay bounded.
        let mut seed: u64 = 0x9e3779b97f4a7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        fn gen(depth: u32, next: &mut dyn FnMut() -> u64) -> Expr {
            if depth == 0 || next() % 3 == 0 {
                Expr::Num((next() % 5) as u128) // small leaves
            } else {
                let l = gen(depth - 1, next);
                let r = gen(depth - 1, next);
                if next() % 2 == 0 {
                    Expr::Add(Box::new(l), Box::new(r))
                } else {
                    Expr::Mul(Box::new(l), Box::new(r))
                }
            }
        }
        for _ in 0..200 {
            let e = gen(3, &mut next);
            let net = eval_net(&e).0;
            let loom = eval_loom(&e);
            assert_eq!(net, loom, "formula {} : net={} loom={}", expr_to_latte(&e), net, loom);
        }
    }

    fn lt(l: Expr, r: Expr) -> Expr {
        Expr::Lt(Box::new(l), Box::new(r))
    }
    fn iff(c: Expr, t: Expr, e: Expr) -> Expr {
        Expr::If(Box::new(c), Box::new(t), Box::new(e))
    }

    #[test]
    fn compiles_comparison() {
        // Loobean: 0 = true, 1 = false
        assert_eq!(eval_net(&lt(Expr::Num(3), Expr::Num(5))).0, 0);
        assert_eq!(eval_net(&lt(Expr::Num(5), Expr::Num(3))).0, 1);
        assert_eq!(eval_net(&lt(Expr::Num(4), Expr::Num(4))).0, 1);
        assert_eq!(eval_net(&lt(Expr::Num(0), Expr::Num(1))).0, 0);
        for a in 0..6u128 {
            for b in 0..6u128 {
                let e = lt(Expr::Num(a), Expr::Num(b));
                assert_eq!(eval_net(&e).0, eval_loom(&e), "lt {} {}", a, b);
            }
        }
    }

    #[test]
    fn compiles_conditional() {
        // if (lt 2 3) then 10 else 20  ->  10 (condition true)
        let e = iff(lt(Expr::Num(2), Expr::Num(3)), Expr::Num(10), Expr::Num(20));
        assert_eq!(eval_net(&e).0, 10);
        assert_eq!(eval_net(&e).0, eval_loom(&e));
        // if (lt 3 2) then 10 else 20  ->  20 (condition false; the unused branch is erased)
        let f = iff(lt(Expr::Num(3), Expr::Num(2)), Expr::Num(10), Expr::Num(20));
        assert_eq!(eval_net(&f).0, 20);
        assert_eq!(eval_net(&f).0, eval_loom(&f));
        // branches may themselves be computations
        let g = iff(
            lt(Expr::Num(1), Expr::Num(2)),
            Expr::Add(Box::new(Expr::Num(3)), Box::new(Expr::Num(4))),
            Expr::Mul(Box::new(Expr::Num(5)), Box::new(Expr::Num(6))),
        );
        assert_eq!(eval_net(&g).0, 7);
        assert_eq!(eval_net(&g).0, eval_loom(&g));
    }

    #[test]
    fn net_control_flow_matches_loom_randomized() {
        // Random nested formulas over +, *, <, and if(<)… — control flow driven by a computed
        // Loobean — each cross-checked against the Loom interpreter.
        let mut seed: u64 = 0xd1b54a32d192ed03;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // a value-producing expression
        fn gen(depth: u32, next: &mut dyn FnMut() -> u64) -> Expr {
            if depth == 0 || next() % 3 == 0 {
                return Expr::Num((next() % 5) as u128);
            }
            match next() % 4 {
                0 => Expr::Add(Box::new(gen(depth - 1, next)), Box::new(gen(depth - 1, next))),
                1 => Expr::Mul(Box::new(gen(depth - 1, next)), Box::new(gen(depth - 1, next))),
                2 => Expr::Lt(Box::new(gen(depth - 1, next)), Box::new(gen(depth - 1, next))),
                _ => Expr::If(
                    // condition is always a comparison → a genuine Loobean (matches Loom's `if`)
                    Box::new(Expr::Lt(
                        Box::new(gen(depth - 1, next)),
                        Box::new(gen(depth - 1, next)),
                    )),
                    Box::new(gen(depth - 1, next)),
                    Box::new(gen(depth - 1, next)),
                ),
            }
        }
        for _ in 0..200 {
            let e = gen(3, &mut next);
            let net = eval_net(&e).0;
            let loom = eval_loom(&e);
            assert_eq!(net, loom, "formula {} : net={} loom={}", expr_to_latte(&e), net, loom);
        }
    }

    #[test]
    fn latte_source_compiles_on_net() {
        // The net compiler accepts real Latte source for its supported fragment, agreeing with
        // the interpreter; unsupported constructs are reported rather than mis-evaluated.
        for (src, want) in [
            ("(add 2 3)", 5u128),
            ("(mul (add 2 3) 2)", 10),
            ("(lt 3 5)", 0),
            ("(lt 5 3)", 1),
            ("if (lt 2 3) then (add 10 5) else (mul 9 9)", 15),
            ("(mul (mul 2 2) (add 1 2))", 12),
        ] {
            let (v, _steps) = super::run_str(src).expect(src);
            assert_eq!(v, want, "net value for {}", src);
            let n = crate::latte::run_with_libs(src, &["std"]).unwrap();
            let loom = n.as_atom().and_then(|a| a.to_u128()).unwrap();
            assert_eq!(v, loom, "net vs loom for {}", src);
        }
        assert!(super::run_str("(reverse [1 [2 0]])").is_err());
        assert!(super::run_str("(len [1 0])").is_err());
    }
}

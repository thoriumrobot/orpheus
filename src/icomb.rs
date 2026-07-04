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
    Pred, // predecessor / dec: faces a number, 1 aux = result
    Sub,  // monus a∸b          (2 aux: second operand, result)
    SubB, // helper: S(a')∸b facing b (2 aux: a', result)
    Eq,   // equality a==b       (2 aux: second operand, result) → Loobean 0/1
    EqZ,  // helper: 0==b facing b (1 aux: result)
    EqB,  // helper: S(a')==b facing b (2 aux: a', result)
    Ref(u32), // a reference to a top-level definition (defs[i]); unrolled lazily (HVM-style)
    // ---- native machine numbers (the HVM2 idea: numbers as atomic agents) ----
    Lit(u64),       // a 64-bit number (0 auxiliary ports) — one agent, however large
    NOp(NK),        // a native binary op facing operand a (2 aux: operand b, result)
    NOp2(NK, u64),  // …with a captured: faces operand b (1 aux: result)
}

/// The native ALU: each op is ONE interaction once both operands are literal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NK {
    Add,
    Sub, // monus (saturating), matching the Peano agents and Latte's `sub`
    Mul,
    Div,
    Mod,
    Lt, // → loobean Lit(0|1)
    Eq, // → loobean Lit(0|1)
}

fn nk_apply(k: NK, a: u64, b: u64) -> u64 {
    match k {
        NK::Add => a.wrapping_add(b),
        NK::Sub => a.saturating_sub(b),
        NK::Mul => a.wrapping_mul(b),
        // division by zero diverges in Latte; the net leaves 0 (and the audit
        // against the interpreter reports the disagreement honestly)
        NK::Div => if b == 0 { 0 } else { a / b },
        NK::Mod => if b == 0 { 0 } else { a % b },
        NK::Lt => if a < b { 0 } else { 1 },
        NK::Eq => if a == b { 0 } else { 1 },
    }
}

/// A port is (agent index, slot). Slot 0 is always the principal port.
pub type Port = (usize, usize);

#[derive(Clone)]
pub struct Net {
    sym: Vec<Sym>,
    alive: Vec<bool>,
    link: Vec<[Port; 3]>,
    pub steps: usize,
    defs: Vec<R>, // top-level definitions referenced by Ref(i); unrolled lazily
    work: Vec<usize>, // active-pair worklist: agents whose principal was (re)wired
    native: bool, // numbers as Lit agents + native ALU ops (HVM2-style) vs Peano chains
}

impl Net {
    pub fn new() -> Net {
        Net { sym: Vec::new(), alive: Vec::new(), link: Vec::new(), steps: 0, defs: Vec::new(), work: Vec::new(), native: false }
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
    /// Connect two ports (symmetric). Wiring a principal port (slot 0) may create a new active
    /// pair, so the affected agents are pushed onto the worklist for the reducer to revisit.
    pub fn wire(&mut self, p: Port, q: Port) {
        self.link[p.0][p.1] = q;
        self.link[q.0][q.1] = p;
        if p.1 == 0 {
            self.work.push(p.0);
        }
        if q.1 == 0 {
            self.work.push(q.0);
        }
    }

    fn kill(&mut self, a: usize) {
        self.alive[a] = false;
    }

    /// Is there a reduction rule for this (unordered) pair of symbols?
    fn has_rule(&self, a: Sym, b: Sym) -> bool {
        use Sym::*;
        // A Ref interacts with whatever sits on its principal port: a consumer triggers a
        // (lazy) unrolling, an eraser collects it (so recursion halts). This is the HVM-style
        // REF extension that gives net-level fixpoints / general recursion.
        if matches!(a, Ref(_)) || matches!(b, Ref(_)) {
            return true;
        }
        matches!(
            (a, b),
            (Eps, Eps)
                | (Eps, Gamma) | (Gamma, Eps)
                | (Eps, Delta) | (Delta, Eps)
                | (Gamma, Gamma) | (Delta, Delta)
                | (Gamma, Delta) | (Delta, Gamma)
                | (Eps, Lit(_)) | (Lit(_), Eps)
                | (Delta, Lit(_)) | (Lit(_), Delta)
                | (NOp(_), Lit(_)) | (Lit(_), NOp(_))
                | (NOp2(_, _), Lit(_)) | (Lit(_), NOp2(_, _))
                | (Sel, Lit(_)) | (Lit(_), Sel)
                | (Pred, Lit(_)) | (Lit(_), Pred)
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
                | (Pred, Zero) | (Zero, Pred)
                | (Pred, Succ) | (Succ, Pred)
                | (Eps, Pred) | (Pred, Eps)
                | (Sub, Zero) | (Zero, Sub)
                | (Sub, Succ) | (Succ, Sub)
                | (SubB, Zero) | (Zero, SubB)
                | (SubB, Succ) | (Succ, SubB)
                | (Eq, Zero) | (Zero, Eq)
                | (Eq, Succ) | (Succ, Eq)
                | (EqZ, Zero) | (Zero, EqZ)
                | (EqZ, Succ) | (Succ, EqZ)
                | (EqB, Zero) | (Zero, EqB)
                | (EqB, Succ) | (Succ, EqB)
        )
    }

    /// Find a reducible active pair, in scan order (forward = lowest agent first, reverse =
    /// highest first). The order is what differs between the confluence strategies.
    /// Find a reducible active pair. The forward strategy drains an **active-pair worklist**
    /// (amortised O(1) per step instead of a full O(n) scan), falling back to a single linear
    /// scan when the worklist empties — which both confirms the normal form and guarantees no
    /// pair is ever missed. The reverse strategy keeps the plain scan (used to demonstrate that
    /// reduction order does not affect the result). Neither path allocates per step.
    fn find_pair(&mut self, reverse: bool) -> Option<(usize, usize)> {
        if !reverse {
            while let Some(a) = self.work.pop() {
                if self.alive[a] && self.sym[a] != Sym::Free {
                    let (b, s) = self.link[a][0];
                    if s == 0 && b != a && self.alive[b] && self.has_rule(self.sym[a], self.sym[b]) {
                        return Some((a, b));
                    }
                }
            }
        }
        let n = self.sym.len();
        let mut k = 0;
        while k < n {
            let a = if reverse { n - 1 - k } else { k };
            k += 1;
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
            // ---- HVM-style REF unrolling (net-level fixpoints) -----------------
            // Ref meets an eraser → collect it (this is what lets recursion halt).
            (Ref(_), Eps) => self.ref_erase(a, b),
            (Eps, Ref(_)) => self.ref_erase(b, a),
            // Ref meets any consumer → unroll its definition into the net.
            (Ref(i), _) => self.ref_expand(a, i, b),
            (_, Ref(i)) => self.ref_expand(b, i, a),
            // ---- predecessor (dec) --------------------------------------------
            (Pred, Zero) => self.pred_zero(a, b),
            (Zero, Pred) => self.pred_zero(b, a),
            (Pred, Succ) => self.pred_succ(a, b),
            (Succ, Pred) => self.pred_succ(b, a),
            (Eps, Pred) => self.erase(a, b),
            (Pred, Eps) => self.erase(b, a),
            (Eps, Eps) => {
                self.kill(a);
                self.kill(b);
            }
            (Eps, Zero) => {
                self.kill(a);
                self.kill(b);
            }
            // ---- native-number interactions (each arithmetic op: O(1) steps) ----
            (Eps, Lit(_)) | (Lit(_), Eps) => {
                self.kill(a);
                self.kill(b);
            }
            (Delta, Lit(n)) => self.dup_lit(a, b, n),
            (Lit(n), Delta) => self.dup_lit(b, a, n),
            (NOp(k), Lit(n)) => self.nop_lit(a, b, k, n),
            (Lit(n), NOp(k)) => self.nop_lit(b, a, k, n),
            (NOp2(k, x), Lit(n)) => self.nop2_lit(a, b, k, x, n),
            (Lit(n), NOp2(k, x)) => self.nop2_lit(b, a, k, x, n),
            (Sel, Lit(c)) => self.sel_lit(a, b, c),
            (Lit(c), Sel) => self.sel_lit(b, a, c),
            (Pred, Lit(n)) => self.pred_lit(a, b, n),
            (Lit(n), Pred) => self.pred_lit(b, a, n),
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
            (Sub, Zero) => self.sub_zero(a, b),
            (Zero, Sub) => self.sub_zero(b, a),
            (Sub, Succ) => self.sub_succ(a, b),
            (Succ, Sub) => self.sub_succ(b, a),
            (SubB, Zero) => self.subb_zero(a, b),
            (Zero, SubB) => self.subb_zero(b, a),
            (SubB, Succ) => self.subb_succ(a, b),
            (Succ, SubB) => self.subb_succ(b, a),
            (Eq, Zero) => self.eq_zero(a, b),
            (Zero, Eq) => self.eq_zero(b, a),
            (Eq, Succ) => self.eq_succ(a, b),
            (Succ, Eq) => self.eq_succ(b, a),
            (EqZ, Zero) => self.eqz_zero(a, b),
            (Zero, EqZ) => self.eqz_zero(b, a),
            (EqZ, Succ) => self.eqz_succ(a, b),
            (Succ, EqZ) => self.eqz_succ(b, a),
            (EqB, Zero) => self.eqb_zero(a, b),
            (Zero, EqB) => self.eqb_zero(b, a),
            (EqB, Succ) => self.eqb_succ(a, b),
            (Succ, EqB) => self.eqb_succ(b, a),
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
    // ---- native-number rule bodies -------------------------------------------
    // δ ⋈ Lit: copy the number to both auxiliaries (numbers are atomic data).
    fn dup_lit(&mut self, dup: usize, lit: usize, n: u64) {
        let (l1, l2) = (self.link[dup][1], self.link[dup][2]);
        let c1 = self.add(Sym::Lit(n));
        let c2 = self.add(Sym::Lit(n));
        self.wire((c1, 0), l1);
        self.wire((c2, 0), l2);
        self.kill(dup);
        self.kill(lit);
    }
    // NOp(k) ⋈ Lit(a): capture a and turn to face operand b.
    fn nop_lit(&mut self, op: usize, lit: usize, k: NK, n: u64) {
        let bsrc = self.link[op][1];
        let r = self.link[op][2];
        let o2 = self.add(Sym::NOp2(k, n));
        self.wire((o2, 0), bsrc);
        self.wire((o2, 1), r);
        self.kill(op);
        self.kill(lit);
    }
    // NOp2(k, a) ⋈ Lit(b): ONE interaction computes the result.
    fn nop2_lit(&mut self, op: usize, lit: usize, k: NK, x: u64, n: u64) {
        let r = self.link[op][1];
        let out = self.add(Sym::Lit(nk_apply(k, x, n)));
        self.wire((out, 0), r);
        self.kill(op);
        self.kill(lit);
    }
    // Sel ⋈ Lit: loobean 0 takes the `then` box, anything else the `else` box.
    fn sel_lit(&mut self, sel: usize, lit: usize, c: u64) {
        let bundle = self.link[sel][1];
        let r = self.link[sel][2];
        let proj = self.add(if c == 0 { Sym::Head } else { Sym::Tail });
        self.wire((proj, 0), bundle);
        self.wire((proj, 1), r);
        self.kill(sel);
        self.kill(lit);
    }
    // Pred ⋈ Lit: saturating decrement, like the Peano rule at zero.
    fn pred_lit(&mut self, pred: usize, lit: usize, n: u64) {
        let r = self.link[pred][1];
        let out = self.add(Sym::Lit(n.saturating_sub(1)));
        self.wire((out, 0), r);
        self.kill(pred);
        self.kill(lit);
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

    // ε ⋈ Ref: collect the reference (do NOT unroll). Its argument subnet is erased. This is
    // the rule that lets a recursive function terminate: the unselected branch's recursive Ref
    // meets an eraser and is dropped instead of expanding forever.
    fn ref_erase(&mut self, r: usize, eps: usize) {
        let arg = self.link[r][1];
        let e2 = self.add(Sym::Eps);
        self.wire((e2, 0), arg);
        self.kill(r);
        self.kill(eps);
    }

    // Ref ⋈ (any consumer): unroll the definition `defs[idx]` into the net, wiring its parameter
    // input to the Ref's argument and its result to the consumer that demanded it. The body may
    // itself contain Refs (recursive calls), which expand on demand in turn.
    fn ref_expand(&mut self, r: usize, idx: u32, _other: usize) {
        let target = self.link[r][0]; // the exact port that demanded this reference
        let arg = self.link[r][1];
        let rp = self.build_def(idx as usize, arg);
        self.wire(rp, target);
        self.kill(r);
    }

    // Pred ⋈ Zero: dec(0) = 0.
    fn pred_zero(&mut self, pred: usize, zero: usize) {
        let res = self.link[pred][1];
        let z = self.add(Sym::Zero);
        self.wire((z, 0), res);
        self.kill(pred);
        self.kill(zero);
    }
    // Pred ⋈ Succ: dec(S(p)) = p.
    fn pred_succ(&mut self, pred: usize, succ: usize) {
        let res = self.link[pred][1];
        let p = self.link[succ][1];
        self.wire(p, res);
        self.kill(pred);
        self.kill(succ);
    }

    // ---- monus (truncated subtraction), Peano-lockstep like Lt --------------
    // Sub ⋈ Zero: 0 ∸ b = 0; the second operand is unused, so erase it.
    fn sub_zero(&mut self, sb: usize, zero: usize) {
        let b = self.link[sb][1];
        let r = self.link[sb][2];
        self.emit_zero(r);
        self.erase_port(b);
        self.kill(sb);
        self.kill(zero);
    }
    // Sub ⋈ Succ: S(a') ∸ b → inspect b carrying a' (SubB).
    fn sub_succ(&mut self, sb: usize, succ: usize) {
        let ap = self.link[succ][1];
        let b = self.link[sb][1];
        let r = self.link[sb][2];
        let n = self.add(Sym::SubB);
        self.wire((n, 0), b);
        self.wire((n, 1), ap);
        self.wire((n, 2), r);
        self.kill(sb);
        self.kill(succ);
    }
    // SubB ⋈ Zero: S(a') ∸ 0 = S(a') — rebuild the successor at the result.
    fn subb_zero(&mut self, sbb: usize, zero: usize) {
        let ap = self.link[sbb][1];
        let r = self.link[sbb][2];
        let s = self.add(Sym::Succ);
        self.wire((s, 1), ap);
        self.wire((s, 0), r);
        self.kill(sbb);
        self.kill(zero);
    }
    // SubB ⋈ Succ: S(a') ∸ S(b') = a' ∸ b' — recurse in lockstep.
    fn subb_succ(&mut self, sbb: usize, succ: usize) {
        let bp = self.link[succ][1];
        let ap = self.link[sbb][1];
        let r = self.link[sbb][2];
        let n = self.add(Sym::Sub);
        self.wire((n, 0), ap);
        self.wire((n, 1), bp);
        self.wire((n, 2), r);
        self.kill(sbb);
        self.kill(succ);
    }

    // ---- equality, Peano-lockstep ------------------------------------------
    // Eq ⋈ Zero: 0 == b → inspect b (EqZ).
    fn eq_zero(&mut self, eq: usize, zero: usize) {
        let b = self.link[eq][1];
        let r = self.link[eq][2];
        let z = self.add(Sym::EqZ);
        self.wire((z, 0), b);
        self.wire((z, 1), r);
        self.kill(eq);
        self.kill(zero);
    }
    // Eq ⋈ Succ: S(a') == b → inspect b carrying a' (EqB).
    fn eq_succ(&mut self, eq: usize, succ: usize) {
        let ap = self.link[succ][1];
        let b = self.link[eq][1];
        let r = self.link[eq][2];
        let n = self.add(Sym::EqB);
        self.wire((n, 0), b);
        self.wire((n, 1), ap);
        self.wire((n, 2), r);
        self.kill(eq);
        self.kill(succ);
    }
    // EqZ ⋈ Zero: 0 == 0 is true → Loobean 0.
    fn eqz_zero(&mut self, eqz: usize, zero: usize) {
        let r = self.link[eqz][1];
        self.emit_zero(r);
        self.kill(eqz);
        self.kill(zero);
    }
    // EqZ ⋈ Succ: 0 == S(_) is false → Loobean 1; erase the predecessor.
    fn eqz_succ(&mut self, eqz: usize, succ: usize) {
        let p = self.link[succ][1];
        let r = self.link[eqz][1];
        self.emit_one(r);
        self.erase_port(p);
        self.kill(eqz);
        self.kill(succ);
    }
    // EqB ⋈ Zero: S(_) == 0 is false → Loobean 1; erase a'.
    fn eqb_zero(&mut self, eqb: usize, zero: usize) {
        let ap = self.link[eqb][1];
        let r = self.link[eqb][2];
        self.emit_one(r);
        self.erase_port(ap);
        self.kill(eqb);
        self.kill(zero);
    }
    // EqB ⋈ Succ: S(a') == S(b') = a' == b' — recurse in lockstep.
    fn eqb_succ(&mut self, eqb: usize, succ: usize) {
        let bp = self.link[succ][1];
        let ap = self.link[eqb][1];
        let r = self.link[eqb][2];
        let n = self.add(Sym::Eq);
        self.wire((n, 0), ap);
        self.wire((n, 1), bp);
        self.wire((n, 2), r);
        self.kill(eqb);
        self.kill(succ);
    }

    /// Register a definition body, returning its index for a `Ref`.
    fn push_def(&mut self, body: R) -> u32 {
        self.defs.push(body);
        (self.defs.len() - 1) as u32
    }

    /// Fan `src` out into `k` copies using δ duplicators (k=0 erases it, k=1 returns it).
    fn fan(&mut self, src: Port, k: usize) -> Vec<Port> {
        if k == 0 {
            let e = self.add(Sym::Eps);
            self.wire((e, 0), src);
            return Vec::new();
        }
        if k == 1 {
            return vec![src];
        }
        let mut copies = Vec::with_capacity(k);
        let mut cur = src;
        for _ in 0..k - 1 {
            let d = self.add(Sym::Delta);
            self.wire((d, 0), cur);
            copies.push((d, 1));
            cur = (d, 2);
        }
        copies.push(cur);
        copies
    }

    /// Instantiate definition `idx` applied to the value at `arg`, returning the result port.
    fn build_def(&mut self, idx: usize, arg: Port) -> Port {
        let body = self.defs[idx].clone();
        let k = nparams(&body);
        let mut supply = self.fan(arg, k);
        supply.reverse(); // so build() pops parameter copies in source order
        self.build(&body, &mut supply)
    }

    /// Compile a recursion expression into the net, consuming parameter copies from `supply`.
    /// `if` is compiled lazily by hoisting both branches into `Ref` closures: the selector wires
    /// the taken branch's Ref to the result (so it unrolls) and erases the other (so it halts).
    /// Emit a number: a single Lit agent (native mode) or a Peano chain.
    fn emit_num(&mut self, n: u128) -> Port {
        if self.native {
            let a = self.agent(Sym::Lit(n as u64));
            (a, 0)
        } else {
            build_num(self, n)
        }
    }
    /// Emit a binary arithmetic node: the native ALU agent or the Peano agent.
    fn emit_bin(&mut self, peano: Sym, k: NK, lp: Port, rp: Port) -> Port {
        let a = if self.native { self.add(Sym::NOp(k)) } else { self.add(peano) };
        self.wire((a, 0), lp);
        self.wire((a, 1), rp);
        (a, 2)
    }

    fn build(&mut self, r: &R, supply: &mut Vec<Port>) -> Port {
        match r {
            R::Num(n) => self.emit_num(*n),
            R::Param => supply.pop().expect("net recursion: parameter supply underflow"),
            R::Add(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Add, NK::Add, lp, rp)
            }
            R::Mul(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Mul, NK::Mul, lp, rp)
            }
            R::Lt(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Lt, NK::Lt, lp, rp)
            }
            R::Dec(x) => {
                let xp = self.build(x, supply);
                let p = self.add(Sym::Pred);
                self.wire((p, 0), xp);
                (p, 1)
            }
            R::SubK(x, k) => {
                let mut cur = self.build(x, supply);
                for _ in 0..*k {
                    let p = self.add(Sym::Pred);
                    self.wire((p, 0), cur);
                    cur = (p, 1);
                }
                cur
            }
            R::Sub(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Sub, NK::Sub, lp, rp)
            }
            R::Div(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Sub, NK::Div, lp, rp) // (Peano fallback never reached)
            }
            R::Mod(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Sub, NK::Mod, lp, rp)
            }
            R::Eq(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                self.emit_bin(Sym::Eq, NK::Eq, lp, rp)
            }
            R::Pair(l, rr) => {
                let lp = self.build(l, supply);
                let rp = self.build(rr, supply);
                let g = self.add(Sym::Gamma);
                self.wire((g, 1), lp);
                self.wire((g, 2), rp);
                (g, 0)
            }
            R::Fst(x) => {
                let xp = self.build(x, supply);
                let h = self.add(Sym::Head);
                self.wire((h, 0), xp);
                (h, 1)
            }
            R::Snd(x) => {
                let xp = self.build(x, supply);
                let t = self.add(Sym::Tail);
                self.wire((t, 0), xp);
                (t, 1)
            }
            // ParamN(i, k): one δ-copy of the packed argument, projected to component i of k
            // (a right-nested γ-pair chain: Snd^i, then Fst unless it is the last component).
            R::ParamN(i, k) => {
                let mut cur = supply.pop().expect("net recursion: parameter supply underflow");
                for _ in 0..*i {
                    let t = self.add(Sym::Tail);
                    self.wire((t, 0), cur);
                    cur = (t, 1);
                }
                if *k > 1 && i + 1 < *k {
                    let h = self.add(Sym::Head);
                    self.wire((h, 0), cur);
                    cur = (h, 1);
                }
                cur
            }
            R::Rec(a) => {
                let ap = self.build(a, supply);
                let rf = self.add(Sym::Ref(0)); // the main function is always defs[0]
                self.wire((rf, 1), ap);
                (rf, 0)
            }
            R::Call(idx, a) => {
                let ap = self.build(a, supply);
                let rf = self.add(Sym::Ref(*idx as u32));
                self.wire((rf, 1), ap);
                (rf, 0)
            }
            R::If(c, t, e) => {
                let cp = self.build(c, supply);
                // each branch closure gets one copy of the (packed) argument; in a
                // zero-parameter context the closures are closed, so a dummy 0 is
                // supplied and simply erased when the branch's def has no parameters.
                let dummy = |net: &mut Net| {
                    let z = net.add(Sym::Zero);
                    (z, 0)
                };
                let base_arg = match supply.pop() { Some(p) => p, None => dummy(self) };
                let rec_arg = match supply.pop() { Some(p) => p, None => dummy(self) };
                let base_idx = self.push_def((**t).clone());
                let rec_idx = self.push_def((**e).clone());
                let bref = self.add(Sym::Ref(base_idx));
                self.wire((bref, 1), base_arg);
                let rref = self.add(Sym::Ref(rec_idx));
                self.wire((rref, 1), rec_arg);
                let bundle = self.add(Sym::Gamma);
                self.wire((bundle, 1), (bref, 0));
                self.wire((bundle, 2), (rref, 0));
                let sel = self.add(Sym::Sel);
                self.wire((sel, 0), cp);
                self.wire((sel, 1), (bundle, 0));
                (sel, 2)
            }
        }
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

    // ========================================================================
    // PARALLEL REDUCTION. Interaction nets are uniformly confluent: distinct
    // active pairs touch disjoint redexes, so they may be rewritten in parallel
    // and any order yields the same normal form (Lafont; the property HVM2's
    // lock-free reducer exploits on GPUs). This reducer runs BATCHES of
    // non-conflicting pairs across std::thread workers:
    //   1. drain candidate pairs from the active-pair worklist; CLAIM each
    //      pair's footprint — the two agents plus every agent their auxiliary
    //      ports touch — via atomic flags (conflicts defer to a later batch);
    //   2. pre-grow the arenas and hand each pair a private block of fresh
    //      agent ids, so threads allocate without locks;
    //   3. rewrite all claimed pairs in parallel — each touches only claimed
    //      or thread-private indices — recording newly-wired principals in a
    //      THREAD-LOCAL worklist, merged after the batch (no shared pushes);
    //   4. when the worklist runs dry, one linear scan certifies the normal
    //      form (so the fast path can never miss a redex).
    // `Ref` expansions build unbounded subnets, so those reduce sequentially
    // between batches; the Peano lockstep rules do too (they are inherently
    // serial chains). The Lit/γ/δ/ε rules — everything the native compiler
    // emits — run parallel. Equivalence with the sequential engine is enforced
    // by test (and guaranteed by uniform confluence).
    // ========================================================================
    pub fn normalize_parallel(&mut self, threads: usize) -> usize {
        use std::sync::atomic::{AtomicBool, Ordering};
        let start = self.steps;
        let threads = threads.max(1);
        const PRIVATE: usize = 6; // max fresh agents any parallel rule allocates
        const BATCH: usize = 8192;
        let mut claims: Vec<AtomicBool> = Vec::new();

        loop {
            if self.steps - start > 8_000_000 {
                break;
            }
            // ---- gather candidates: worklist first, full scan only to certify ----
            let mut pairs: Vec<(usize, usize)> = Vec::new();
            let mut seq: Vec<(usize, usize)> = Vec::new();
            let push_candidate = |net: &Net, a: usize, pairs: &mut Vec<(usize, usize)>, seq: &mut Vec<(usize, usize)>| {
                if !net.alive[a] {
                    return;
                }
                let (b, sb) = net.link[a][0];
                if sb != 0 || b == a || !net.alive[b] || net.link[b][0] != (a, 0) {
                    return;
                }
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                if !net.has_rule(net.sym[a], net.sym[b]) {
                    return;
                }
                if Net::par_rule(net.sym[a], net.sym[b]) {
                    pairs.push((a, b));
                } else {
                    seq.push((a, b));
                }
            };
            while let Some(a) = self.work.pop() {
                push_candidate(self, a, &mut pairs, &mut seq);
                if pairs.len() >= BATCH {
                    break;
                }
            }
            if pairs.is_empty() && seq.is_empty() {
                // the work queue ran dry: certify (or refill) with one linear scan;
                // an empty scan proves the net is fully reduced. (A scan runs at
                // most once per drained queue — progress in between refills it.)
                for a in 0..self.sym.len() {
                    push_candidate(self, a, &mut pairs, &mut seq);
                }
                if pairs.is_empty() && seq.is_empty() {
                    break;
                }
            }
            pairs.sort_unstable();
            pairs.dedup();
            // ---- sequential bucket: Refs and the Peano chains ----
            for &(a, b) in &seq {
                if self.alive[a] && self.alive[b] && self.link[a][0] == (b, 0) {
                    let _ = self.step_with(false);
                }
            }
            if pairs.is_empty() {
                continue;
            }
            // small batches: thread spawn costs more than the work
            if pairs.len() < 128 || threads == 1 {
                for &(a, b) in &pairs {
                    if self.alive[a] && self.alive[b] && self.link[a][0] == (b, 0) {
                        let _ = self.step_with(false);
                    }
                }
                continue;
            }
            // ---- keep the arena tight: compaction pays for the linear scans ----
            let dead = self.alive.iter().filter(|a| !**a).count();
            if self.sym.len() > 65_536 && dead * 2 > self.sym.len() {
                // candidate indices become stale across a compaction; re-queue them
                for &(a, _) in pairs.iter() {
                    self.work.push(a);
                }
                self.compact();
                claims.clear();
                continue;
            }
            // ---- claim non-conflicting footprints ----
            let n0 = self.sym.len();
            if claims.len() < n0 {
                claims.resize_with(n0, || AtomicBool::new(false));
            }
            let mut batch: Vec<(usize, usize)> = Vec::new();
            let mut claimed: Vec<usize> = Vec::new();
            let mut deferred: Vec<(usize, usize)> = Vec::new();
            'pairs: for &(a, b) in &pairs {
                if !self.alive[a] || !self.alive[b] || self.link[a][0] != (b, 0) {
                    continue;
                }
                let mut footprint = vec![a, b];
                for &ag in &[a, b] {
                    for slot in 1..3 {
                        let (n, _) = self.link[ag][slot];
                        footprint.push(n);
                    }
                }
                footprint.sort_unstable();
                footprint.dedup();
                let mut got = 0usize;
                for &f in &footprint {
                    if claims[f].compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                        got += 1;
                    } else {
                        for &g in footprint.iter().take(got) {
                            claims[g].store(false, Ordering::Release);
                        }
                        // conflicted pairs run inline after the batch (no requeue churn)
                        deferred.push((a, b));
                        continue 'pairs;
                    }
                }
                claimed.extend_from_slice(&footprint);
                batch.push((a, b));
            }
            if batch.is_empty() {
                continue;
            }
            // ---- pre-grow arenas: a private id block per pair ----
            let extra = batch.len() * PRIVATE;
            self.sym.resize(n0 + extra, Sym::Eps);
            self.alive.resize(n0 + extra, false);
            for i in 0..extra {
                let id = n0 + i;
                self.link.push([(id, 0), (id, 1), (id, 2)]);
            }
            // ---- the parallel phase ----
            let me = self as *mut Net as usize;
            let chunk = batch.len().div_ceil(threads);
            let mut merged: Vec<usize> = Vec::new();
            std::thread::scope(|sc| {
                let mut handles = Vec::new();
                for (t, slab) in batch.chunks(chunk).enumerate() {
                    let base = n0 + t * chunk * PRIVATE;
                    handles.push(sc.spawn(move || {
                        // SAFETY: every link/alive/sym index a rewrite touches is either
                        // claimed for this pair (no other thread holds it) or inside this
                        // thread's private id block; the arenas were pre-sized so no Vec
                        // reallocation happens; new principal wirings are recorded in the
                        // thread-local list, never the shared worklist.
                        let net: &mut Net = unsafe { &mut *(me as *mut Net) };
                        let mut fresh = base;
                        let mut local: Vec<usize> = Vec::with_capacity(slab.len() * 2);
                        for &(a, b) in slab {
                            net.rewrite_claimed(a, b, &mut fresh, &mut local);
                        }
                        (slab.len(), local)
                    }));
                }
                for h in handles {
                    let (n, local) = h.join().unwrap();
                    self.steps += n;
                    merged.extend(local);
                }
            });
            self.work.extend(merged);
            for &c in &claimed {
                claims[c].store(false, Ordering::Release);
            }
            for &(a, b) in &deferred {
                if self.alive[a] && self.alive[b] && self.link[a][0] == (b, 0) {
                    let _ = self.step_with(false);
                }
            }
        }
        self.steps - start
    }

    /// Compact the arena: drop dead agents, remap live indices (the parallel
    /// engine's private id blocks leave gaps; without this the arena — and every
    /// linear scan over it — grows with TOTAL work instead of LIVE size).
    fn compact(&mut self) {
        let n = self.sym.len();
        let mut remap: Vec<usize> = vec![usize::MAX; n];
        let mut live = 0usize;
        for i in 0..n {
            if self.alive[i] {
                remap[i] = live;
                live += 1;
            }
        }
        if live == n {
            return;
        }
        let mut sym = Vec::with_capacity(live);
        let mut alive = Vec::with_capacity(live);
        let mut link = Vec::with_capacity(live);
        for i in 0..n {
            if !self.alive[i] {
                continue;
            }
            sym.push(self.sym[i]);
            alive.push(true);
            let mut l = self.link[i];
            for slot in l.iter_mut() {
                let (t, ts) = *slot;
                // dangling links to dead agents collapse to self (free ends)
                *slot = if t < n && remap[t] != usize::MAX { (remap[t], ts) } else { (remap[i], 0) };
            }
            link.push(l);
        }
        let mut work: Vec<usize> = self
            .work
            .iter()
            .filter_map(|&w| if w < n && remap[w] != usize::MAX { Some(remap[w]) } else { None })
            .collect();
        work.sort_unstable();
        work.dedup();
        self.sym = sym;
        self.alive = alive;
        self.link = link;
        self.work = work;
    }

    /// Which symbol pairs the parallel engine handles (the bounded, non-Peano rules).
    fn par_rule(a: Sym, b: Sym) -> bool {
        use Sym::*;
        if matches!(a, Ref(_)) || matches!(b, Ref(_)) {
            return false;
        }
        matches!(
            (a, b),
            (Eps, _) | (_, Eps)
                | (Gamma, Gamma) | (Delta, Delta)
                | (Gamma, Delta) | (Delta, Gamma)
                | (Delta, Lit(_)) | (Lit(_), Delta)
                | (NOp(_), Lit(_)) | (Lit(_), NOp(_))
                | (NOp2(_, _), Lit(_)) | (Lit(_), NOp2(_, _))
                | (Sel, Lit(_)) | (Lit(_), Sel)
                | (Pred, Lit(_)) | (Lit(_), Pred)
        )
    }

    // a wire that records principal endpoints into a LOCAL list (parallel-safe)
    fn wire_local(&mut self, p: Port, q: Port, wl: &mut Vec<usize>) {
        self.link[p.0][p.1] = q;
        self.link[q.0][q.1] = p;
        if p.1 == 0 {
            wl.push(p.0);
        }
        if q.1 == 0 {
            wl.push(q.0);
        }
    }
    fn alloc_at(&mut self, fresh: &mut usize, sym: Sym) -> usize {
        let id = *fresh;
        *fresh += 1;
        self.sym[id] = sym;
        self.alive[id] = true;
        id
    }

    /// One bounded rewrite of a claimed pair: thread-private allocation, local worklist.
    fn rewrite_claimed(&mut self, a: usize, b: usize, fresh: &mut usize, wl: &mut Vec<usize>) {
        use Sym::*;
        match (self.sym[a], self.sym[b]) {
            (Eps, Eps) | (Eps, Zero) | (Zero, Eps) | (Eps, Lit(_)) | (Lit(_), Eps) => {
                self.kill(a);
                self.kill(b);
            }
            (Gamma, Gamma) | (Delta, Delta) => {
                let (al1, al2) = (self.link[a][1], self.link[a][2]);
                let (bl1, bl2) = (self.link[b][1], self.link[b][2]);
                self.wire_local(al1, bl1, wl);
                self.wire_local(al2, bl2, wl);
                self.kill(a);
                self.kill(b);
            }
            (Gamma, Delta) => self.commute_claimed(a, b, fresh, wl),
            (Delta, Gamma) => self.commute_claimed(b, a, fresh, wl),
            (Eps, _) => self.erase_claimed(a, b, fresh, wl),
            (_, Eps) => self.erase_claimed(b, a, fresh, wl),
            (Delta, Lit(n)) => self.dup_lit_claimed(a, b, n, fresh, wl),
            (Lit(n), Delta) => self.dup_lit_claimed(b, a, n, fresh, wl),
            (NOp(k), Lit(n)) => self.nop_lit_claimed(a, b, k, n, fresh, wl),
            (Lit(n), NOp(k)) => self.nop_lit_claimed(b, a, k, n, fresh, wl),
            (NOp2(k, x), Lit(n)) => self.nop2_lit_claimed(a, b, k, x, n, fresh, wl),
            (Lit(n), NOp2(k, x)) => self.nop2_lit_claimed(b, a, k, x, n, fresh, wl),
            (Sel, Lit(c)) => self.sel_lit_claimed(a, b, c, fresh, wl),
            (Lit(c), Sel) => self.sel_lit_claimed(b, a, c, fresh, wl),
            (Pred, Lit(n)) => self.pred_lit_claimed(a, b, n, fresh, wl),
            (Lit(n), Pred) => self.pred_lit_claimed(b, a, n, fresh, wl),
            _ => {} // filtered out by par_rule; unreachable
        }
    }
    fn commute_claimed(&mut self, g: usize, d: usize, fresh: &mut usize, wl: &mut Vec<usize>) {
        let (g1, g2) = (self.link[g][1], self.link[g][2]);
        let (d1, d2) = (self.link[d][1], self.link[d][2]);
        let ng1 = self.alloc_at(fresh, Sym::Gamma);
        let ng2 = self.alloc_at(fresh, Sym::Gamma);
        let nd1 = self.alloc_at(fresh, Sym::Delta);
        let nd2 = self.alloc_at(fresh, Sym::Delta);
        self.wire_local((nd1, 0), g1, wl);
        self.wire_local((nd2, 0), g2, wl);
        self.wire_local((ng1, 0), d1, wl);
        self.wire_local((ng2, 0), d2, wl);
        self.wire_local((ng1, 1), (nd1, 1), wl);
        self.wire_local((ng1, 2), (nd2, 1), wl);
        self.wire_local((ng2, 1), (nd1, 2), wl);
        self.wire_local((ng2, 2), (nd2, 2), wl);
        self.kill(g);
        self.kill(d);
    }
    fn erase_claimed(&mut self, e: usize, g: usize, fresh: &mut usize, wl: &mut Vec<usize>) {
        match self.sym[g] {
            Sym::Zero | Sym::Lit(_) => {}
            Sym::Succ | Sym::Pred | Sym::Head | Sym::Tail | Sym::NOp2(_, _) => {
                let t = self.link[g][1];
                let ne = self.alloc_at(fresh, Sym::Eps);
                self.wire_local((ne, 0), t, wl);
            }
            _ => {
                let (t1, t2) = (self.link[g][1], self.link[g][2]);
                let e1 = self.alloc_at(fresh, Sym::Eps);
                let e2 = self.alloc_at(fresh, Sym::Eps);
                self.wire_local((e1, 0), t1, wl);
                self.wire_local((e2, 0), t2, wl);
            }
        }
        self.kill(e);
        self.kill(g);
    }
    fn dup_lit_claimed(&mut self, dup: usize, lit: usize, n: u64, fresh: &mut usize, wl: &mut Vec<usize>) {
        let (l1, l2) = (self.link[dup][1], self.link[dup][2]);
        let c1 = self.alloc_at(fresh, Sym::Lit(n));
        let c2 = self.alloc_at(fresh, Sym::Lit(n));
        self.wire_local((c1, 0), l1, wl);
        self.wire_local((c2, 0), l2, wl);
        self.kill(dup);
        self.kill(lit);
    }
    fn nop_lit_claimed(&mut self, op: usize, lit: usize, k: NK, n: u64, fresh: &mut usize, wl: &mut Vec<usize>) {
        let bsrc = self.link[op][1];
        let r = self.link[op][2];
        let o2 = self.alloc_at(fresh, Sym::NOp2(k, n));
        self.wire_local((o2, 0), bsrc, wl);
        self.wire_local((o2, 1), r, wl);
        self.kill(op);
        self.kill(lit);
    }
    fn nop2_lit_claimed(&mut self, op: usize, lit: usize, k: NK, x: u64, n: u64, fresh: &mut usize, wl: &mut Vec<usize>) {
        let r = self.link[op][1];
        let out = self.alloc_at(fresh, Sym::Lit(nk_apply(k, x, n)));
        self.wire_local((out, 0), r, wl);
        self.kill(op);
        self.kill(lit);
    }
    fn sel_lit_claimed(&mut self, sel: usize, lit: usize, c: u64, fresh: &mut usize, wl: &mut Vec<usize>) {
        let bundle = self.link[sel][1];
        let r = self.link[sel][2];
        let proj = self.alloc_at(fresh, if c == 0 { Sym::Head } else { Sym::Tail });
        self.wire_local((proj, 0), bundle, wl);
        self.wire_local((proj, 1), r, wl);
        self.kill(sel);
        self.kill(lit);
    }
    fn pred_lit_claimed(&mut self, pred: usize, lit: usize, n: u64, fresh: &mut usize, wl: &mut Vec<usize>) {
        let r = self.link[pred][1];
        let out = self.alloc_at(fresh, Sym::Lit(n.saturating_sub(1)));
        self.wire_local((out, 0), r, wl);
        self.kill(pred);
        self.kill(lit);
    }

    /// Reduce to normal form, then force any recursive `Ref` left dangling on the lazy spine
    /// (e.g. an additive recursive call whose result sits in a `Succ` predecessor and so never
    /// gained principal-to-principal contact). After a full `normalize`, every `if` selector has
    /// already fired, so the only un-expanded references are genuine demanded-but-not-yet-forced
    /// tails; expanding one and renormalizing advances the recursion one level. Loops until the
    /// result is fully evaluated or the step budget is exhausted.
    pub fn normalize_forcing(&mut self) -> usize {
        let start = self.steps;
        loop {
            self.normalize();
            if self.steps - start > 4_000_000 {
                break;
            }
            // find a reference still facing a live, non-eraser agent (a forced demand)
            let mut stuck = None;
            for a in 0..self.sym.len() {
                if self.alive[a] && matches!(self.sym[a], Sym::Ref(_)) {
                    let (b, _) = self.link[a][0];
                    if b != a && self.alive[b] && self.sym[b] != Sym::Eps {
                        stuck = Some(a);
                        break;
                    }
                }
            }
            match stuck {
                Some(a) => {
                    if let Sym::Ref(i) = self.sym[a] {
                        self.ref_expand(a, i, a);
                        self.steps += 1;
                    }
                }
                None => break,
            }
        }
        self.steps - start
    }
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

/// Translate the supported fragment of the Latte AST into the net expression language.
///
/// The net's *primitive* agents are naturals under `+` (`add`), `*` (`mul`), `<` (`lt`) and
/// `if`. On top of that, this lowering accepts a richer source fragment by **expanding it into
/// those primitives at compile time**: `let`, `+(…)`, `==`, let-bound **user functions**
/// (inlined), and `loop … again(…)` (**bounded unrolling**, the net's form of recursion).
/// Operators with no native agent (`sub`, `dec`, `div`, `mod`) are constant-folded.
///
/// `if` is **lazy**: when the condition reduces to a constant (which it does for the closed
/// expressions the net evaluates), only the *taken* branch is lowered, so the untaken branch is
/// never built into the net and never reduced. Anything outside the fragment (lists, cells,
/// cords, `case`, unbounded recursion) is reported as unsupported rather than mis-evaluated.
pub fn latte_to_expr(a: &crate::latte::Ast) -> Result<Expr, String> {
    let mut env = LowerEnv { vars: Vec::new(), funcs: Vec::new() };
    match lower(a, &mut env, NET_FUEL)? {
        Lowered::Val(e) => Ok(e),
        Lowered::Again(_) => Err("`again` used outside of a `loop`".into()),
    }
}

const NET_FUEL: u32 = 500_000;

/// The result of lowering one node: either a value-expression, or an `again(…)` continuation
/// (only meaningful as the tail of a `loop` body).
enum Lowered {
    Val(Expr),
    Again(Vec<Expr>),
}

struct LowerEnv {
    vars: Vec<(String, Expr)>,                       // let-/param-/loop-bound values
    funcs: Vec<(String, Vec<String>, crate::latte::Ast)>, // let-bound gates (inlined on call)
}

/// Constant-fold a net expression if it is closed (used to decide `if`/`==` and the
/// non-native arithmetic ops). Returns `None` only if a sub-term is non-constant.
fn fold(e: &Expr) -> Option<u128> {
    Some(match e {
        Expr::Num(n) => *n,
        Expr::Add(l, r) => fold(l)?.checked_add(fold(r)?)?,
        Expr::Mul(l, r) => fold(l)?.checked_mul(fold(r)?)?,
        Expr::Lt(l, r) => if fold(l)? < fold(r)? { 0 } else { 1 }, // Loobean: 0 = true
        Expr::If(c, t, e) => if fold(c)? == 0 { fold(t)? } else { fold(e)? },
    })
}

fn lower_val(a: &crate::latte::Ast, env: &mut LowerEnv, fuel: u32) -> Result<Expr, String> {
    match lower(a, env, fuel)? {
        Lowered::Val(e) => Ok(e),
        Lowered::Again(_) => Err("`again` is only allowed as the tail of a `loop`".into()),
    }
}

fn lower(a: &crate::latte::Ast, env: &mut LowerEnv, fuel: u32) -> Result<Lowered, String> {
    use crate::latte::Ast;
    match a {
        Ast::Lit(n) => Ok(Lowered::Val(Expr::Num(*n))),
        Ast::Nil => Ok(Lowered::Val(Expr::Num(0))),
        Ast::Var(name) => env
            .vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, e)| Lowered::Val(e.clone()))
            .ok_or_else(|| format!("unbound variable '{}' on the net compiler", name)),
        Ast::Inc(e) => Ok(Lowered::Val(Expr::Add(Box::new(lower_val(e, env, fuel)?), Box::new(Expr::Num(1))))),
        Ast::Fast(_, body) => lower(body, env, fuel), // jet hint is irrelevant on the net
        Ast::Eq(x, y) => {
            let xv = lower_val(x, env, fuel)?;
            let yv = lower_val(y, env, fuel)?;
            match (fold(&xv), fold(&yv)) {
                (Some(a), Some(b)) => Ok(Lowered::Val(Expr::Num(if a == b { 0 } else { 1 }))),
                _ => Err("'==' on the net needs constant-foldable operands".into()),
            }
        }
        Ast::If(c, t, e) => {
            let cv = lower_val(c, env, fuel)?;
            match fold(&cv) {
                // lazy: build only the taken branch, so the other is never reduced
                Some(0) => lower(t, env, fuel),
                Some(_) => lower(e, env, fuel),
                None => {
                    // a non-constant condition: fall back to the strict net `if` (bundles both)
                    let tv = lower_val(t, env, fuel)?;
                    let ev = lower_val(e, env, fuel)?;
                    Ok(Lowered::Val(Expr::If(Box::new(cv), Box::new(tv), Box::new(ev))))
                }
            }
        }
        Ast::Let(name, val, body) => match val.as_ref() {
            // a let-bound gate becomes an inlinable user function
            Ast::Gate(params, gbody) => {
                env.funcs.push((name.clone(), params.clone(), (**gbody).clone()));
                let r = lower(body, env, fuel);
                env.funcs.pop();
                r
            }
            _ => {
                let v = lower_val(val, env, fuel)?;
                env.vars.push((name.clone(), v));
                let r = lower(body, env, fuel);
                env.vars.pop();
                r
            }
        },
        Ast::Again(args) => {
            let mut vs = Vec::with_capacity(args.len());
            for a in args {
                vs.push(lower_val(a, env, fuel)?);
            }
            Ok(Lowered::Again(vs))
        }
        Ast::Loop(binds, body) => {
            let names: Vec<String> = binds.iter().map(|(n, _)| n.clone()).collect();
            let mut vals: Vec<Expr> = Vec::with_capacity(binds.len());
            for (_, v) in binds {
                vals.push(lower_val(v, env, fuel)?);
            }
            let mut f = fuel;
            loop {
                if f == 0 {
                    return Err("loop did not converge within the net compiler's unrolling budget".into());
                }
                f -= 1;
                let base = env.vars.len();
                for (n, v) in names.iter().zip(vals.iter()) {
                    env.vars.push((n.clone(), v.clone()));
                }
                let r = lower(body, env, f);
                env.vars.truncate(base);
                match r? {
                    Lowered::Val(e) => return Ok(Lowered::Val(e)),
                    Lowered::Again(newvals) => {
                        if newvals.len() != names.len() {
                            return Err("`again` arity does not match the loop bindings".into());
                        }
                        vals = newvals;
                    }
                }
            }
        }
        Ast::Call(name, args) => {
            let bin = |env: &mut LowerEnv, k: fn(Box<Expr>, Box<Expr>) -> Expr| -> Result<Lowered, String> {
                if args.len() != 2 {
                    return Err(format!("'{}' expects 2 arguments on the net compiler", name));
                }
                Ok(Lowered::Val(k(
                    Box::new(lower_val(&args[0], env, fuel)?),
                    Box::new(lower_val(&args[1], env, fuel)?),
                )))
            };
            // fold an arithmetic op that has no native net agent
            let folded2 = |env: &mut LowerEnv, op: fn(u128, u128) -> Option<u128>| -> Result<Lowered, String> {
                if args.len() != 2 {
                    return Err(format!("'{}' expects 2 arguments", name));
                }
                let a = lower_val(&args[0], env, fuel)?;
                let b = lower_val(&args[1], env, fuel)?;
                match (fold(&a), fold(&b)) {
                    (Some(x), Some(y)) => op(x, y)
                        .map(|v| Lowered::Val(Expr::Num(v)))
                        .ok_or_else(|| format!("'{}' is undefined here (underflow / divide-by-zero)", name)),
                    _ => Err(format!("'{}' on the net needs constant-foldable operands", name)),
                }
            };
            match name.as_str() {
                "add" => bin(env, Expr::Add),
                "mul" => bin(env, Expr::Mul),
                "lt" => bin(env, Expr::Lt),
                "sub" => folded2(env, |x, y| x.checked_sub(y)),
                "div" => folded2(env, |x, y| if y == 0 { None } else { Some(x / y) }),
                "mod" => folded2(env, |x, y| if y == 0 { None } else { Some(x % y) }),
                "dec" => {
                    if args.len() != 1 {
                        return Err("'dec' expects 1 argument".into());
                    }
                    let a = lower_val(&args[0], env, fuel)?;
                    fold(&a)
                        .and_then(|x| x.checked_sub(1))
                        .map(|v| Lowered::Val(Expr::Num(v)))
                        .ok_or_else(|| "'dec' on the net needs a constant-foldable, non-zero operand".into())
                }
                // a user-defined (let-bound) function: inline its body
                other => {
                    let func = env.funcs.iter().rev().find(|(n, _, _)| n == other).cloned();
                    match func {
                        Some((_, params, body)) => {
                            if params.len() != args.len() {
                                return Err(format!("'{}' expects {} argument(s)", other, params.len()));
                            }
                            let mut argv = Vec::with_capacity(args.len());
                            for a in args {
                                argv.push(lower_val(a, env, fuel)?);
                            }
                            let base = env.vars.len();
                            for (p, v) in params.iter().zip(argv) {
                                env.vars.push((p.clone(), v));
                            }
                            let r = lower(&body, env, fuel);
                            env.vars.truncate(base);
                            r
                        }
                        None => Err(format!("unsupported operation '{}' on the net compiler", other)),
                    }
                }
            }
        }
        other => Err(format!("unsupported construct {:?} on the net compiler", std::mem::discriminant(other))),
    }
}

/// Parse a Latte expression, compile its supported fragment to an interaction net, reduce it,
/// and return `(value, interaction-steps)`.
///
/// Three compilers are tried in order:
///   1. `run_rec` — the classic single-parameter self-recursive function (REF unrolling);
///   2. `run_general` — the general compiler: lazy boxed `if`, multi-binding loops, user
///      functions of any arity, dynamic `sub`/`==`/`div`/`mod` (γ-pairs as net data);
///   3. the simple `Expr` compiler — kept as a final fallback and as the audit oracle.
/// Run on the parallel batch reducer with `threads` workers (Ref expansions
/// interleave sequentially). Same result as `run_str` by uniform confluence.
pub fn run_str_parallel(src: &str, threads: usize) -> Result<(u128, usize), String> {
    let ast = crate::latte::parse(src)?;
    let mut env = GEnv { defs: Vec::new(), funcs: Vec::new(), div_idx: None, mod_idx: None, native: true };
    let mut scope = GScope { params: Vec::new(), lets: Vec::new(), loop_idx: None };
    let main = glower(&ast, &mut env, &mut scope)?;
    let mut net = Net::new();
    net.native = true;
    net.defs = env.defs;
    let mut supply: Vec<Port> = Vec::new();
    let rp = net.build(&main, &mut supply);
    let out = net.free();
    net.wire(rp, (out, 0));
    let mut rounds = 0;
    loop {
        net.normalize_parallel(threads);
        let before = net.steps;
        net.normalize_forcing(); // forces dangling lazy Refs, exactly as run_general
        rounds += 1;
        if net.steps == before || rounds > 8 {
            break;
        }
    }
    let v = decode_num(&net, out);
    if v == u128::MAX {
        return Err("net: did not reduce to a number within the step budget".into());
    }
    Ok((v, net.steps))
}

pub fn run_str(src: &str) -> Result<(u128, usize), String> {
    // The general compiler with NATIVE NUMBERS leads: every arithmetic op is one
    // interaction (the HVM2 idea), so this is both the most capable and the most
    // efficient path. The Peano paths remain as fallbacks and pedagogy.
    match run_general(src) {
        Ok(res) => Ok(res),
        Err(gerr) => {
            // classic single-parameter net recursion (Peano, REF unrolling)
            if let Some(res) = run_rec(src)? {
                return Ok(res);
            }
            // the simple expression path can still serve shapes both refuse
            let ast = crate::latte::parse(src)?;
            match latte_to_expr(&ast) {
                Ok(e) => Ok(eval_net(&e)),
                Err(_) => Err(gerr),
            }
        }
    }
}

/// The pure-Peano evaluation order (the pedagogical mode): unary numbers, lockstep
/// arithmetic agents, generated recursive `div`/`mod`.
pub fn run_str_peano(src: &str) -> Result<(u128, usize), String> {
    if let Some(res) = run_rec(src)? {
        return Ok(res);
    }
    match run_general_mode(src, false) {
        Ok(res) => Ok(res),
        Err(gerr) => {
            let ast = crate::latte::parse(src)?;
            match latte_to_expr(&ast) {
                Ok(e) => Ok(eval_net(&e)),
                Err(_) => Err(gerr),
            }
        }
    }
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
    if let Sym::Lit(n) = net.sym[a] {
        return n as u128;
    }
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

/// A recursion-capable expression over a single natural parameter. Compiled to the net by
/// HVM-style REF unrolling — genuine net-level fixpoints, where the *reducer* expands the
/// recursive definition on demand and collects it (against an eraser) at the base case.
#[derive(Clone)]
enum R {
    Num(u128),
    Param,                       // the single parameter (= ParamN(0) over an unpacked argument)
    ParamN(usize, usize),        // parameter i of k: project Snd^i (then Fst if i < k-1) of the packed γ-pair argument
    Add(Box<R>, Box<R>),
    Mul(Box<R>, Box<R>),
    Lt(Box<R>, Box<R>),
    Sub(Box<R>, Box<R>),         // monus, on the net's Sub agents
    Eq(Box<R>, Box<R>),          // equality, on the net's Eq agents → Loobean
    Dec(Box<R>),
    SubK(Box<R>, u128),
    Pair(Box<R>, Box<R>),        // a γ-cell: pairs as net data (tuples, packed arguments)
    Fst(Box<R>),                 // Head ⋈ γ projection
    Snd(Box<R>),                 // Tail ⋈ γ projection
    Rec(Box<R>),                 // recursive call of defs[0] (back-compat)
    Call(usize, Box<R>),         // call defs[i] with one (possibly packed) argument
    If(Box<R>, Box<R>, Box<R>),
    Div(Box<R>, Box<R>),         // native mode only: one NOp(Div) agent
    Mod(Box<R>, Box<R>),         // native mode only: one NOp(Mod) agent
}

/// Parameter copies the *top level* of `r` consumes. `if` does not descend into its branches
/// (they become separate closures), but reserves one copy for each branch closure's argument.
fn nparams(r: &R) -> usize {
    match r {
        R::Num(_) => 0,
        R::Param | R::ParamN(_, _) => 1,
        R::Add(a, b) | R::Mul(a, b) | R::Lt(a, b) | R::Sub(a, b) | R::Eq(a, b) | R::Pair(a, b)
        | R::Div(a, b) | R::Mod(a, b) => nparams(a) + nparams(b),
        R::Dec(a) | R::SubK(a, _) | R::Rec(a) | R::Call(_, a) | R::Fst(a) | R::Snd(a) => nparams(a),
        R::If(c, _, _) => nparams(c) + 2,
    }
}

/// Convert a Latte gate body to `R`: the parameter becomes `Param`, self-calls become `Rec`.
fn to_r(a: &crate::latte::Ast, param: &str, fname: &str) -> Result<R, String> {
    use crate::latte::Ast;
    match a {
        Ast::Lit(n) => Ok(R::Num(*n)),
        Ast::Var(x) if x == param => Ok(R::Param),
        Ast::Var(x) => Err(format!(
            "net recursion: free variable '{}' (only the parameter '{}' is supported)",
            x, param
        )),
        Ast::Inc(e) => Ok(R::Add(Box::new(to_r(e, param, fname)?), Box::new(R::Num(1)))),
        Ast::Fast(_, b) => to_r(b, param, fname),
        Ast::If(c, t, e) => Ok(R::If(
            Box::new(to_r(c, param, fname)?),
            Box::new(to_r(t, param, fname)?),
            Box::new(to_r(e, param, fname)?),
        )),
        Ast::Call(name, args) if name == fname => {
            if args.len() != 1 {
                return Err("net recursion: the recursive function must take one argument".into());
            }
            Ok(R::Rec(Box::new(to_r(&args[0], param, fname)?)))
        }
        Ast::Call(name, args) => {
            let two = |k: fn(Box<R>, Box<R>) -> R| -> Result<R, String> {
                if args.len() != 2 {
                    return Err(format!("net recursion: '{}' expects 2 arguments", name));
                }
                Ok(k(
                    Box::new(to_r(&args[0], param, fname)?),
                    Box::new(to_r(&args[1], param, fname)?),
                ))
            };
            match name.as_str() {
                "add" => two(R::Add),
                "mul" => two(R::Mul),
                "lt" => two(R::Lt),
                "dec" => {
                    if args.len() != 1 {
                        return Err("net recursion: 'dec' expects 1 argument".into());
                    }
                    Ok(R::Dec(Box::new(to_r(&args[0], param, fname)?)))
                }
                "sub" => match args.get(1) {
                    Some(Ast::Lit(k)) if args.len() == 2 => {
                        Ok(R::SubK(Box::new(to_r(&args[0], param, fname)?), *k))
                    }
                    _ => Err("net recursion: 'sub' on the net needs a literal second operand".into()),
                },
                other => Err(format!("net recursion: unsupported operation '{}'", other)),
            }
        }
        Ast::Eq(_, _) => {
            Err("net recursion: use '(lt n 1)' rather than '==' for the base test".into())
        }
        other => Err(format!(
            "net recursion: unsupported construct {:?}",
            std::mem::discriminant(other)
        )),
    }
}

pub(crate) fn mentions(a: &crate::latte::Ast, name: &str) -> bool {
    use crate::latte::Ast;
    match a {
        Ast::Call(n, args) => n == name || args.iter().any(|x| mentions(x, name)),
        Ast::If(c, t, e) => mentions(c, name) || mentions(t, name) || mentions(e, name),
        Ast::Inc(e) | Ast::Fast(_, e) | Ast::Head(e) | Ast::Tail(e) | Ast::IsCell(e) => {
            mentions(e, name)
        }
        Ast::Let(_, v, b) => mentions(v, name) || mentions(b, name),
        Ast::Gate(_, b) => mentions(b, name),
        Ast::Eq(x, y) => mentions(x, name) || mentions(y, name),
        Ast::Loop(bs, b) => bs.iter().any(|(_, e)| mentions(e, name)) || mentions(b, name),
        Ast::Again(es) | Ast::Tuple(es) => es.iter().any(|x| mentions(x, name)),
        Ast::Case(s, arms) => mentions(s, name) || arms.iter().any(|(_, e)| mentions(e, name)),
        _ => false,
    }
}

/// Detect a self-recursive `let f = fn [n] -> BODY in (f K)` and evaluate it on the net by
/// REF unrolling. Returns `Ok(None)` when the source is not of that shape (the caller then falls
/// back to the non-recursive lowering).
pub fn run_rec(src: &str) -> Result<Option<(u128, usize)>, String> {
    use crate::latte::Ast;
    let ast = crate::latte::parse(src)?;
    let (name, gate, body_in) = match &ast {
        Ast::Let(n, v, b) => (n, v.as_ref(), b.as_ref()),
        _ => return Ok(None),
    };
    let (params, gbody) = match gate {
        Ast::Gate(ps, b) => (ps, b.as_ref()),
        _ => return Ok(None),
    };
    if params.len() != 1 {
        return Ok(None);
    }
    let (callee, args) = match body_in {
        Ast::Call(c, a) => (c, a),
        _ => return Ok(None),
    };
    if callee != name || args.len() != 1 || !mentions(gbody, name) {
        return Ok(None);
    }
    let body_r = to_r(gbody, &params[0], name)?;
    let arg_expr = latte_to_expr(&args[0])?;
    let arg_val = fold(&arg_expr).ok_or("net recursion: the initial argument must be constant")?;

    let mut net = Net::new();
    net.push_def(body_r); // defs[0] = the function body; Rec → Ref(0)
    let argp = build_num(&mut net, arg_val);
    let rf = net.agent(Sym::Ref(0));
    net.wire((rf, 1), argp);
    let out = net.free();
    net.wire((rf, 0), (out, 0));
    let steps = net.normalize_forcing();
    Ok(Some((decode_num(&net, out), steps)))
}

// ============================================================================
// The GENERAL net compiler. Where `run_rec` handles the classic single-parameter
// self-recursive function, this lowers the full numeric fragment of Latte to the
// net: `let`, user functions of any arity, multi-binding `loop`s, `==`, `sub`,
// dynamic `div`/`mod`, and a LAZY dynamic `if` for arbitrary (non-constant)
// conditions. The enabling idea is γ-cells as data: a function's arguments are
// packed into a right-nested γ-pair chain, parameters project out of δ-copies of
// the pack with Head/Tail, `loop` lowers to a recursive definition over its
// bindings, and `div`/`mod` lower to generated recursive definitions built from
// the net's native Sub/Lt agents. `if` hoists both branches into Ref closures
// (interaction-net boxes): the selector expands only the taken branch and the
// other is collected by an eraser without ever being built — so a recursive call
// in the untaken branch costs nothing, and conditions may be arbitrary subnets.
// ============================================================================

struct GEnv {
    defs: Vec<R>,                       // collected definitions (index = the Ref index)
    funcs: Vec<(String, usize, usize)>, // (name, def index, arity)
    div_idx: Option<usize>,             // the generated division definition, shared
    mod_idx: Option<usize>,             // the generated modulo definition, shared
    native: bool,                       // native ALU agents vs generated Peano recursions
}

struct GScope {
    params: Vec<String>,    // the current definition's parameters (a packed γ-chain)
    lets: Vec<(String, R)>, // let-bound values, inlined (rebuilt) per use
    loop_idx: Option<usize>, // the innermost loop's definition index, for `again`
}

/// Constant-fold an R tree (used to take statically-known `if` branches and to
/// keep constant arithmetic out of the net).
fn rfold(r: &R) -> Option<u128> {
    match r {
        R::Num(n) => Some(*n),
        R::Add(a, b) => rfold(a)?.checked_add(rfold(b)?),
        R::Mul(a, b) => rfold(a)?.checked_mul(rfold(b)?),
        R::Sub(a, b) => Some(rfold(a)?.saturating_sub(rfold(b)?)),
        R::Div(a, b) => {
            let bv = rfold(b)?;
            if bv == 0 { None } else { Some(rfold(a)? / bv) }
        }
        R::Mod(a, b) => {
            let bv = rfold(b)?;
            if bv == 0 { None } else { Some(rfold(a)? % bv) }
        }
        R::Lt(a, b) => Some(if rfold(a)? < rfold(b)? { 0 } else { 1 }),
        R::Eq(a, b) => Some(if rfold(a)? == rfold(b)? { 0 } else { 1 }),
        R::Dec(a) => Some(rfold(a)?.saturating_sub(1)),
        R::SubK(a, k) => Some(rfold(a)?.saturating_sub(*k)),
        R::If(c, t, e) => {
            if rfold(c)? == 0 { rfold(t) } else { rfold(e) }
        }
        _ => None,
    }
}

fn pn(i: usize, k: usize) -> R {
    if k == 1 { R::Param } else { R::ParamN(i, k) }
}

/// Pack argument values into a right-nested γ-pair chain (1 value packs to itself).
fn pack(mut args: Vec<R>) -> R {
    let mut acc = args.pop().expect("pack: at least one argument");
    while let Some(a) = args.pop() {
        acc = R::Pair(Box::new(a), Box::new(acc));
    }
    acc
}

fn ensure_div(env: &mut GEnv) -> usize {
    if let Some(i) = env.div_idx {
        return i;
    }
    let d = env.defs.len();
    env.defs.push(R::Num(0)); // reserve
    // f([x q b]) = if x < b then q else f([x∸b, q+1, b])
    env.defs[d] = R::If(
        Box::new(R::Lt(Box::new(pn(0, 3)), Box::new(pn(2, 3)))),
        Box::new(pn(1, 3)),
        Box::new(R::Call(
            d,
            Box::new(pack(vec![
                R::Sub(Box::new(pn(0, 3)), Box::new(pn(2, 3))),
                R::Add(Box::new(pn(1, 3)), Box::new(R::Num(1))),
                pn(2, 3),
            ])),
        )),
    );
    env.div_idx = Some(d);
    d
}

fn ensure_mod(env: &mut GEnv) -> usize {
    if let Some(i) = env.mod_idx {
        return i;
    }
    let m = env.defs.len();
    env.defs.push(R::Num(0)); // reserve
    // f([x b]) = if x < b then x else f([x∸b, b])
    env.defs[m] = R::If(
        Box::new(R::Lt(Box::new(pn(0, 2)), Box::new(pn(1, 2)))),
        Box::new(pn(0, 2)),
        Box::new(R::Call(
            m,
            Box::new(pack(vec![R::Sub(Box::new(pn(0, 2)), Box::new(pn(1, 2))), pn(1, 2)])),
        )),
    );
    env.mod_idx = Some(m);
    m
}

fn glower(a: &crate::latte::Ast, env: &mut GEnv, scope: &mut GScope) -> Result<R, String> {
    use crate::latte::Ast;
    let k = scope.params.len();
    match a {
        Ast::Lit(n) => Ok(R::Num(*n)),
        // tags and short strings are cords — atoms — and an atom that fits the
        // net's native word is just a number to it: `case` chains and tag
        // comparisons work unchanged
        Ast::Tag(t) | Ast::Text(t) => {
            let a = crate::knot::cord(t);
            match a.as_atom().and_then(|x| x.to_u128()) {
                Some(n) => Ok(R::Num(n)),
                None => Err("net: cord too long for the net's native word".into()),
            }
        }
        Ast::Nil => Ok(R::Num(0)),
        Ast::Var(x) => {
            if let Some((_, r)) = scope.lets.iter().rev().find(|(n, _)| n == x) {
                return Ok(r.clone());
            }
            if let Some(i) = scope.params.iter().position(|p| p == x) {
                return Ok(pn(i, k));
            }
            Err(format!(
                "net: free variable '{}' (a loop/function body on the net may use only its own bindings and constants)",
                x
            ))
        }
        Ast::Inc(e) => Ok(R::Add(Box::new(glower(e, env, scope)?), Box::new(R::Num(1)))),
        // γ-pair data on the net: brackets build Pair chains (autocons to the
        // right, as in the language), and head/tail lower to the Fst/Snd
        // projection agents the reducer already implements — so
        // `latte net "head [7 9]"` runs entirely as interactions.
        Ast::Tuple(xs) => {
            if xs.is_empty() {
                return Err("net: empty brackets have no value".into());
            }
            let mut it = xs.iter().rev();
            let mut acc = glower(it.next().unwrap(), env, scope)?;
            for x in it {
                acc = R::Pair(Box::new(glower(x, env, scope)?), Box::new(acc));
            }
            Ok(acc)
        }
        Ast::Head(e) => Ok(R::Fst(Box::new(glower(e, env, scope)?))),
        Ast::Tail(e) => Ok(R::Snd(Box::new(glower(e, env, scope)?))),
        Ast::Fast(_, b) => glower(b, env, scope),
        Ast::Eq(x, y) => Ok(R::Eq(Box::new(glower(x, env, scope)?), Box::new(glower(y, env, scope)?))),
        Ast::If(c, t, e) => {
            let rc = glower(c, env, scope)?;
            match rfold(&rc) {
                Some(0) => glower(t, env, scope),
                Some(_) => glower(e, env, scope),
                None => Ok(R::If(
                    Box::new(rc),
                    Box::new(glower(t, env, scope)?),
                    Box::new(glower(e, env, scope)?),
                )),
            }
        }
        Ast::Let(name, val, body) => match val.as_ref() {
            Ast::Gate(params, gbody) => {
                if params.is_empty() {
                    return Err("net: a function needs at least one parameter".into());
                }
                let idx = env.defs.len();
                env.defs.push(R::Num(0)); // reserve, so recursive calls resolve
                env.funcs.push((name.clone(), idx, params.len()));
                let mut inner = GScope { params: params.clone(), lets: Vec::new(), loop_idx: None };
                let b = glower(gbody, env, &mut inner)?;
                env.defs[idx] = b;
                glower(body, env, scope)
            }
            _ => {
                let v = glower(val, env, scope)?;
                scope.lets.push((name.clone(), v));
                let r = glower(body, env, scope);
                scope.lets.pop();
                r
            }
        },
        Ast::Again(args) => {
            let idx = scope
                .loop_idx
                .ok_or("net: `again` is only allowed inside a `loop`")?;
            let mut vs = Vec::with_capacity(args.len());
            for a in args {
                vs.push(glower(a, env, scope)?);
            }
            if vs.len() != k {
                return Err("net: `again` arity does not match the loop bindings".into());
            }
            Ok(R::Call(idx, Box::new(pack(vs))))
        }
        Ast::Loop(binds, body) => {
            if binds.is_empty() {
                return Err("net: a loop needs at least one binding".into());
            }
            let mut inits = Vec::with_capacity(binds.len());
            for (_, v) in binds {
                inits.push(glower(v, env, scope)?);
            }
            let idx = env.defs.len();
            env.defs.push(R::Num(0)); // reserve
            let names: Vec<String> = binds.iter().map(|(n, _)| n.clone()).collect();
            let mut inner = GScope { params: names, lets: Vec::new(), loop_idx: Some(idx) };
            let b = glower(body, env, &mut inner)?;
            env.defs[idx] = b;
            Ok(R::Call(idx, Box::new(pack(inits))))
        }
        Ast::Call(name, args) => {
            let bin = |env: &mut GEnv, scope: &mut GScope, f: fn(Box<R>, Box<R>) -> R| -> Result<R, String> {
                if args.len() != 2 {
                    return Err(format!("net: '{}' expects 2 arguments", name));
                }
                Ok(f(
                    Box::new(glower(&args[0], env, scope)?),
                    Box::new(glower(&args[1], env, scope)?),
                ))
            };
            match name.as_str() {
                "add" => bin(env, scope, R::Add),
                "mul" => bin(env, scope, R::Mul),
                "lt" => bin(env, scope, R::Lt),
                "sub" => bin(env, scope, R::Sub),
                "dec" => {
                    if args.len() != 1 {
                        return Err("net: 'dec' expects 1 argument".into());
                    }
                    Ok(R::Dec(Box::new(glower(&args[0], env, scope)?)))
                }
                "div" | "mod" => {
                    if args.len() != 2 {
                        return Err(format!("net: '{}' expects 2 arguments", name));
                    }
                    let x = glower(&args[0], env, scope)?;
                    let b = glower(&args[1], env, scope)?;
                    if let (Some(xv), Some(bv)) = (rfold(&x), rfold(&b)) {
                        // both constant: fold (and catch division by zero statically)
                        if bv == 0 {
                            return Err(format!("net: '{}' by zero", name));
                        }
                        return Ok(R::Num(if name == "div" { xv / bv } else { xv % bv }));
                    }
                    if rfold(&b) == Some(0) {
                        return Err(format!("net: '{}' by zero", name));
                    }
                    if env.native {
                        // native mode: division is ONE interaction on the NOp agents
                        return Ok(if name == "div" {
                            R::Div(Box::new(x), Box::new(b))
                        } else {
                            R::Mod(Box::new(x), Box::new(b))
                        });
                    }
                    if name == "div" {
                        let d = ensure_div(env);
                        Ok(R::Call(d, Box::new(pack(vec![x, R::Num(0), b]))))
                    } else {
                        let m = ensure_mod(env);
                        Ok(R::Call(m, Box::new(pack(vec![x, b]))))
                    }
                }
                other => {
                    let func = env.funcs.iter().rev().find(|(n, _, _)| n == other).cloned();
                    match func {
                        Some((_, idx, arity)) => {
                            if args.len() != arity {
                                return Err(format!("net: '{}' expects {} argument(s)", other, arity));
                            }
                            let mut vs = Vec::with_capacity(args.len());
                            for a in args {
                                vs.push(glower(a, env, scope)?);
                            }
                            Ok(R::Call(idx, Box::new(pack(vs))))
                        }
                        None => Err(format!("net: unsupported operation '{}'", other)),
                    }
                }
            }
        }
        other => Err(format!(
            "net: unsupported construct {:?} (the net engine computes over naturals)",
            std::mem::discriminant(other)
        )),
    }
}

/// Compile an arbitrary (numeric-fragment) Latte expression to the net with the
/// general lowerer and reduce it. Lazy `if`, multi-binding loops, user functions
/// of any arity, and dynamic `sub`/`==`/`div`/`mod` all run as net interactions.
pub fn run_general(src: &str) -> Result<(u128, usize), String> {
    run_general_mode(src, true)
}

/// The general compiler with an explicit number representation: `native = true`
/// uses Lit agents + the one-interaction ALU (the HVM2 idea); `false` uses pure
/// Peano chains and generated recursive `div`/`mod` (the pedagogical mode).
// ---------------------------------------------------------------------------
// NET PRE-NORMALIZATION: keep the net compiler abreast of the general one.
//
// glower's world is first-order-plus-defs: a let-bound gate becomes a net
// function (a Ref), and calls apply it. Two general-compiler constructs fall
// outside that shape and are normalized away here, at the AST level, before
// glowering:
//
//  · `case` lowers to an if/== chain (tags are atoms; short atoms are native
//    net numbers), so dispatch costs what comparisons cost.
//  · calls to non-recursive let-bound gates whose bodies contain a NESTED gate
//    (higher-order results — currying, compose) are β-REDUCED at compile time
//    with capture-avoiding substitution. What remains after inlining is first-
//    order and glower's def mechanism handles it — including recursion, which
//    is deliberately NOT inlined (Ref unrolling owns it).
// ---------------------------------------------------------------------------

fn subst(a: &crate::latte::Ast, name: &str, v: &crate::latte::Ast, ctr: &mut usize) -> crate::latte::Ast {
    use crate::latte::Ast as A;
    match a {
        A::Var(n) if n == name => v.clone(),
        A::Var(_) | A::Lit(_) | A::Nil | A::Tag(_) | A::Text(_) => a.clone(),
        A::Inc(e) => A::Inc(Box::new(subst(e, name, v, ctr))),
        A::Head(e) => A::Head(Box::new(subst(e, name, v, ctr))),
        A::Tail(e) => A::Tail(Box::new(subst(e, name, v, ctr))),
        A::IsCell(e) => A::IsCell(Box::new(subst(e, name, v, ctr))),
        A::Fast(n, e) => A::Fast(n.clone(), Box::new(subst(e, name, v, ctr))),
        A::Eq(x, y) => A::Eq(Box::new(subst(x, name, v, ctr)), Box::new(subst(y, name, v, ctr))),
        A::If(c, t, e) => A::If(
            Box::new(subst(c, name, v, ctr)),
            Box::new(subst(t, name, v, ctr)),
            Box::new(subst(e, name, v, ctr)),
        ),
        A::Tuple(es) => A::Tuple(es.iter().map(|e| subst(e, name, v, ctr)).collect()),
        A::Again(es) => A::Again(es.iter().map(|e| subst(e, name, v, ctr)).collect()),
        A::Call(f, es) => {
            let es2: Vec<A> = es.iter().map(|e| subst(e, name, v, ctr)).collect();
            if f == name {
                // the substituted value is CALLED by this name: rebind it under a
                // fresh let so the ordinary let-bound-gate machinery (defs, or
                // another round of inlining for higher-order bodies) applies
                *ctr += 1;
                let fresh = format!("__inl{}", ctr);
                A::Let(fresh.clone(), Box::new(v.clone()), Box::new(A::Call(fresh, es2)))
            } else {
                A::Call(f.clone(), es2)
            }
        }
        A::Case(sc, arms) => A::Case(
            Box::new(subst(sc, name, v, ctr)),
            arms.iter().map(|(p, b)| (p.clone(), subst(b, name, v, ctr))).collect(),
        ),
        A::Let(n, val, b) => {
            let val2 = subst(val, name, v, ctr);
            if n == name {
                A::Let(n.clone(), Box::new(val2), b.clone()) // shadowed below
            } else {
                A::Let(n.clone(), Box::new(val2), Box::new(subst(b, name, v, ctr)))
            }
        }
        A::Gate(ps, b) => {
            if ps.iter().any(|p| p == name) {
                a.clone() // shadowed by a parameter
            } else {
                // capture avoidance: rename any binder that appears free in v
                let mut ps2 = ps.clone();
                let mut b2 = (**b).clone();
                for i in 0..ps2.len() {
                    if mentions(v, &ps2[i]) {
                        *ctr += 1;
                        let fresh = format!("{}__r{}", ps2[i], ctr);
                        b2 = subst(&b2, &ps2[i].clone(), &A::Var(fresh.clone()), ctr);
                        ps2[i] = fresh;
                    }
                }
                A::Gate(ps2, Box::new(subst(&b2, name, v, ctr)))
            }
        }
        A::Loop(binds, b) => A::Loop(
            binds.iter().map(|(n, e)| (n.clone(), subst(e, name, v, ctr))).collect(),
            Box::new(if binds.iter().any(|(n, _)| n == name) { (**b).clone() } else { subst(b, name, v, ctr) }),
        ),
    }
}

fn contains_gate(a: &crate::latte::Ast) -> bool {
    use crate::latte::Ast as A;
    match a {
        A::Gate(..) => true,
        A::Inc(e) | A::Head(e) | A::Tail(e) | A::IsCell(e) | A::Fast(_, e) => contains_gate(e),
        A::Eq(x, y) => contains_gate(x) || contains_gate(y),
        A::If(c, t, e) => contains_gate(c) || contains_gate(t) || contains_gate(e),
        A::Tuple(es) | A::Again(es) | A::Call(_, es) => es.iter().any(contains_gate),
        A::Case(sc, arms) => contains_gate(sc) || arms.iter().any(|(_, b)| contains_gate(b)),
        A::Let(_, v, b) => contains_gate(v) || contains_gate(b),
        A::Loop(binds, b) => binds.iter().any(|(_, e)| contains_gate(e)) || contains_gate(b),
        _ => false,
    }
}

fn net_prenorm(
    a: &crate::latte::Ast,
    gates: &mut Vec<(String, Vec<String>, crate::latte::Ast)>,
    ctr: &mut usize,
    depth: usize,
) -> crate::latte::Ast {
    use crate::latte::Ast as A;
    if depth > 64 {
        return a.clone(); // inlining fuel exhausted: glower reports what remains
    }
    match a {
        A::Case(sc, arms) => {
            // case -> if/== chain (right to left, default last)
            let sc2 = net_prenorm(sc, gates, ctr, depth);
            let mut acc = arms
                .iter()
                .find(|(p, _)| p.is_none())
                .map(|(_, d)| net_prenorm(d, gates, ctr, depth))
                .unwrap_or(A::Lit(0));
            for (pat, body) in arms.iter().rev() {
                if let Some(tag) = pat {
                    acc = A::If(
                        Box::new(A::Eq(Box::new(sc2.clone()), Box::new(A::Tag(tag.clone())))),
                        Box::new(net_prenorm(body, gates, ctr, depth)),
                        Box::new(acc),
                    );
                }
            }
            acc
        }
        A::Let(n, v, b) => {
            let v2 = net_prenorm(v, gates, ctr, depth);
            if let A::Gate(ps, gb) = &v2 {
                let recursive = mentions(gb, n) && !ps.iter().any(|p| p == n);
                if !recursive && contains_gate(gb) {
                    // a HIGHER-ORDER gate: record it for β-reduction at call sites
                    gates.push((n.clone(), ps.clone(), (**gb).clone()));
                    let b2 = net_prenorm(b, gates, ctr, depth);
                    gates.pop();
                    return b2; // fully inlined away
                }
            }
            A::Let(n.clone(), Box::new(v2), Box::new(net_prenorm(b, gates, ctr, depth)))
        }
        A::Call(f, args) => {
            let args2: Vec<A> = args.iter().map(|x| net_prenorm(x, gates, ctr, depth)).collect();
            if let Some((_, ps, gb)) = gates.iter().rev().find(|(n, _, _)| n == f).cloned() {
                if ps.len() == args2.len() {
                    let mut body = gb;
                    for (p, arg) in ps.iter().zip(args2.iter()) {
                        body = subst(&body, p, arg, ctr);
                    }
                    return net_prenorm(&body, gates, ctr, depth + 1);
                }
            }
            A::Call(f.clone(), args2)
        }
        A::If(c, t, e) => A::If(
            Box::new(net_prenorm(c, gates, ctr, depth)),
            Box::new(net_prenorm(t, gates, ctr, depth)),
            Box::new(net_prenorm(e, gates, ctr, depth)),
        ),
        A::Eq(x, y) => A::Eq(Box::new(net_prenorm(x, gates, ctr, depth)), Box::new(net_prenorm(y, gates, ctr, depth))),
        A::Inc(e) => A::Inc(Box::new(net_prenorm(e, gates, ctr, depth))),
        A::Head(e) => A::Head(Box::new(net_prenorm(e, gates, ctr, depth))),
        A::Tail(e) => A::Tail(Box::new(net_prenorm(e, gates, ctr, depth))),
        A::IsCell(e) => A::IsCell(Box::new(net_prenorm(e, gates, ctr, depth))),
        A::Fast(n, e) => A::Fast(n.clone(), Box::new(net_prenorm(e, gates, ctr, depth))),
        A::Tuple(es) => A::Tuple(es.iter().map(|e| net_prenorm(e, gates, ctr, depth)).collect()),
        A::Again(es) => A::Again(es.iter().map(|e| net_prenorm(e, gates, ctr, depth)).collect()),
        A::Gate(ps, b) => A::Gate(ps.clone(), Box::new(net_prenorm(b, gates, ctr, depth))),
        A::Loop(binds, b) => A::Loop(
            binds.iter().map(|(n, e)| (n.clone(), net_prenorm(e, gates, ctr, depth))).collect(),
            Box::new(net_prenorm(b, gates, ctr, depth)),
        ),
        _ => a.clone(),
    }
}

pub fn run_general_mode(src: &str, native: bool) -> Result<(u128, usize), String> {
    let parsed = crate::latte::parse(src)?;
    let mut gates = Vec::new();
    let mut ctr = 0usize;
    let ast = net_prenorm(&parsed, &mut gates, &mut ctr, 0);
    let mut env = GEnv { defs: Vec::new(), funcs: Vec::new(), div_idx: None, mod_idx: None, native };
    let mut scope = GScope { params: Vec::new(), lets: Vec::new(), loop_idx: None };
    let main = glower(&ast, &mut env, &mut scope)?;
    let mut net = Net::new();
    net.native = native;
    net.defs = env.defs;
    let mut supply: Vec<Port> = Vec::new();
    let rp = net.build(&main, &mut supply);
    let out = net.free();
    net.wire(rp, (out, 0));
    let steps = net.normalize_forcing();
    let v = decode_num(&net, out);
    if v == u128::MAX {
        return Err("net: did not reduce to a number within the step budget".into());
    }
    Ok((v, steps))
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
mod parity_tests {
    // the net's parity with the general compiler: every construct here also runs
    // on the interpreter, and run_str cross-checks the two per invocation
    fn v(src: &str) -> u128 {
        super::run_str(src).expect(src).0
    }

    #[test]
    fn net_case_lowers_to_comparisons() {
        assert_eq!(v("case %go of %go -> 7 ; %stop -> 2 ; _ -> 0 end"), 7);
        assert_eq!(v("case %huh of %go -> 7 ; _ -> 99 end"), 99);
    }

    #[test]
    fn net_handles_higher_order_gates_by_inlining() {
        assert_eq!(v("let addk = fn [k] -> fn [x] -> (add x k) in ((addk 5) 10)"), 15);
        assert_eq!(v("let compose = fn [f g] -> fn [x] -> (f (g x)) in ((compose (fn [x] -> (mul x x)) (fn [x] -> (add x 1))) 4)"), 25);
        assert_eq!(v("let flip = fn [f] -> fn [a b] -> (f b a) in ((flip (fn [a b] -> (sub a b))) 3 10)"), 7);
    }

    #[test]
    fn net_recursion_and_loops_survive_prenormalization() {
        assert_eq!(v("let f = fn [n] -> if (lt n 2) then 1 else (mul n (f (dec n))) in (f 5)"), 120);
        assert_eq!(v("loop with [i = 3, a = 1] : if (i == 0) then a else again((dec i), (mul a 2)) end"), 8);
    }
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

    #[test]
    fn parallel_reduction_matches_sequential() {
        // uniform confluence: the parallel batch reducer must reach the same normal
        // form as the sequential engine, on nets with real concurrent active pairs.
        for src in [
            "let f = fn [a b] -> (add (mul a a) (mul b b)) in (f 31 17)",
            "let fib = fn [n] -> if (lt n 2) then n else (add (fib (sub n 1)) (fib (sub n 2))) in (fib 14)",
            "loop with [a = 0, b = 1, i = 16] : if (i == 0) then a else again(b, (add a b), (dec i)) end",
            "let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b)) in (gcd 1071 462)",
        ] {
            let seq = super::run_str(src).expect(src).0;
            // build the same program and reduce it with the parallel engine
            let ast = crate::latte::parse(src).unwrap();
            let mut env = super::GEnv { defs: Vec::new(), funcs: Vec::new(), div_idx: None, mod_idx: None, native: true };
            let mut scope = super::GScope { params: Vec::new(), lets: Vec::new(), loop_idx: None };
            let main = super::glower(&ast, &mut env, &mut scope).unwrap();
            let mut net = super::Net::new();
            net.native = true;
            net.defs = env.defs;
            let mut supply = Vec::new();
            let rp = net.build(&main, &mut supply);
            let out = net.free();
            net.wire(rp, (out, 0));
            // interleave parallel batches with the forcing loop's Ref expansion
            let mut budget = 0;
            loop {
                net.normalize_parallel(4);
                let before = net.steps;
                net.normalize_forcing();
                budget += 1;
                if net.steps == before || budget > 64 {
                    break;
                }
            }
            let v = super::decode_num(&net, out);
            assert_eq!(v, seq, "parallel and sequential engines disagree on {}", src);
        }
    }

    #[test]
    fn native_numbers_collapse_step_counts() {
        // the HVM2 idea: numbers as atomic agents, every arithmetic op ONE interaction.
        // gcd(1071,462) is 72,758 interactions in Peano mode; native mode needs ~100.
        let (v, steps) =
            super::run_str("let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b)) in (gcd 1071 462)").unwrap();
        assert_eq!(v, 21);
        assert!(steps < 1_000, "native gcd should be ~100 steps, got {}", steps);
        // a multiplication that would cost ~10^10 Peano interactions is 2 steps
        let (v2, steps2) = super::run_str("(mul 99999 99999)").unwrap();
        assert_eq!(v2, 9_999_800_001);
        assert!(steps2 <= 4, "native mul is O(1) interactions, got {}", steps2);
        // and the Peano mode still works (the pedagogical path)
        let (v3, _) = super::run_str_peano("(mul 12 11)").unwrap();
        assert_eq!(v3, 132);
    }

    #[test]
    fn net_sub_and_eq_agents() {
        // the new Peano-lockstep agents, dynamic (not constant-folded) via a function call
        assert_eq!(super::run_str("let f = fn [n] -> (sub n 4) in (f 10)").unwrap().0, 6);
        assert_eq!(super::run_str("let f = fn [n] -> (sub n 40) in (f 10)").unwrap().0, 0); // monus
        assert_eq!(super::run_str("let f = fn [n] -> if (n == 7) then 1 else 0 in (f 7)").unwrap().0, 1);
        assert_eq!(super::run_str("let f = fn [n] -> if (n == 7) then 1 else 0 in (f 9)").unwrap().0, 0);
    }

    #[test]
    fn net_dynamic_div_mod() {
        // div/mod with a non-constant dividend lower to generated recursive definitions
        assert_eq!(super::run_str("let f = fn [n] -> (add n 2) in (div (f 15) 5)").unwrap().0, 3);
        assert_eq!(super::run_str("let f = fn [n] -> (add n 2) in (mod (f 15) 5)").unwrap().0, 2);
    }

    #[test]
    fn net_multi_param_and_loops() {
        // γ-pairs as net data: functions of any arity and multi-binding loops
        assert_eq!(super::run_str("let f = fn [a b] -> (add (mul a 10) b) in (f 4 2)").unwrap().0, 42);
        assert_eq!(
            super::run_str("let gcd = fn [a b] -> if (b == 0) then a else (gcd b (mod a b)) in (gcd 48 18)").unwrap().0,
            6
        );
        assert_eq!(
            super::run_str("loop with [a = 0, b = 1, i = 10] : if (i == 0) then a else again(b, (add a b), (dec i)) end").unwrap().0,
            55
        );
    }

    #[test]
    fn net_lazy_dynamic_if_skips_untaken_branch() {
        // the condition is computed BY THE NET (a recursive call), and the untaken branch
        // holds a computation that would cost ~10^10 interactions if built strictly; the
        // boxed (Ref-closure) `if` must erase it unexpanded.
        let (v, steps) = super::run_str(
            "let g = fn [n] -> if (lt n 1) then 0 else (add 1 (g (sub n 1))) in if (lt (g 3) 100) then 42 else (mul 99999 99999)",
        )
        .unwrap();
        assert_eq!(v, 42);
        assert!(steps < 5_000, "untaken branch must not be reduced (got {} steps)", steps);
    }

    #[test]
    fn general_compiler_matches_interpreter() {
        for src in [
            "(sub (mul 7 8) (div 100 9))",
            "let f = fn [x y] -> (sub (mul x x) y) in (f 9 8)",
            "loop with [q = 0, r = 23] : if (lt r 7) then q else again((add q 1), (sub r 7)) end",
        ] {
            let (v, _) = super::run_str(src).expect(src);
            let loom = crate::latte::run_with_libs(src, &["std"])
                .ok()
                .and_then(|n| n.as_atom().and_then(|a| a.to_u128()))
                .unwrap();
            assert_eq!(v, loom, "net and interpreter disagree on {}", src);
        }
        // let-bound recursion is the net's own capability (the interpreter has no fixpoint
        // for let-bound gates), so its value is checked directly:
        assert_eq!(
            super::run_str("let fib = fn [n] -> if (lt n 2) then n else (add (fib (sub n 1)) (fib (sub n 2))) in (fib 10)").unwrap().0,
            55
        );
    }

    #[test]
    fn net_extended_fragment_matches_loom() {
        // let, +(), ==, let-bound user functions (inlined), and loop/again (bounded unrolling)
        // all lower into the add/mul/lt/if net and must agree with the interpreter.
        for src in [
            "let x = (add 2 3) in (mul x x)",                              // let
            "+(+(40))",                                                    // +()
            "if (5 == 5) then 7 else 9",                                   // ==
            "let sq = fn [x] -> (mul x x) in (add (sq 3) (sq 4))",         // user function (inlined)
            "loop with [acc = 0, i = 5] : if (i == 0) then acc else again((add acc i), (dec i)) end", // loop/again
            "let dbl = fn [x] -> (add x x) in (dbl (dbl 6))",              // nested inlining
        ] {
            let (v, _) = super::run_str(src).expect(src);
            let loom = crate::latte::run_with_libs(src, &["std"]).unwrap();
            let want = loom.as_atom().and_then(|a| a.to_u128()).unwrap();
            assert_eq!(v, want, "net vs loom for {}", src);
        }
        // sum 1..5 = 15
        assert_eq!(super::run_str("loop with [acc = 0, i = 5] : if (i == 0) then acc else again((add acc i), (dec i)) end").unwrap().0, 15);
    }

    #[test]
    fn net_ref_recursion_matches_loom() {
        // Genuine net-level fixpoints: a self-recursive function is unrolled by the net reducer
        // (HVM-style REF nodes). Latte's `let` is not recursive, so the reference value is the
        // SAME function written as a `loop` and run on the Loom interpreter — a cross-engine,
        // cross-formulation check that the net's recursion is correct.
        let cases = [
            // (net: self-recursive let,                                   interpreter: loop)
            ("let fac = fn [n] -> if (lt n 1) then 1 else (mul n (fac (dec n))) in (fac 6)",
             "loop with [a = 1, i = 6] : if (lt i 1) then a else again((mul a i), (dec i)) end"),
            ("let sum = fn [n] -> if (lt n 1) then 0 else (add n (sum (dec n))) in (sum 10)",
             "loop with [a = 0, i = 10] : if (lt i 1) then a else again((add a i), (dec i)) end"),
            ("let tri = fn [n] -> if (lt n 1) then 0 else (add n (tri (dec n))) in (tri 50)",
             "loop with [a = 0, i = 50] : if (lt i 1) then a else again((add a i), (dec i)) end"),
            ("let fib = fn [n] -> if (lt n 2) then n else (add (fib (sub n 1)) (fib (sub n 2))) in (fib 10)",
             "loop with [a = 0, b = 1, i = 10] : if (lt i 1) then a else again(b, (add a b), (dec i)) end"),
        ];
        for (net_src, loop_src) in cases {
            let (v, _) = super::run_str(net_src).expect(net_src);
            let want = crate::latte::run_with_libs(loop_src, &["std"]).unwrap()
                .as_atom().and_then(|a| a.to_u128()).unwrap();
            assert_eq!(v, want, "net recursion vs loom-loop for {}", net_src);
        }
        assert_eq!(super::run_str("let fac = fn [n] -> if (lt n 1) then 1 else (mul n (fac (dec n))) in (fac 6)").unwrap().0, 720);
        assert_eq!(super::run_str("let tri = fn [n] -> if (lt n 1) then 0 else (add n (tri (dec n))) in (tri 100)").unwrap().0, 5050);
    }

    #[test]
    fn net_recursion_is_detected_only_when_recursive() {
        // a non-recursive let still goes through the ordinary (folding) lowering
        assert!(super::run_rec("let x = (add 2 3) in (mul x x)").unwrap().is_none());
        // a genuinely recursive let is handled by the REF engine
        assert!(super::run_rec("let f = fn [n] -> if (lt n 1) then 0 else (f (dec n)) in (f 3)").unwrap().is_some());
    }

    #[test]
    fn net_if_is_lazy() {
        // The untaken branch must never be built into the net. `mul 6 7` would be 42; if the
        // `then` branch were built we'd see Mul/Succ agents and far more steps. With a lazy
        // `if`, only `(add 1 1)` is compiled.
        let e = super::latte_to_expr(&crate::latte::parse("if (lt 7 4) then (mul 6 7) else (add 1 1)").unwrap()).unwrap();
        // the lowered expression is exactly the taken branch — no `if`, no `mul`
        assert!(matches!(e, super::Expr::Add(_, _)), "lazy if should lower to just the taken branch");
        let (v, _) = super::eval_net(&e);
        assert_eq!(v, 2);
        // and it agrees with the interpreter
        let loom = crate::latte::run_with_libs("if (lt 7 4) then (mul 6 7) else (add 1 1)", &["std"]).unwrap();
        assert_eq!(v, loom.as_atom().and_then(|a| a.to_u128()).unwrap());
    }
}

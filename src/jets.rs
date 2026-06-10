//! Native jets for the standard library's hot arithmetic. Latte's `std` builds all
//! arithmetic from the single successor primitive, so `add`/`sub`/`mul`/`div`/`mod` are
//! unary in the operands' magnitude — correct but slow. These jets give a native
//! fast-path for the `%add %sub %mul %div %mod` hints; in audit mode the interpreter
//! checks each against the pure reduction, so acceleration can never change a result.

use crate::knot::{num, N};
use crate::loom::{self, slot, Crash, Eval};
use crate::atom::Atom;

fn operands(subject: &N) -> Result<(u128, u128), Crash> {
    let s = slot(&Atom::from_u128(3), subject)?; // sample = [a b] at axis 3
    let (a, b) = s.as_cell().ok_or_else(|| Crash::Bottom("arith jet: sample not [a b]".into()))?;
    let a = a.as_atom().and_then(|x| x.to_u128()).ok_or_else(|| Crash::Bottom("arith jet: a not a small atom".into()))?;
    let b = b.as_atom().and_then(|x| x.to_u128()).ok_or_else(|| Crash::Bottom("arith jet: b not a small atom".into()))?;
    Ok((a, b))
}

fn jet_add(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    a.checked_add(b).map(num).ok_or_else(|| Crash::Bottom("add jet overflow".into()))
}
fn jet_sub(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    // pure `sub` diverges if b > a; the jet reports a clean domain error instead
    a.checked_sub(b).map(num).ok_or_else(|| Crash::Bottom("sub jet: underflow".into()))
}
fn jet_mul(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    a.checked_mul(b).map(num).ok_or_else(|| Crash::Bottom("mul jet overflow".into()))
}
fn jet_div(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    if b == 0 {
        return Err(Crash::Bottom("div jet: divide by zero".into()));
    }
    Ok(num(a / b))
}
fn jet_mod(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    if b == 0 {
        return Err(Crash::Bottom("mod jet: divide by zero".into()));
    }
    Ok(num(a % b))
}

fn jet_lt(s: &N) -> Eval {
    let (a, b) = operands(s)?;
    Ok(num(if a < b { 0 } else { 1 })) // loobean: 0 = true
}
fn jet_dec(s: &N) -> Eval {
    let n = slot(&Atom::from_u128(3), s)?;
    let v = n.as_atom().and_then(|x| x.to_u128()).ok_or_else(|| Crash::Bottom("dec jet: not a small atom".into()))?;
    if v == 0 {
        return Err(Crash::Bottom("dec jet: dec(0)".into()));
    }
    Ok(num(v - 1))
}

// ============================================================================
// SIGNED FIXED-POINT VECTOR JETS (num.lat). The numeric libraries (nn, ta, fin,
// trace) spend nearly all their time in `nadd/nsub/nmul/ndiv` over [sign mag]
// pairs and in `ndot`/`nsqrt`; native fast-paths here speed them by orders of
// magnitude. Each jet REPLICATES the Latte definition exactly (same rounding,
// same -0 normalization), and the audit mode verifies that on every call.
// ============================================================================

/// Decode a num.lat signed number [sign mag] (mag is fixed-point ×1000).
fn signed_of(n: &N) -> Result<(bool, u128), Crash> {
    let (sg, mg) = n.as_cell().ok_or_else(|| Crash::Bottom("signed jet: not a [sign mag] cell".into()))?;
    let sg = sg.as_atom().and_then(|x| x.to_u128()).ok_or_else(|| Crash::Bottom("signed jet: sign not an atom".into()))?;
    let mg = mg.as_atom().and_then(|x| x.to_u128()).ok_or_else(|| Crash::Bottom("signed jet: mag not an atom".into()))?;
    Ok((sg != 0, mg))
}
/// Encode with num.lat's `nnorm`: a zero magnitude is always positive.
fn signed_to(neg: bool, mag: u128) -> N {
    crate::knot::cell(num(if neg && mag != 0 { 1 } else { 0 }), num(mag))
}
fn signed_operands(s: &N) -> Result<((bool, u128), (bool, u128)), Crash> {
    let smp = slot(&Atom::from_u128(3), s)?;
    let (a, b) = smp.as_cell().ok_or_else(|| Crash::Bottom("signed jet: sample not [a b]".into()))?;
    Ok((signed_of(&a)?, signed_of(&b)?))
}

// the four scalar ops, exactly as lib/num.lat defines them
fn nadd_raw(a: (bool, u128), b: (bool, u128)) -> (bool, u128) {
    let ((sa, ma), (sb, mb)) = (a, b);
    if sa == sb {
        (sa, ma + mb)
    } else if ma >= mb {
        (sa, ma - mb)
    } else {
        (sb, mb - ma)
    }
}
fn nmul_raw(a: (bool, u128), b: (bool, u128)) -> (bool, u128) {
    (a.0 != b.0, a.1 * b.1 / 1000)
}
fn ndiv_raw(a: (bool, u128), b: (bool, u128)) -> Result<(bool, u128), Crash> {
    if b.1 == 0 {
        return Err(Crash::Bottom("ndiv jet: divide by zero".into()));
    }
    Ok((a.0 != b.0, a.1 * 1000 / b.1))
}
fn jet_nadd(s: &N) -> Eval {
    let (a, b) = signed_operands(s)?;
    let r = nadd_raw(a, b);
    Ok(signed_to(r.0, r.1))
}
fn jet_nsub(s: &N) -> Eval {
    let (a, b) = signed_operands(s)?;
    let r = nadd_raw(a, (!b.0, b.1));
    Ok(signed_to(r.0, r.1))
}
fn jet_nmul(s: &N) -> Eval {
    let (a, b) = signed_operands(s)?;
    let r = nmul_raw(a, b);
    Ok(signed_to(r.0, r.1))
}
fn jet_ndiv(s: &N) -> Eval {
    let (a, b) = signed_operands(s)?;
    let r = ndiv_raw(a, b)?;
    Ok(signed_to(r.0, r.1))
}
fn jet_nlt(s: &N) -> Eval {
    let (a, b) = signed_operands(s)?;
    // signed less-than -> loobean, exactly as num.lat (note: -0 vs +0 follows the
    // same branch structure because nnorm keeps magnitudes-0 positive upstream)
    let lt = if a.0 == b.0 {
        if !a.0 { a.1 < b.1 } else { b.1 < a.1 }
    } else {
        a.0 && !b.0
    };
    Ok(num(if lt { 0 } else { 1 }))
}

/// ndot: the fold of nadd(acc, nmul(u_i, v_i)) over two equal-length lists,
/// starting from [0 0] — exactly num.lat's loop, in native arithmetic.
fn jet_ndot(s: &N) -> Eval {
    let smp = slot(&Atom::from_u128(3), s)?;
    let (u, v) = smp.as_cell().ok_or_else(|| Crash::Bottom("ndot jet: sample not [u v]".into()))?;
    let (mut cu, mut cv) = (u, v);
    let mut acc = (false, 0u128);
    loop {
        let (uc, vc) = (cu.as_cell(), cv.as_cell());
        match uc {
            None => break, // the Latte loop stops when the FIRST list ends
            Some((uh, ut)) => {
                let (vh, vt) = match vc {
                    Some(p) => p,
                    None => return Err(Crash::Bottom("ndot jet: second list shorter".into())),
                };
                let a = signed_of(&uh)?;
                let b = signed_of(&vh)?;
                let prod = nmul_raw(a, b);
                // nmul ends in nnorm, so feed the normalized product into nadd
                let prod = (prod.0 && prod.1 != 0, prod.1);
                acc = nadd_raw(acc, prod);
                acc = (acc.0 && acc.1 != 0, acc.1); // nnorm after nadd
                cu = ut;
                cv = vt;
            }
        }
    }
    Ok(signed_to(acc.0, acc.1))
}

/// nsqrt: Newton's method, 24 iterations, replicating num.lat's exact fixed-point
/// rounding: r' = 0.5 * (r + x/r), with [0 500]*z = 500*z/1000 truncation.
fn jet_nsqrt(s: &N) -> Eval {
    let x = slot(&Atom::from_u128(3), s)?;
    let (sx, mx) = signed_of(&x)?;
    // nlt x [0 1]: true for any negative x, and for 0 <= x < 0.001
    if sx || mx < 1 {
        return Ok(signed_to(false, 0));
    }
    // r = nmax [0 1000] x
    let mut r = if mx < 1000 { 1000u128 } else { mx };
    for _ in 0..24 {
        let q = mx * 1000 / r; // ndiv x r (both positive)
        let sum = r + q; // nadd, same sign
        r = 500 * sum / 1000; // nmul [0 500] sum
    }
    Ok(signed_to(false, r))
}

/// Register the standard arithmetic jets. Idempotent.
pub fn register_std_jets() {
    loom::set_jets_enabled(true);
    loom::register_jet(b"add", jet_add);
    loom::register_jet(b"sub", jet_sub);
    loom::register_jet(b"mul", jet_mul);
    loom::register_jet(b"div", jet_div);
    loom::register_jet(b"mod", jet_mod);
    loom::register_jet(b"lt", jet_lt);
    loom::register_jet(b"dec", jet_dec);
    loom::register_jet(b"nadd", jet_nadd);
    loom::register_jet(b"nsub", jet_nsub);
    loom::register_jet(b"nmul", jet_nmul);
    loom::register_jet(b"ndiv", jet_ndiv);
    loom::register_jet(b"nlt", jet_nlt);
    loom::register_jet(b"ndot", jet_ndot);
    loom::register_jet(b"nsqrt", jet_nsqrt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latte::run_with_libs;

    #[test]
    fn jetted_arithmetic_is_correct() {
        register_std_jets();
        assert_eq!(run_with_libs("(mul 123 1000)", &["std"]).unwrap(), num(123000));
        assert_eq!(run_with_libs("(div 123456 1000)", &["std"]).unwrap(), num(123));
        assert_eq!(run_with_libs("(mod 123456 1000)", &["std"]).unwrap(), num(456));
        assert_eq!(run_with_libs("(sub 1000 1)", &["std"]).unwrap(), num(999));
        assert_eq!(run_with_libs("(add 999 1)", &["std"]).unwrap(), num(1000));
    }

    #[test]
    fn signed_vector_jets_match_pure_under_audit() {
        // the new num.lat jets must agree with the pure Latte definitions bit-for-bit
        register_std_jets();
        loom::set_jet_audit(true);
        for (src, want) in [
            ("(nadd [0 1500] [1 700])", "(nadd [0 1500] [1 700])"),
            ("(nsub [1 300] [1 900])", "(nsub [1 300] [1 900])"),
            ("(nmul [1 2500] [0 4000])", "(nmul [1 2500] [0 4000])"),
            ("(ndiv [0 10000] [0 3000])", "(ndiv [0 10000] [0 3000])"),
            ("(nlt [1 5] [0 0])", "(nlt [1 5] [0 0])"),
            ("(ndot [ [0 1000] [ [1 2000] 0 ] ] [ [0 3000] [ [0 500] 0 ] ])",
             "(ndot [ [0 1000] [ [1 2000] 0 ] ] [ [0 3000] [ [0 500] 0 ] ])"),
            ("(nsqrt [0 2000000])", "(nsqrt [0 2000000])"),
        ] {
            // run once with audit on: any jet/pure disagreement crashes the call
            let r = run_with_libs(src, &["std", "num"]);
            assert!(r.is_ok(), "{} failed under jet audit: {:?}", src, r.err());
            let _ = want;
        }
        loom::set_jet_audit(false);
    }

    #[test]
    fn jet_matches_pure_under_audit() {
        // with audit on, the jet result must equal the pure reduction or the run crashes
        register_std_jets();
        loom::set_jet_audit(true);
        let r = run_with_libs("(mul 21 2)", &["std"]).unwrap();
        loom::set_jet_audit(false);
        assert_eq!(r, num(42));
    }
}

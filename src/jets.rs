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
    fn jet_matches_pure_under_audit() {
        // with audit on, the jet result must equal the pure reduction or the run crashes
        register_std_jets();
        loom::set_jet_audit(true);
        let r = run_with_libs("(mul 21 2)", &["std"]).unwrap();
        loom::set_jet_audit(false);
        assert_eq!(r, num(42));
    }
}

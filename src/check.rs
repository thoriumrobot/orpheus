//! A static type checker for Latte — the compile-time companion to the runtime
//! mold/aura system. Where molds validate and coerce *values* on Loom, this pass
//! infers a structural *type* for an expression and flags shape errors before the
//! program ever runs.
//!
//! The type lattice is deliberately small and mirrors nouns:
//!   `@`        an atom
//!   `[T T]`    a cell (pair) of known parts
//!   `*`        a noun of unknown shape (the top type)
//!
//! The checker is *sound but conservative*: it reports an error only when an
//! operation is provably misapplied (taking the `head`/`tail` of something that must
//! be an atom, or incrementing something that must be a cell). Anything it cannot pin
//! down — arm calls, loops, closures — becomes `*`, which is compatible with every
//! operation, so a well-typed program is never rejected. This is the honest static
//! fragment over Loom's untyped substrate: it catches real bugs without pretending to
//! a totality it cannot have.

use crate::latte::Ast;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Atom,
    Cell(Box<Ty>, Box<Ty>),
    Noun, // top: unknown shape
}

impl Ty {
    pub fn show(&self) -> String {
        match self {
            Ty::Atom => "@".to_string(),
            Ty::Noun => "*".to_string(),
            Ty::Cell(a, b) => format!("[{} {}]", a.show(), b.show()),
        }
    }
    fn is_definitely_cell(&self) -> bool {
        matches!(self, Ty::Cell(_, _))
    }
}

/// Least upper bound of two branch types: equal shapes are preserved, conflicts widen
/// to `*`.
fn join(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Atom, Ty::Atom) => Ty::Atom,
        (Ty::Cell(a1, a2), Ty::Cell(b1, b2)) => Ty::Cell(Box::new(join(*a1, *b1)), Box::new(join(*a2, *b2))),
        _ => Ty::Noun,
    }
}

type Env = HashMap<String, Ty>;

/// Infer the structural type of an expression, or report the first shape error.
pub fn check(ast: &Ast) -> Result<Ty, String> {
    infer(ast, &Env::new())
}

fn infer(ast: &Ast, env: &Env) -> Result<Ty, String> {
    Ok(match ast {
        Ast::Lit(_) | Ast::Tag(_) | Ast::Text(_) | Ast::Nil => Ty::Atom,

        Ast::Var(name) => env.get(name).cloned().unwrap_or(Ty::Noun),

        Ast::Tuple(elems) => {
            if elems.is_empty() {
                return Ok(Ty::Atom);
            }
            let tys: Vec<Ty> = elems.iter().map(|e| infer(e, env)).collect::<Result<_, _>>()?;
            // right-nested cons: [a b c] = [a [b c]]
            let mut it = tys.into_iter().rev();
            let mut acc = it.next().unwrap();
            for t in it {
                acc = Ty::Cell(Box::new(t), Box::new(acc));
            }
            acc
        }

        Ast::Inc(e) => {
            let t = infer(e, env)?;
            if t.is_definitely_cell() {
                return Err(format!("type error: `+` expects an atom but got a cell {}", t.show()));
            }
            Ty::Atom
        }

        Ast::Head(e) => {
            let t = infer(e, env)?;
            match t {
                Ty::Cell(h, _) => *h,
                Ty::Atom => return Err("type error: `head` of an atom (atoms have no head)".into()),
                Ty::Noun => Ty::Noun,
            }
        }

        Ast::Tail(e) => {
            let t = infer(e, env)?;
            match t {
                Ty::Cell(_, tl) => *tl,
                Ty::Atom => return Err("type error: `tail` of an atom (atoms have no tail)".into()),
                Ty::Noun => Ty::Noun,
            }
        }

        Ast::IsCell(e) => {
            infer(e, env)?; // check inner consistency
            Ty::Atom // a loobean
        }

        Ast::Eq(a, b) => {
            infer(a, env)?;
            infer(b, env)?;
            Ty::Atom // a loobean
        }

        Ast::If(c, t, e) => {
            infer(c, env)?;
            join(infer(t, env)?, infer(e, env)?)
        }

        Ast::Let(name, val, body) => {
            let tv = infer(val, env)?;
            let mut env2 = env.clone();
            env2.insert(name.clone(), tv);
            infer(body, &env2)?
        }

        Ast::Case(subj, arms) => {
            infer(subj, env)?;
            let mut acc: Option<Ty> = None;
            for (bind, body) in arms {
                let mut env2 = env.clone();
                if let Some(n) = bind {
                    env2.insert(n.clone(), Ty::Noun); // matched value: shape not narrowed
                }
                let t = infer(body, &env2)?;
                acc = Some(match acc {
                    None => t,
                    Some(prev) => join(prev, t),
                });
            }
            acc.unwrap_or(Ty::Noun)
        }

        Ast::Loop(binds, body) => {
            // a loop is a fixpoint: check the seeds and body, but its result shape can
            // change across `again`, so we don't pin it down.
            let mut env2 = env.clone();
            for (name, seed) in binds {
                infer(seed, env)?;
                env2.insert(name.clone(), Ty::Noun);
            }
            infer(body, &env2)?;
            Ty::Noun
        }

        Ast::Again(args) => {
            for a in args {
                infer(a, env)?;
            }
            Ty::Noun // recurs; not a value in the usual sense
        }

        Ast::Call(_name, args) => {
            for a in args {
                infer(a, env)?; // check argument expressions
            }
            Ty::Noun // an arm's result shape is not tracked statically
        }

        Ast::Fast(_name, body) => infer(body, env)?,

        Ast::Gate(params, body) => {
            let mut env2 = env.clone();
            for p in params {
                env2.insert(p.clone(), Ty::Noun);
            }
            infer(body, &env2)?;
            Ty::Noun // a closure value
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latte::parse;

    fn ty(src: &str) -> Result<Ty, String> {
        check(&parse(src).expect("parses"))
    }

    #[test]
    fn literals_are_atoms() {
        assert_eq!(ty("5").unwrap(), Ty::Atom);
        assert_eq!(ty("0xff").unwrap(), Ty::Atom);
        assert_eq!(ty("%sun").unwrap(), Ty::Atom);
        assert_eq!(ty("nil").unwrap(), Ty::Atom);
    }

    #[test]
    fn cells_carry_structure() {
        assert_eq!(ty("[1 2]").unwrap(), Ty::Cell(Box::new(Ty::Atom), Box::new(Ty::Atom)));
        assert_eq!(ty("[1 [2 3]]").unwrap().show(), "[@ [@ @]]");
    }

    #[test]
    fn head_and_tail_project() {
        assert_eq!(ty("head [1 2]").unwrap(), Ty::Atom);
        assert_eq!(ty("tail [1 [2 3]]").unwrap().show(), "[@ @]");
    }

    #[test]
    fn head_of_atom_is_an_error() {
        assert!(ty("head 5").is_err());
        assert!(ty("tail 7").is_err());
    }

    #[test]
    fn increment_of_cell_is_an_error() {
        assert!(ty("+([1 2])").is_err());
        assert_eq!(ty("+(5)").unwrap(), Ty::Atom);
    }

    #[test]
    fn iscell_and_eq_are_loobeans() {
        assert_eq!(ty("iscell [1 2]").unwrap(), Ty::Atom);
        assert_eq!(ty("(5 == 6)").unwrap(), Ty::Atom);
    }

    #[test]
    fn if_joins_branches() {
        assert_eq!(ty("if (iscell 0) then 1 else 2").unwrap(), Ty::Atom);
        // conflicting branch shapes widen to the top type
        assert_eq!(ty("if (iscell 0) then 1 else [2 3]").unwrap(), Ty::Noun);
        // matching cell branches keep structure
        assert_eq!(ty("if (iscell 0) then [1 2] else [3 4]").unwrap().show(), "[@ @]");
    }

    #[test]
    fn let_binds_types() {
        assert_eq!(ty("let x = [1 2] in head x").unwrap(), Ty::Atom);
        assert!(ty("let x = 5 in head x").is_err()); // x is an atom -> head is illegal
    }

    #[test]
    fn calls_and_loops_are_top() {
        assert_eq!(ty("(add 1 2)").unwrap(), Ty::Noun);
        // but argument errors are still caught inside a call
        assert!(ty("(add (head 9) 2)").is_err());
        // head of an unknown (call) result is permitted
        assert_eq!(ty("head (foo 1)").unwrap(), Ty::Noun);
    }
}

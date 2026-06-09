//! Mold/aura support on the host side: aura-aware rendering of values *through* a
//! mold, plus a CLI demo. The mold operations themselves (`bunt`/`nest`/`clam`) live
//! in `lib/mold.lat` and run on Loom; this module only interprets the mold descriptor
//! for display and drives a demonstration.
//!
//! Mold encoding (shared with lib/mold.lat):
//!   [0 aura]    atom mold        [1 [mh mt]]  cell mold
//!   [2 me]      list mold        [3 [ma mb]]  fork mold

use crate::atom::Atom;
use crate::knot::{Knot, N};
use crate::latte;

fn head(n: &N) -> Option<N> {
    match &**n {
        Knot::Cell(h, _) => Some(h.clone()),
        _ => None,
    }
}
fn tail(n: &N) -> Option<N> {
    match &**n {
        Knot::Cell(_, t) => Some(t.clone()),
        _ => None,
    }
}
fn tag(n: &N) -> Option<u128> {
    head(n).and_then(|h| h.as_atom().and_then(|a| a.to_u128()))
}

/// Render a bare atom according to an aura.
pub fn render_aura(a: &Atom, aura: &str) -> String {
    let u = a.to_u128();
    match aura {
        "ux" => match u {
            Some(v) => format!("0x{:x}", v),
            None => format!("0x{}", hex_le(a)),
        },
        "ub" => match u {
            Some(v) => format!("0b{:b}", v),
            None => "0b…".into(),
        },
        "t" => format!("'{}'", a.as_cord().unwrap_or_default()),
        "tas" => format!("%{}", a.as_cord().unwrap_or_default()),
        _ => match u {
            // "ud", "u", "" and anything else: decimal
            Some(v) => v.to_string(),
            None => format!("0x{}", hex_le(a)),
        },
    }
}

fn hex_le(a: &Atom) -> String {
    a.bytes_le().iter().rev().map(|b| format!("{:02x}", b)).collect()
}

/// Render a value through a mold, applying auras to atom leaves.
pub fn render_with_mold(value: &N, mold: &N) -> String {
    match tag(mold) {
        Some(0) => {
            let aura = tail(mold).and_then(|t| t.as_atom().and_then(|a| a.as_cord())).unwrap_or_default();
            match &**value {
                Knot::Atom(a) => render_aura(a, &aura),
                Knot::Cell(..) => render_plain(value), // shape mismatch: print plainly
            }
        }
        Some(1) => {
            let p = tail(mold).unwrap_or_else(|| value.clone());
            let (mh, mt) = (head(&p), tail(&p));
            match (head(value), tail(value), mh, mt) {
                (Some(vh), Some(vt), Some(mh), Some(mt)) => {
                    format!("[{} {}]", render_with_mold(&vh, &mh), render_with_mold(&vt, &mt))
                }
                _ => render_plain(value),
            }
        }
        Some(2) => {
            let me = tail(mold).unwrap_or_else(|| value.clone());
            let mut parts = Vec::new();
            let mut cur = value.clone();
            while let (Some(h), Some(t)) = (head(&cur), tail(&cur)) {
                parts.push(render_with_mold(&h, &me));
                cur = t;
            }
            format!("~[{}]", parts.join(" "))
        }
        Some(3) => {
            // fork: render with whichever branch the value fits
            let p = tail(mold).unwrap_or_else(|| value.clone());
            let (a, b) = (head(&p), tail(&p));
            match (a, b) {
                (Some(a), Some(b)) => {
                    if nest(&a, value) {
                        render_with_mold(value, &a)
                    } else {
                        render_with_mold(value, &b)
                    }
                }
                _ => render_plain(value),
            }
        }
        _ => render_plain(value),
    }
}

/// Plain structural print (no auras): decimal atoms, [h t] cells.
fn render_plain(n: &N) -> String {
    match &**n {
        Knot::Atom(a) => a.to_u128().map(|v| v.to_string()).unwrap_or_else(|| format!("0x{}", hex_le(a))),
        Knot::Cell(h, t) => format!("[{} {}]", render_plain(h), render_plain(t)),
    }
}

/// Host mirror of lib/mold.lat `nest`, used only to pick a fork branch for display.
fn nest(mold: &N, value: &N) -> bool {
    match tag(mold) {
        Some(0) => matches!(&**value, Knot::Atom(_)),
        Some(1) => {
            if let (Some(p), Knot::Cell(vh, vt)) = (tail(mold), &**value) {
                if let (Some(mh), Some(mt)) = (head(&p), tail(&p)) {
                    return nest(&mh, vh) && nest(&mt, vt);
                }
            }
            false
        }
        Some(2) => {
            let me = match tail(mold) {
                Some(m) => m,
                None => return false,
            };
            let mut cur = value.clone();
            loop {
                match &*cur {
                    Knot::Atom(a) if a.is_zero() => return true,
                    Knot::Cell(h, t) => {
                        if !nest(&me, h) {
                            return false;
                        }
                        cur = t.clone();
                    }
                    _ => return false,
                }
            }
        }
        Some(3) => {
            if let Some(p) = tail(mold) {
                if let (Some(a), Some(b)) = (head(&p), tail(&p)) {
                    return nest(&a, value) || nest(&b, value);
                }
            }
            false
        }
        _ => false,
    }
}

fn run(expr: &str) -> N {
    latte::run_with_libs(expr, &["std", "mold"]).expect("mold expr runs")
}

/// `lattice mold` — a tour of the mold/aura type system.
pub fn cmd_mold() {
    println!("Orpheus molds & auras (types running on Loom)\n");

    // a record-like mold: [@ud @ux]
    let pair_src = "(mcell (matom %ud) (matom %ux))";
    let pair_mold = run(pair_src);
    println!("mold  pair = [@ud @ux]");
    println!("  bunt        = {}", render_with_mold(&run(&format!("(bunt {})", pair_src)), &pair_mold));
    println!("  nest [3 255]= {}", yesno(&run(&format!("(nest {} [3 255])", pair_src))));
    println!("  nest 7      = {}", yesno(&run(&format!("(nest {} 7)", pair_src))));
    println!("  clam 7      = {}  (coerced to the default)", render_with_mold(&run(&format!("(clam {} 7)", pair_src)), &pair_mold));
    println!("  clam [9 250]= {}", render_with_mold(&run(&format!("(clam {} [9 250])", pair_src)), &pair_mold));

    // a list mold: (list @ud)
    let list_src = "(mlist (matom %ud))";
    let list_mold = run(list_src);
    println!("\nmold  nums = (list @ud)");
    println!("  bunt              = {}", render_with_mold(&run(&format!("(bunt {})", list_src)), &list_mold));
    println!("  clam [1 [2 [3 0]]]= {}", render_with_mold(&run(&format!("(clam {} [1 [2 [3 0]]])", list_src)), &list_mold));
    println!("  nest [1 [2 3]]    = {}  (improper list)", yesno(&run(&format!("(nest {} [1 [2 3]])", list_src))));

    // one atom, several auras
    println!("\nthe atom 255 dressed by aura:");
    let a = Atom::from_u128(255);
    for aura in ["ud", "ux", "ub"] {
        println!("  @{:<3} = {}", aura, render_aura(&a, aura));
    }
    let word = crate::knot::cord("liɣō");
    if let Knot::Atom(t) = &*word {
        println!("  @t   = {}", render_aura(t, "t"));
        println!("  @tas = {}", render_aura(t, "tas"));
    }
}

fn yesno(n: &N) -> &'static str {
    match n.as_atom().and_then(|a| a.to_u128()) {
        Some(0) => "yes",
        _ => "no",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knot::{cell, cord, num};

    #[test]
    fn aura_rendering() {
        let a = Atom::from_u128(255);
        assert_eq!(render_aura(&a, "ud"), "255");
        assert_eq!(render_aura(&a, "ux"), "0xff");
        assert_eq!(render_aura(&a, "ub"), "0b11111111");
        if let Knot::Atom(t) = &*cord("hi") {
            assert_eq!(render_aura(t, "t"), "'hi'");
            assert_eq!(render_aura(t, "tas"), "%hi");
        }
    }

    #[test]
    fn render_pair_with_auras() {
        // mold [@ud @ux] = [1 [[0 'ud'] [0 'ux']]], value [10 255]  ->  "[10 0xff]"
        let mold = cell(
            num(1),
            cell(cell(num(0), cord("ud")), cell(num(0), cord("ux"))),
        );
        let value = cell(num(10), num(255));
        assert_eq!(render_with_mold(&value, &mold), "[10 0xff]");
    }

    #[test]
    fn molds_run_on_loom() {
        // the Latte mold ops produce the values we render
        assert_eq!(run("(bunt (mcell (matom %ud) (matom %ux)))"), cell(num(0), num(0)));
        assert_eq!(run("(nest (mlist (matom %ud)) [1 [2 [3 0]]])"), num(0)); // nests
        assert_eq!(run("(clam (matom %ud) [9 9])"), num(0)); // cell coerced to atom default
    }
}

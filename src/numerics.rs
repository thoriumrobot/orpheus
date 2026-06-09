//! Host front-ends and tests for the numeric Latte libraries: signed fixed-point
//! numbers (`num`), n-dimensional tensors (`tensor`), and machine-learning training
//! (`ml`). The math lives in `lib/*.lat`; this host links the libraries, runs demos,
//! and formats fixed-point results.

use crate::knot::{Knot, N};
use crate::latte;

fn run(expr: &str, libs: &[&str]) -> Result<N, String> {
    latte::run_with_libs(expr, libs)
}

/// Format a signed fixed-point number `[sign mag]` as a decimal string.
fn show_signed(n: &N) -> String {
    if let Knot::Cell(s, m) = &**n {
        let sign = s.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = m.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let sgn = if sign == 1 && mag != 0 { "-" } else { "" };
        format!("{}{}.{:03}", sgn, mag / 1000, mag % 1000)
    } else {
        n.as_atom().and_then(|a| a.to_u128()).map(|x| x.to_string()).unwrap_or_else(|| "?".into())
    }
}

/// Collect a Latte list into a Vec of its elements.
fn to_list(n: &N) -> Vec<N> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        out.push(h.clone());
        cur = t.clone();
    }
    out
}

pub fn cmd_tensor(_args: &[String]) {
    println!("tensor — n-dimensional tensors in Latte (lib/tensor.lat)\n");
    let demos: &[(&str, &str)] = &[
        ("shape of a 2x3 tensor", "(tshape (treshape (tfromnats [1 [2 [3 [4 [5 [6 0]]]]]]) [2 [3 0]]))"),
        ("element [1,2] of that 2x3 (row-major, 0-indexed)", "(tget (treshape (tfromnats [1 [2 [3 [4 [5 [6 0]]]]]]) [2 [3 0]]) [1 [2 0]])"),
        ("sum of all elements", "(tsum (tfromnats [1 [2 [3 [4 0]]]]))"),
        ("dot product [1,2,3]·[4,5,6]", "(tdot (tfromnats [1 [2 [3 0]]]) (tfromnats [4 [5 [6 0]]]))"),
    ];
    for (label, expr) in demos {
        match run(expr, &["std", "num", "tensor"]) {
            Ok(v) => println!("  {:<46} = {}", label, render_tensor_result(&v)),
            Err(e) => println!("  {:<46} ! {}", label, e),
        }
    }
    // 2x2 matmul
    match run(
        "(tmatmul (treshape (tfromnats [1 [2 [3 [4 0]]]]) [2 [2 0]]) (treshape (tfromnats [5 [6 [7 [8 0]]]]) [2 [2 0]]))",
        &["std", "num", "tensor"],
    ) {
        Ok(v) => {
            let data = to_list(&tail(&v));
            let nums: Vec<String> = data.iter().map(show_signed).collect();
            println!("  {:<46} = [{}] shape 2x2", "[[1,2],[3,4]] · [[5,6],[7,8]]", nums.join(", "));
        }
        Err(e) => println!("  matmul ! {}", e),
    }
}

pub fn cmd_ml(args: &[String]) {
    let mut iters: u64 = 5000;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--iters" {
            i += 1;
            if i < args.len() {
                iters = args[i].parse().unwrap_or(5000);
            }
        }
        i += 1;
    }
    println!("ml — training a model in Latte by gradient descent (lib/ml.lat)\n");
    println!("  task: fit  y = w·x + b  to the points (1,3) (2,5) (3,7) (4,9)  [true w=2, b=1]");
    println!("  {} iterations of gradient descent, learning rate 0.2\n", iters);
    let expr = format!("(fit_demo {})", iters);
    match run(&expr, &["std", "num", "tensor", "ml"]) {
        Ok(v) => {
            // result is [w b]
            if let Knot::Cell(w, b) = &*v {
                println!("  learned w = {}", show_signed(w));
                println!("  learned b = {}", show_signed(b));
            } else {
                println!("  result: {}", show_signed(&v));
            }
        }
        Err(e) => println!("  error: {}", e),
    }
}

fn tail(n: &N) -> N {
    match &**n {
        Knot::Cell(_, t) => t.clone(),
        _ => n.clone(),
    }
}

fn render_tensor_result(v: &N) -> String {
    // a 0-terminated list of plain atoms (e.g. a shape) renders as [a b c]
    if let Some(xs) = plain_nat_list(v) {
        let parts: Vec<String> = xs.iter().map(|x| x.to_string()).collect();
        return format!("[{}]", parts.join(" "));
    }
    match &**v {
        Knot::Cell(_, _) => show_signed(v),
        Knot::Atom(a) => a.to_u128().map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
    }
}

/// If `n` is a 0-terminated cons list whose elements are all plain atoms, return them.
fn plain_nat_list(n: &N) -> Option<Vec<u128>> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    loop {
        match &*cur {
            Knot::Atom(a) if a.is_zero() => return Some(out),
            Knot::Cell(h, t) => {
                out.push(h.as_atom().and_then(|a| a.to_u128())?);
                cur = t.clone();
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knot::{cell, num};

    fn n(e: &str, libs: &[&str]) -> N {
        run(e, libs).unwrap()
    }


    #[test]
    fn ml_loss_is_zero_at_optimum() {
        // mse with the true weights w=2,b=1 on the exact data is 0
        let v = run(
            "(mse [0 2000] [0 1000] (tdata (tfromnats [1 [2 [3 [4 0]]]])) (tdata (tfromnats [3 [5 [7 [9 0]]]])))",
            &["std", "num", "tensor", "ml"],
        )
        .unwrap();
        assert_eq!(v, cell(num(0), num(0)));
    }

    #[test]
    fn ml_linear_regression_learns() {
        // after enough steps, gradient descent recovers w≈2, b≈1 (fixed-point ×1000)
        let v = run("(fit_demo 5000)", &["std", "num", "tensor", "ml"]).unwrap();
        let (w, b) = v.as_cell().unwrap();
        let mag = |x: &N| x.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap() as i64;
        let near = |a: i64, t: i64, tol: i64| (a - t).abs() <= tol;
        assert!(near(mag(&w), 2000, 40), "w mag {} not ~2000", mag(&w));
        assert!(near(mag(&b), 1000, 60), "b mag {} not ~1000", mag(&b));
    }

    #[test]
    fn tensor_ops() {
        let l = &["std", "num", "tensor"];
        // dot product [1,2,3]·[4,5,6] = 32
        assert_eq!(n("(tdot (tfromnats [1 [2 [3 0]]]) (tfromnats [4 [5 [6 0]]]))", l), cell(num(0), num(32000)));
        // sum of [1,2,3,4] = 10
        assert_eq!(n("(tsum (tfromnats [1 [2 [3 [4 0]]]]))", l), cell(num(0), num(10000)));
        // 2x2 matmul [[1,2],[3,4]]·[[5,6],[7,8]] = [[19,22],[43,50]]
        let m = n("(tmatmul (treshape (tfromnats [1 [2 [3 [4 0]]]]) [2 [2 0]]) (treshape (tfromnats [5 [6 [7 [8 0]]]]) [2 [2 0]]))", l);
        let got: Vec<u128> = to_list(&tail(&m)).iter().map(|e| e.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap()).collect();
        assert_eq!(got, vec![19000, 22000, 43000, 50000]);
    }



    #[test]
    fn signed_arithmetic() {
        let l = &["std", "num"];
        assert_eq!(n("(nadd [0 1000] [0 2000])", l), cell(num(0), num(3000))); // 1 + 2 = 3
        assert_eq!(n("(nadd [0 1000] [1 3000])", l), cell(num(1), num(2000))); // 1 + -3 = -2
        assert_eq!(n("(nmul [1 2000] [0 3000])", l), cell(num(1), num(6000))); // -2 * 3 = -6
        assert_eq!(n("(nsub [0 1000] [0 3000])", l), cell(num(1), num(2000))); // 1 - 3 = -2
        assert_eq!(n("(ndiv [0 6000] [0 2000])", l), cell(num(0), num(3000))); // 6 / 2 = 3
        assert_eq!(n("(nsub [0 3000] [0 3000])", l), cell(num(0), num(0)));    // 3 - 3 = +0
    }
}

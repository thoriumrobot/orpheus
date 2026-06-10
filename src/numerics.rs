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
    // optional first arg selects the model; default is linear regression
    let model = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("linear");
    let mut iters: u64 = if model == "linear" { 5000 } else { 20 };
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--iters" || args[i] == "--epochs") && i + 1 < args.len() {
            iters = args[i + 1].parse().unwrap_or(iters);
            i += 1;
        }
        i += 1;
    }
    let libs = ["std", "num", "tensor", "ml"];
    let nats = |n: &N| -> String {
        to_list(n)
            .iter()
            .map(|x| x.as_atom().and_then(|a| a.to_u128()).map(|v| v.to_string()).unwrap_or("?".into()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    match model {
        "perceptron" => {
            println!("ml — perceptron, an online linear classifier (lib/ml.lat)\n");
            println!("  task: classify x>0 as class 1, x<0 as class 0 (points +3 +1 −2 −4)");
            println!("  {} epochs, learning rate 0.2\n", iters);
            match (run(&format!("(perceptron_demo {})", iters), &libs), run(&format!("(perceptron_check {})", iters), &libs)) {
                (Ok(wb), Ok(pred)) => {
                    if let Knot::Cell(w, b) = &*wb {
                        println!("  learned w = {}", show_signed(w));
                        println!("  learned b = {}", show_signed(b));
                    }
                    println!("  predictions on training points (want 1 1 0 0): {}", nats(&pred));
                }
                (Err(e), _) | (_, Err(e)) => println!("  error: {}", e),
            }
        }
        "kmeans" => {
            println!("ml — k-means clustering, k = 2 (lib/ml.lat)\n");
            println!("  task: cluster the 1-D points {{1, 2, 3, 10, 11, 12}}");
            println!("  {} iterations, initial centroids 0.0 and 9.0\n", iters);
            match run(&format!("(kmeans_demo {})", iters), &libs) {
                Ok(cs) => {
                    if let Knot::Cell(c0, c1) = &*cs {
                        println!("  centroid A = {}", show_signed(c0));
                        println!("  centroid B = {}", show_signed(c1));
                    }
                }
                Err(e) => println!("  error: {}", e),
            }
        }
        "knn" => {
            println!("ml — k-nearest-neighbour classification, k = 1 (lib/ml.lat)\n");
            println!("  train: points 1,2 → class 0 ; points 9,10 → class 1\n");
            for (label, q) in [("3.0", "[0 3000]"), ("8.0", "[0 8000]"), ("1.5", "[0 1500]")] {
                match run(&format!("(knn_demo {})", q), &libs) {
                    Ok(v) => println!("  query x = {:>4}  →  class {}", label, show_signed(&v)),
                    Err(e) => println!("  error: {}", e),
                }
            }
        }
        _ => {
            println!("ml — training a model in Latte by gradient descent (lib/ml.lat)\n");
            println!("  task: fit  y = w·x + b  to the points (1,3) (2,5) (3,7) (4,9)  [true w=2, b=1]");
            println!("  {} iterations of gradient descent, learning rate 0.2\n", iters);
            match run(&format!("(fit_demo {})", iters), &libs) {
                Ok(v) => {
                    if let Knot::Cell(w, b) = &*v {
                        println!("  learned w = {}", show_signed(w));
                        println!("  learned b = {}", show_signed(b));
                    } else {
                        println!("  result: {}", show_signed(&v));
                    }
                }
                Err(e) => println!("  error: {}", e),
            }
            println!("\n  other models: latte ml perceptron | kmeans | knn");
        }
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

/// A signed fixed-point number as an `f64` (magnitude / 1000, with sign).
fn signed_f64(n: &N) -> f64 {
    if let Knot::Cell(s, m) = &**n {
        let sign = s.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = m.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64 / 1000.0;
        if sign == 1 { -mag } else { mag }
    } else {
        0.0
    }
}

/// `latte nn` — train a small multilayer perceptron by backprop, in Latte.
pub fn cmd_nn(args: &[String]) {
    let libs = ["std", "num", "tensor", "ml", "nn"];
    let mut epochs: u64 = 300;
    let mut out = "nn-loss.svg".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--epochs" | "--iters" if i + 1 < args.len() => {
                epochs = args[i + 1].parse().unwrap_or(epochs);
                i += 1;
            }
            "--out" if i + 1 < args.len() => {
                out = args[i + 1].clone();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    println!("nn — a neural network trained by backpropagation in Latte (lib/nn.lat)\n");
    println!("  network : 1 input -> 2 ReLU hidden units -> 1 linear output (an MLP)");
    println!("  task    : learn  y = |x|  — a linear model cannot; one ReLU layer can");
    println!("  training: {} epochs of full-batch backprop, learning rate 0.08\n", epochs);
    let stride = (epochs / 20).max(1);
    let mut losses = Vec::new();
    let mut e = 0u64;
    loop {
        match run(&format!("(nn_demo_loss {})", e), &libs) {
            Ok(v) => losses.push(signed_f64(&v)),
            Err(err) => {
                println!("  error: {}", err);
                return;
            }
        }
        if e >= epochs {
            break;
        }
        e = (e + stride).min(epochs);
    }
    println!(
        "  loss (MSE):  start {:.3}   ->   end {:.3}",
        losses.first().copied().unwrap_or(0.0),
        losses.last().copied().unwrap_or(0.0)
    );
    if let Ok(p) = run(&format!("(nn_demo_pred {})", epochs), &libs) {
        let preds: Vec<String> = to_list(&p).iter().map(show_signed).collect();
        println!("  inputs   x =  -3.0  -1.0   1.0   3.0");
        println!("  target |x| =   3.0   1.0   1.0   3.0");
        println!("  network    =  {}", preds.iter().map(|s| format!("{:>4}", s)).collect::<Vec<_>>().join("  "));
    }
    let svg = crate::viz::render_lines("MLP training loss (MSE) — learning y = |x|", &["loss"], &[losses]);
    match std::fs::write(&out, &svg) {
        Ok(_) => println!("\n  loss curve written to {}  (open in a browser)", out),
        Err(e) => println!("\n  (could not write {}: {})", out, e),
    }
    println!("\n  Composable layers (lib/nn.lat): build deeper nets as a list of layers —");
    println!("  %dense / %relu / %tanh / %res (residual skip). `net_fwd` folds the forward");
    println!("  pass, so ResNet-style blocks (`resblock`) are ordinary data.");
}

/// `latte fin gold` — a practical financial-ML pipeline on real gold/USD data.
pub fn cmd_fin(args: &[String]) {
    let libs = ["std", "num", "tensor", "ml", "nn", "fin"];
    let mut k: usize = 5;
    let mut iters: u64 = 40;
    let mut out = "gold-fin.svg".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--k" | "--window" if i + 1 < args.len() => {
                k = args[i + 1].parse().unwrap_or(k);
                i += 1;
            }
            "--iters" | "--epochs" if i + 1 < args.len() => {
                iters = args[i + 1].parse().unwrap_or(iters);
                i += 1;
            }
            "--out" if i + 1 < args.len() => {
                out = args[i + 1].clone();
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let closes = crate::golddata::GOLD_CLOSE_X100;
    let n = closes.len();
    let nsamples = n.saturating_sub(1).saturating_sub(k);
    let split = (nsamples * 7) / 10;
    let (d0, d1) = crate::golddata::GOLD_SPAN;
    println!("fin — practical financial machine learning, in Latte (lib/fin.lat)\n");
    println!("  data    : {} daily XAU/USD closes, {} -> {}", n, d0, d1);
    println!("  features : the previous {} daily returns (percent)", k);
    println!("  label    : next-day direction, up=1 / down=0  [fixed-horizon labeling]");
    println!("  model    : logistic regression, {} epochs of batch gradient descent", iters);
    println!("  validation: walk-forward — {} train days then {} unseen test days (no shuffle)\n", split, nsamples - split);
    let mut plist = String::from("0");
    for &c in closes.iter().rev() {
        plist = format!("[ [0 {}] {} ]", c * 10, plist);
    }
    let expr = format!("(gold_run {} {} {} {})", plist, k, iters, split);
    match run(&expr, &libs) {
        Ok(v) => {
            let parts = to_list(&v);
            if parts.len() >= 5 {
                let train = signed_f64(&parts[0]) * 100.0;
                let test = signed_f64(&parts[1]) * 100.0;
                let base = signed_f64(&parts[2]) * 100.0;
                let strat: Vec<f64> = to_list(&parts[3]).iter().map(signed_f64).collect();
                let buyhold: Vec<f64> = to_list(&parts[4]).iter().map(signed_f64).collect();
                println!("  -- results --------------------------------");
                println!("  training accuracy   : {:.1}%", train);
                println!("  TEST accuracy        : {:.1}%   (out-of-sample)", test);
                println!("  baseline (majority) : {:.1}%", base);
                println!("  edge over baseline  : {:+.1} pts", test - base);
                println!(
                    "  test-set return     : strategy {:+.1}%   buy & hold {:+.1}%",
                    strat.last().copied().unwrap_or(0.0),
                    buyhold.last().copied().unwrap_or(0.0)
                );
                println!("\n  Note: daily price direction is close to a coin flip — financial data is");
                println!("  low signal-to-noise (cf. Lopez de Prado, Advances in Financial ML). Any");
                println!("  edge here is small and fragile; this shows the *pipeline*, not a profit machine.");
                let svg = crate::viz::render_lines(
                    "Gold/USD out-of-sample equity: strategy vs buy & hold (cumulative %)",
                    &["strategy", "buy & hold"],
                    &[strat, buyhold],
                );
                match std::fs::write(&out, &svg) {
                    Ok(_) => println!("\n  equity curve written to {}  (open in a browser)", out),
                    Err(e) => println!("\n  (could not write {}: {})", out, e),
                }
            } else {
                println!("  unexpected result shape ({} parts)", parts.len());
            }
        }
        Err(e) => println!("  error: {}", e),
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
    fn ml_new_models_learn() {
        let l = &["std", "num", "tensor", "ml"];
        // perceptron classifies the (separable) training set perfectly: predictions 1 1 0 0
        let pred = to_list(&run("(perceptron_check 10)", l).unwrap());
        let bits: Vec<u128> = pred.iter().map(|x| x.as_atom().unwrap().to_u128().unwrap()).collect();
        assert_eq!(bits, vec![1, 1, 0, 0], "perceptron predictions");
        // k-means recovers centroids ≈ 2.0 and 11.0 for {1,2,3, 10,11,12}
        let cs = run("(kmeans_demo 8)", l).unwrap();
        let (c0, c1) = cs.as_cell().unwrap();
        let mag = |x: &N| x.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap() as i64;
        assert!((mag(&c0) - 2000).abs() <= 1, "centroid A {} not ~2000", mag(&c0));
        assert!((mag(&c1) - 11000).abs() <= 1, "centroid B {} not ~11000", mag(&c1));
        // 1-NN classifies queries by the nearest training point
        let cls = |q: &str| run(&format!("(knn_demo {})", q), l).unwrap().as_atom().unwrap().to_u128().unwrap();
        assert_eq!(cls("[0 3000]"), 0); // 3.0 → nearest 2.0 → class 0
        assert_eq!(cls("[0 8000]"), 1); // 8.0 → nearest 9.0 → class 1
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

    #[test]
    fn signed_ml_helpers() {
        let l = &["std", "num"];
        let mag = |x: &N| x.as_cell().unwrap().1.as_atom().unwrap().to_u128().unwrap() as i64;
        assert_eq!(n("(nint 100)", l), cell(num(0), num(100000))); // whole 100 -> 100.000
        assert_eq!(n("(ndot [ [0 1000] [ [0 2000] 0 ] ] [ [0 3000] [ [0 4000] 0 ] ])", l), cell(num(0), num(11000))); // 1*3+2*4
        assert!((mag(&n("(nsqrt [0 4000])", l)) - 2000).abs() <= 2); // sqrt 4 = 2
        assert!((mag(&n("(nsqrt [0 2000])", l)) - 1414).abs() <= 3); // sqrt 2 ≈ 1.414
        assert!((mag(&n("(nsigmoid [0 1000])", l)) - 750).abs() <= 1); // fast-sigmoid(1) = 0.75
        assert_eq!(n("(nsigmoid [1 1000])", l), cell(num(0), num(250))); // fast-sigmoid(-1) = 0.25
    }

    #[test]
    fn nn_mlp_learns_abs_by_backprop() {
        // a 2-ReLU-hidden MLP fits y=|x| (impossible for a linear model): loss falls to ~0
        let l = &["std", "num", "tensor", "ml", "nn"];
        let loss0 = signed_f64(&run("(nn_demo_loss 0)", l).unwrap());
        let loss = signed_f64(&run("(nn_demo_loss 300)", l).unwrap());
        assert!(loss0 > 1.0, "start loss {} should be large", loss0);
        assert!(loss < 0.05, "trained loss {} should be ~0", loss);
        // predictions approach the targets 3,1,1,3
        let preds: Vec<f64> = to_list(&run("(nn_demo_pred 300)", l).unwrap()).iter().map(signed_f64).collect();
        let want = [3.0, 1.0, 1.0, 3.0];
        for (p, w) in preds.iter().zip(want.iter()) {
            assert!((p - w).abs() < 0.2, "pred {} not ~{}", p, w);
        }
    }

    #[test]
    fn fin_logistic_learns_mean_reversion() {
        // alternating prices make the last return perfectly predictive of the next
        // direction; logistic regression must beat the 50% majority baseline.
        let l = &["std", "num", "tensor", "ml", "nn", "fin"];
        let prices = "(map (fn [i] -> (npos (add 100000 (mul (mod i 2) 1000)))) (range 120))";
        let r = to_list(&run(&format!("(pipeline {} 1 60 80)", prices), l).unwrap());
        let acc: Vec<f64> = r.iter().map(signed_f64).collect();
        assert!(acc[0] > 0.95, "train acc {} should be ~1", acc[0]);
        assert!(acc[1] > 0.95, "test acc {} should be ~1", acc[1]);
        assert!((acc[2] - 0.5).abs() < 0.1, "baseline {} should be ~0.5", acc[2]);
    }

    #[test]
    fn nn_composable_residual_forward() {
        // a residual block adds its input back: relu(W2·relu(W1 x + b1)+b2) + x.
        // with W1=I, b1=0, W2=I, b2=0 on x=[2], inner = relu(2)=2, +x => 4.
        let l = &["std", "num", "tensor", "ml", "nn"];
        let expr = "(net_fwd [ (resblock [ [ [0 1000] 0 ] 0 ] [ [0 0] 0 ] [ [ [0 1000] 0 ] 0 ] [ [0 0] 0 ]) 0 ] [ [0 2000] 0 ])";
        let v = to_list(&run(expr, l).unwrap());
        assert_eq!(v.len(), 1);
        assert_eq!(signed_f64(&v[0]), 4.0);
    }
}

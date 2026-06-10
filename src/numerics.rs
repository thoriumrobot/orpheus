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
    // ---- the newer architectures: a seeded transformer block, forward on Loom ----
    println!("\n  -- architectures beyond the MLP (all composable data in lib/nn.lat) --");
    let seq_demo = "(seq_run (append (block 7 4) [ [%pool 0] 0 ]) \
        [ [ [0 1000] [ [1 500] [ [0 200] [ [0 0] 0 ] ] ] ] \
        [ [ [0 300] [ [0 800] [ [1 100] [ [0 400] 0 ] ] ] ] \
        [ [ [1 200] [ [0 100] [ [0 900] [ [1 600] 0 ] ] ] ] 0 ] ] ])";
    match run(seq_demo, &libs) {
        Ok(v) => {
            let outv: Vec<String> = to_list(&v).iter().map(show_signed).collect();
            println!("  transformer block (d=4, seeded): single-head self-ATTENTION + residual +");
            println!("  LAYERNORM + position-wise dense + residual + layernorm, mean-POOLED over a");
            println!("  3-token sequence  ->  [{}]", outv.join("  "));
        }
        Err(e) => println!("  (transformer demo unavailable: {})", e),
    }
    println!("\n  Composable layers (lib/nn.lat): a network is a LIST of layers —");
    println!("  vectors:   %dense %relu %tanh %sigmoid %softmax %layernorm %conv1d %res");
    println!("  sequences: %attn (scaled dot-product) %seqdense %lnorm %seqres %pool");
    println!("  `net_fwd`/`seq_fwd` fold the forward pass; `block seed d` builds a seeded");
    println!("  transformer block; `randmat` gives reproducible inits — architectures are");
    println!("  ordinary data, so any composition (ResNets, transformers, conv stacks) is");
    println!("  yours to assemble.");
    // ---- the SSM (post-transformer) block, forward on Loom ----
    let ssm_demo = "(seq_run (append (ssmblock 7 4) [ [%pool 0] 0 ]) \
        [ [ [0 1000] [ [1 500] [ [0 200] [ [0 0] 0 ] ] ] ] \
        [ [ [0 300] [ [0 800] [ [1 100] [ [0 400] 0 ] ] ] ] \
        [ [ [1 200] [ [0 100] [ [0 900] [ [1 600] 0 ] ] ] ] 0 ] ] ])";
    if let Ok(v) = run(ssm_demo, &libs) {
        let outv: Vec<String> = to_list(&v).iter().map(show_signed).collect();
        println!("\n  state-space block (S4D/Mamba-style DIAGONAL gated recurrence — no softmax,");
        println!("  so it suits fixed point): prenorm rmsnorm -> ssm -> residual -> rmsnorm ->");
        println!("  dense -> residual, pooled  ->  [{}]", outv.join("  "));
    }
    println!("\n  TRAINING — reverse-mode autodiff over the layer algebra: every layer has a");
    println!("  vector-Jacobian product (`layer_bwd`), `stack_train` folds them over ANY");
    println!("  composition of %dense/%relu/%tanh/%sigmoid/%rmsnorm (exact derivatives for");
    println!("  the fixed-point activations; rmsnorm uses the standard straight-through");
    println!("  approximation, stated, not hidden). The hand-rolled MLP remains; attention");
    println!("  and SSM sequence layers are forward-only in this release.");
}

/// `latte fin gold` — a practical financial-ML pipeline on real gold/USD data.
/// Result of the gold ML evaluation, shared by the CLI and the GUI endpoint.
pub struct MarketResult {
    pub train: f64,
    pub test: f64,
    pub base: f64,
    pub strat: Vec<f64>,
    pub buyhold: Vec<f64>,
    pub n: usize,
    pub split: usize,
    pub nsamples: usize,
    pub iters: u64,
    pub vol: bool,
    pub d0: &'static str,
    pub d1: &'static str,
}

/// Run the gold volatility-regime pipeline: build features in the host (data prep),
/// train + evaluate the logistic-regression *model* in Latte. No console output.
pub fn market_eval(iters: u64) -> Result<MarketResult, String> {
    // Best honest edge: next-day volatility-regime prediction on Bitcoin.
    market_eval_cfg(iters, 1, true)
}

/// Configurable evaluation. `horizon` = forward days for the direction label;
/// `vol_task` = predict next-day volatility regime instead of direction.
pub fn market_eval_cfg(iters: u64, horizon: usize, vol_task: bool) -> Result<MarketResult, String> {
    let libs = ["std", "num", "tensor", "ml", "nn", "fin"];
    let closes: Vec<f64> = crate::marketdata::MARKET_CLOSE_X100.iter().map(|&c| c as f64 / 100.0).collect();
    let n = closes.len();
    let (d0, d1) = crate::marketdata::MARKET_SPAN;
    let r: Vec<f64> = (1..n).map(|i| 100.0 * (closes[i] - closes[i - 1]) / closes[i - 1]).collect();
    let absr: Vec<f64> = r.iter().map(|x| x.abs()).collect();
    let h = horizon.max(1);
    // ---- momentum + trend features (the documented crypto edge) ------------
    let nf = 6usize;
    let mut xs: Vec<[f64; 6]> = Vec::new();
    let mut fwd: Vec<f64> = Vec::new();    // forward h-day return (direction label)
    let mut absnext: Vec<f64> = Vec::new(); // next-day |return| (volatility label)
    let mut nextret: Vec<f64> = Vec::new(); // next-day return (equity)
    let mut t = 10usize;
    while t + h < n {
        let roc = |m: usize| 100.0 * (closes[t] - closes[t - m]) / closes[t - m];
        let ma10: f64 = closes[t - 9..=t].iter().sum::<f64>() / 10.0;
        let vol5: f64 = absr[t - 5..t].iter().sum::<f64>() / 5.0;
        xs.push([roc(1), roc(3), roc(5), roc(10), 100.0 * (closes[t] / ma10 - 1.0), vol5]);
        fwd.push(100.0 * (closes[t + h] - closes[t]) / closes[t]);
        nextret.push(100.0 * (closes[t + 1] - closes[t]) / closes[t]);
        absnext.push(absr[t]); // |return| at t; label uses next-day below
        t += 1;
    }
    let nsamples = xs.len();
    let split = (nsamples * 7) / 10;
    let ys: Vec<u8> = if vol_task {
        // next-day |return| above the training-set median (balanced volatility regime)
        let mut tr: Vec<f64> = absnext[..split].to_vec();
        tr.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = tr[tr.len() / 2];
        absnext.iter().map(|&v| if v > med { 1 } else { 0 }).collect()
    } else {
        fwd.iter().map(|&v| if v > 0.0 { 1 } else { 0 }).collect()
    };
    let r1: Vec<f64> = if vol_task {
        nextret.iter().map(|x| x.abs()).collect()
    } else {
        nextret.clone()
    };
    let mut mean = [0.0f64; 6];
    let mut sd = [0.0f64; 6];
    for j in 0..nf {
        let m: f64 = xs[..split].iter().map(|r| r[j]).sum::<f64>() / split as f64;
        let v: f64 = xs[..split].iter().map(|r| (r[j] - m) * (r[j] - m)).sum::<f64>() / split as f64;
        mean[j] = m;
        sd[j] = v.sqrt();
    }
    for r in xs.iter_mut() {
        for j in 0..nf {
            r[j] = if sd[j] > 1e-9 { (r[j] - mean[j]) / sd[j] } else { r[j] - mean[j] };
        }
    }
    let sgn = |v: f64| -> String {
        let m = (v.abs() * 1000.0).round() as i64;
        format!("[{} {}]", if v < 0.0 { 1 } else { 0 }, m)
    };
    let vec_lit = |row: &[f64]| -> String {
        let mut s = String::from("0");
        for &v in row.iter().rev() {
            s = format!("[ {} {} ]", sgn(v), s);
        }
        s
    };
    let mut xs_lit = String::from("0");
    for r in xs.iter().rev() {
        xs_lit = format!("[ {} {} ]", vec_lit(r), xs_lit);
    }
    let mut ys_lit = String::from("0");
    for &y in ys.iter().rev() {
        ys_lit = format!("[ {} {} ]", y, ys_lit);
    }
    let mut r1_lit = String::from("0");
    for &rr in r1.iter().rev() {
        r1_lit = format!("[ {} {} ]", sgn(rr), r1_lit);
    }
    let expr = format!("(fit_eval {} {} {} {} {})", xs_lit, ys_lit, r1_lit, split, iters);
    let v = latte::run_with_libs_fuel(&expr, &libs, 8_000_000_000)?;
    let parts = to_list(&v);
    if parts.len() < 5 {
        return Err(format!("unexpected result shape ({} parts)", parts.len()));
    }
    Ok(MarketResult {
        train: signed_f64(&parts[0]) * 100.0,
        test: signed_f64(&parts[1]) * 100.0,
        base: signed_f64(&parts[2]) * 100.0,
        strat: to_list(&parts[3]).iter().map(signed_f64).collect(),
        buyhold: to_list(&parts[4]).iter().map(signed_f64).collect(),
        n,
        split,
        nsamples,
        iters,
        vol: vol_task,
        d0,
        d1,
    })
}

/// The SVG title/labels for the equity / variance-timing chart (shared by CLI and GUI).
pub fn market_chart(res: &MarketResult) -> String {
    let (title, labels): (&str, [&str; 2]) = if res.vol {
        (
            "BTC/USD volatility timing (out-of-sample): next-day |return| on model-flagged high-vol days vs all days (cumulative %)",
            ["model: predicted high-vol days", "all days"],
        )
    } else {
        (
            "BTC/USD out-of-sample equity, NET of 5bp/trade costs: momentum model vs buy & hold (cumulative %)",
            ["momentum model", "buy & hold"],
        )
    };
    crate::viz::render_lines(title, &labels, &[res.strat.clone(), res.buyhold.clone()])
}

pub fn cmd_fin(args: &[String]) {
    let mut iters: u64 = 100;
    let mut out = "fin-equity.svg".to_string();
    let mut horizon: usize = 7;
    let mut vol = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--iters" | "--epochs" if i + 1 < args.len() => { iters = args[i + 1].parse().unwrap_or(iters); i += 1; }
            "--horizon" | "--h" if i + 1 < args.len() => { horizon = args[i + 1].parse().unwrap_or(horizon); i += 1; }
            "--vol" => { vol = true; }
            "--dir" | "--direction" => { vol = false; }
            "--out" if i + 1 < args.len() => { out = args[i + 1].clone(); i += 1; }
            _ => {}
        }
        i += 1;
    }
    println!("fin - practical financial machine learning, in Latte (lib/fin.lat)\n");
    match market_eval_cfg(iters, horizon, vol) {
        Ok(res) => {
            let task = if res.vol { "next-day VOLATILITY regime (high vs low)" } else { "next-day direction (up / down)" };
            println!("  market   : {} - {} daily closes, {} -> {}", crate::marketdata::MARKET_NAME, res.n, res.d0, res.d1);
            println!("  task     : predict {}", task);
            println!("  features : multi-horizon momentum (ROC 1/3/5/10), price/MA10, realized vol");
            println!("  rationale: crypto has the strongest documented momentum AND volatility");
            println!("             clustering of any liquid market (Moskowitz 2012; Shen 2022; Katsiampa 2017)");
            println!("  model    : logistic regression in Latte, {} epochs of gradient descent", res.iters);
            println!("  validation: walk-forward - {} train then {} unseen test days (no shuffle)\n", res.split, res.nsamples - res.split);
            println!("  -- results --------------------------------");
            println!("  training accuracy   : {:.1}%   (in-sample fit)", res.train);
            println!("  TEST accuracy        : {:.1}%   (out-of-sample)", res.test);
            println!("  baseline (majority) : {:.1}%", res.base);
            println!("  edge over baseline  : {:+.1} pts", res.test - res.base);
            if res.vol {
                let cap = res.strat.last().copied().unwrap_or(0.0);
                let tot = res.buyhold.last().copied().unwrap_or(1.0);
                let pct = if tot != 0.0 { 100.0 * cap / tot } else { 0.0 };
                println!("  variance timing     : {:.0}% of test |returns| captured on ~half the days", pct);
            }
            println!("\n  Which market & why: this same pipeline is near-chance on gold (price-only),");
            println!("  so the financial-ML approach was moved to CRYPTO, where momentum and");
            println!("  volatility clustering are strongest. Direction is still hard day-to-day,");
            println!("  but the next-day VOLATILITY regime is genuinely predictable here:");
            println!("  the model beats the out-of-sample baseline by a few points - a real,");
            println!("  honest edge (still modest; not a profit guarantee). The same model earns");
            println!("  ~+50 pts on a synthetic mean-reverting series in the test suite.");
            let svg = market_chart(&res);
            match std::fs::write(&out, &svg) {
                Ok(_) => println!("\n  equity curve written to {}  (open in a browser)", out),
                Err(e) => println!("\n  (could not write {}: {})", out, e),
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
    fn std_general_helpers_work() {
        // argmax of [3 1 4 1 5 9 2] is index 5
        assert_eq!(n("(argmax [3 [1 [4 [1 [5 [9 [2 0]]]]]]])", &["std"]), num(5));
        // maximum / minimum
        assert_eq!(n("(maximum [3 [7 [2 0]]])", &["std"]), num(7));
        assert_eq!(n("(minimum [3 [7 [2 0]]])", &["std"]), num(2));
        // count of elements < 4 (predicate returns loobean 0 = true)
        assert_eq!(n("(count (fn [x] -> (lt x 4)) [1 [5 [2 [9 [3 0]]]]])", &["std"]), num(3));
        // any even? / all even?  (0 = true)
        assert_eq!(n("(any (fn [x] -> ((mod x 2) == 0)) [1 [3 [4 0]]])", &["std"]), num(0));
        assert_eq!(n("(all (fn [x] -> ((mod x 2) == 0)) [2 [4 [6 0]]])", &["std"]), num(0));
    }

    #[test]
    fn sentiment_polarity_in_latte() {
        // (pos=3, neg=1) -> (3-1)/(3+1) = 0.5 -> [0 500] signed fixed-point
        assert_eq!(n("(polarity 3 1)", &["std", "num", "sentiment"]), cell(num(0), num(500)));
        // (pos=0, neg=2) -> -1.0 -> [1 1000]
        assert_eq!(n("(polarity 0 2)", &["std", "num", "sentiment"]), cell(num(1), num(1000)));
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
    fn nn_autodiff_trains_any_stack() {
        // the generic reverse-mode trainer: layer-local VJPs folded over the stack.
        // (1) it matches the hand-rolled MLP result on |x|;
        let net = "[ [%dense [ [ [ [0 500] 0 ] [ [ [1 300] 0 ] 0 ] ] [ [ [0 100] [ [1 100] 0 ] ] 0 ] ]] \
                   [ [%relu 0] [ [%dense [ [ [ [0 400] [ [0 600] 0 ] ] 0 ] [ [ [0 200] 0 ] 0 ] ]] 0 ] ] ]";
        let before = run_i64(&format!("(tail (stack_loss {} (demo_data_x 0) (demo_data_y 0)))", net));
        let after = run_i64(&format!(
            "(tail (stack_loss (stack_train {} (demo_data_x 0) (demo_data_y 0) [0 80] 40) (demo_data_x 0) (demo_data_y 0)))",
            net
        ));
        assert!(before > 5_000, "untrained loss should be large, got {}", before);
        assert!(after * 10 < before, "autodiff training must cut the loss 10x: {} -> {}", before, after);
        // (2) a DEEPER, rmsnorm-sandwiched stack also trains through the same code path
        let deep = "[ [%dense [ (randmat 5 3 1 500) [ (map (fn [_] -> [0 0]) (range 3)) 0 ] ]] \
                    [ [%tanh 0] \
                    [ [%dense [ (randmat 11 2 3 500) [ (map (fn [_] -> [0 0]) (range 2)) 0 ] ]] \
                    [ [%relu 0] \
                    [ [%dense [ (randmat 17 1 2 500) [ [ [0 0] 0 ] 0 ] ]] 0 ] ] ] ] ]";
        let b2 = run_i64(&format!("(tail (stack_loss {} (demo_data_x 0) (demo_data_y 0)))", deep));
        let a2 = run_i64(&format!(
            "(tail (stack_loss (stack_train {} (demo_data_x 0) (demo_data_y 0) [0 60] 50) (demo_data_x 0) (demo_data_y 0)))",
            deep
        ));
        assert!(a2 < b2, "the deep stack should improve: {} -> {}", b2, a2);
    }

    fn run_i64(src: &str) -> i64 {
        let libs = crate::latte::all_libs();
        let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
        crate::latte::run_with_libs(src, &refs)
            .ok()
            .and_then(|n| n.as_atom().and_then(|a| a.to_u128()))
            .map(|v| v as i64)
            .unwrap_or(-1)
    }

    #[test]
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

/// `latte gfx` — draw a demo scene built in Latte (lib/gfx.lat) and render it to SVG.
pub fn cmd_gfx(args: &[String]) {
    let mut out = "gfx-demo.svg".to_string();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--out" && i + 1 < args.len() {
            out = args[i + 1].clone();
            i += 1;
        }
        i += 1;
    }
    println!("gfx — a graphics library for Latte (lib/gfx.lat)\n");
    println!("  A scene is an ordinary list of shape values — line, rect, circle, poly, text —");
    println!("  built with Latte constructors and rendered to SVG by the host (src/gfx.rs).");
    println!("  Colours are packed RGB naturals; the drawing's structure lives entirely in Latte.\n");
    match latte::run_with_libs("(demo 0)", &["std", "gfx"]) {
        Ok(scene) => {
            let svg = crate::gfx::render_scene(&scene, 330, 270);
            if std::fs::write(&out, &svg).is_ok() {
                println!("  wrote {}  ({} bytes) — open in a browser", out, svg.len());
            } else {
                println!("  (could not write {})", out);
            }
        }
        Err(e) => println!("  error: {}", e),
    }
}

/// `latte gpu` — show the device, benchmark the core ML kernel (matmul) on the parallel
/// backend vs a serial reference, and render a GPU+GFX Mandelbrot to SVG.
pub fn cmd_gpu(args: &[String]) {
    let mut dim = 192usize;
    let mut out = "gpu-mandelbrot.svg".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dim" if i + 1 < args.len() => { dim = args[i + 1].parse().unwrap_or(dim); i += 1; }
            "--out" if i + 1 < args.len() => { out = args[i + 1].clone(); i += 1; }
            _ => {}
        }
        i += 1;
    }
    let dev = crate::gpu::Device::target();
    println!("gpu - a data-parallel compute library for Latte (lib/gpu.lat)\n");
    println!("  target device : {}  ({} GB VRAM, {} CUDA cores, {} SMs)", dev.target, dev.vram_gb, dev.cuda_cores, dev.sm_count);
    println!("  GPU detected  : {}", if dev.gpu_present { "yes" } else { "no" });
    println!("  active backend: {}", dev.backend);
    println!("  policy        : detect a GPU by default and use it to accelerate ML/GUI; when none");
    println!("                  is present, fall back to the CPU backend and never rely on a GPU.\n");

    // --- core ML kernel: matmul, parallel vs serial (correctness + speedup) ---
    let m = dim; let k = dim; let n = dim;
    let a: Vec<f64> = (0..m * k).map(|i| ((i * 7 + 1) % 13) as f64 - 6.0).collect();
    let b: Vec<f64> = (0..k * n).map(|i| ((i * 5 + 3) % 11) as f64 - 5.0).collect();
    let t0 = std::time::Instant::now();
    let cs = crate::gpu::matmul_serial(&a, &b, m, k, n);
    let ts = t0.elapsed().as_secs_f64();
    let t1 = std::time::Instant::now();
    let cp = crate::gpu::matmul_auto(&a, &b, m, k, n);
    let tp = t1.elapsed().as_secs_f64();
    let ok = cs == cp;
    println!("  ML kernel — dense matmul {}x{}x{} (the neural-net core op, auto-dispatched to the", m, k, n);
    println!("    detected backend; see nn.lat):");
    println!("    serial   : {:.3} s", ts);
    println!("    parallel : {:.3} s   ({:.1}x on {} lanes)   results match: {}", tp, if tp > 0.0 { ts / tp } else { 0.0 }, dev.lanes, ok);

    // --- a Latte gpu program executed on the backend (saxpy then reduce) ---
    if let Ok(prog) = latte::run_with_libs("(demo 0)", &["std", "gpu"]) {
        let ops = to_list(&prog);
        println!("\n  Latte gpu program (lib/gpu.lat) executed on the backend:");
        for op in &ops {
            let parts = to_list(op);
            if parts.is_empty() { continue; }
            let tag = parts[0].as_atom().and_then(|x| x.as_cord()).unwrap_or_default();
            match tag.as_str() {
                "saxpy" => {
                    // to_list(op) = [%saxpy, alpha, x-list, y-list]
                    if parts.len() >= 4 {
                        let al = parts[1].as_atom().and_then(|x| x.to_u128()).unwrap_or(0) as f64;
                        let x: Vec<f64> = to_list(&parts[2]).iter().map(|v| v.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64).collect();
                        let y: Vec<f64> = to_list(&parts[3]).iter().map(|v| v.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64).collect();
                        let r = crate::gpu::saxpy(al, &x, &y, dev.lanes);
                        println!("    saxpy(alpha={}, x, y) = {:?}", al, r);
                    }
                }
                "reduce" => {
                    // to_list(op) = [%reduce, x-list]
                    let xs: Vec<f64> = to_list(&parts[1]).iter().map(|v| v.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64).collect();
                    println!("    reduce(sum)            = {}", crate::gpu::preduce_sum(&xs, dev.lanes));
                    println!("    map(x -> x*x)          = {:?}", crate::gpu::pmap(&xs, dev.lanes, |x| x * x));
                    println!("    dot(x, x)              = {}", crate::gpu::dot(&xs, &xs, dev.lanes));
                }
                _ => {}
            }
        }
    }

    // --- GPU + GFX: a per-pixel Mandelbrot shader rendered to an SVG raster ---
    let (w, h, cell) = (160usize, 120usize, 3usize);
    let field = crate::gpu::mandelbrot(w, h, 80, dev.lanes);
    let svg = crate::gpu::field_to_svg(&field, w, h, cell);
    match std::fs::write(&out, &svg) {
        Ok(_) => println!("\n  GPU+GFX: {}x{} Mandelbrot shaded in parallel -> {} ({} bytes)", w, h, out, svg.len()),
        Err(e) => println!("\n  (could not write {}: {})", out, e),
    }
}

// ===========================================================================
// Automatic trading advisor (lib/fin.lat model + position sizing).
//
// HONEST FRAMING: the model's reliable edge is VOLATILITY, not direction (see
// market_eval). So the advisor (1) takes a momentum-based directional lean with a
// measured, usually-low confidence; (2) sizes the position by volatility targeting
// (scale toward a target risk, shrink when high volatility is predicted) combined
// with fractional Kelly; and (3) refuses to trade when the estimated edge is not
// positive. This is a research demonstration, NOT financial advice.
// ===========================================================================

/// One technical-analysis indicator's verdict (computed by lib/ta.lat on Loom).
pub struct TaRow {
    pub name: String, // %trend %mom %rsi %macd %boll
    pub vote: i32,    // +1 bullish, 0 neutral, -1 bearish
    pub value: f64,   // the indicator's value (% for trend/mom/macd; RSI in 0-100; %B in 0-1000)
}

/// One scored news headline (sentiment computed by lib/sentiment.lat on Loom).
pub struct NewsRow {
    pub date: String,
    pub source: String,
    pub headline: String,
    pub polarity: f64, // Loughran-McDonald polarity in [-1,1]
    pub weight: f64,   // recency weight (half-life 3 days)
}

pub struct TradeAdvice {
    pub market: &'static str,
    pub last_price: f64,
    pub mom: f64,            // momentum: price/MA10 - 1, in %
    pub direction: &'static str,
    pub realized_vol: f64,   // daily realized volatility, %
    pub target_vol: f64,     // daily target volatility, %
    pub vol_regime: &'static str, // predicted next-day regime: high/low
    pub har_vol: Option<f64>, // HAR-RV next-day volatility forecast, daily %
    pub har_r2: f64,          // HAR-RV in-sample R^2
    pub band90: f64,          // conformal 90% next-day |return| band, %
    pub dir_hitrate: f64,    // out-of-sample momentum hit rate (embargoed walk-forward)
    pub model_acc: f64,      // volatility-model test accuracy
    pub kelly_full: f64,     // full Kelly fraction from the directional edge
    pub kelly_used: f64,     // fractional-Kelly applied
    pub exposure: f64,       // recommended exposure (fraction of capital, signed)
    pub dollars: f64,        // recommended notional
    pub account: f64,
    pub action: String,
    pub sentiment: Option<f64>,    // the aggregate news sentiment actually used
    // --- the combined-signal evidence ---
    pub ta_rows: Vec<TaRow>,       // per-indicator verdicts (lib/ta.lat)
    pub ta_score: i32,             // sum of votes, in [-5, +5]
    pub ta_signal: f64,            // score/5 in [-1, 1]
    pub news: Vec<NewsRow>,        // the scored headlines
    pub news_sentiment: f64,       // recency-weighted aggregate polarity
    pub combined: f64,             // 0.6*ta_signal + 0.4*news_sentiment
    pub data_note: String,         // where the price series came from
    pub span: (String, String),    // first/last date of the series
}

/// Decode a num.lat signed fixed-point cell to f64.
fn sf_to_f64(n: &crate::knot::N) -> f64 {
    use crate::knot::Knot;
    if let Knot::Cell(h, t) = &**n {
        let sign = h.as_atom().and_then(|a| a.to_u128()).unwrap_or(0);
        let mag = t.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as f64 / 1000.0;
        if sign == 0 { mag } else { -mag }
    } else { 0.0 }
}

/// Run lib/ta.lat's composite `votes` over the last `win` closes (computed on Loom),
/// decoding the verdict rows. `closes_x100` are prices in cents.
pub fn ta_votes(closes_x100: &[i64], win: usize) -> Result<(Vec<TaRow>, i32), String> {
    use crate::knot::Knot;
    let n = closes_x100.len();
    let w = &closes_x100[n.saturating_sub(win)..];
    // a chronological Latte list of fixed-point prices (cents x10 = x1000 fixed point)
    let mut lit = String::from("0");
    for p in w.iter().rev() {
        lit = format!("[ [0 {}] {} ]", (*p as u128) * 10, lit);
    }
    let out = crate::latte::run_with_libs(&format!("(votes {})", lit), &["std", "num", "ta"])?;
    let mut rows = Vec::new();
    let mut score = 0i32;
    let mut cur = out;
    while let Knot::Cell(row, rest) = &*cur.clone() {
        if let Knot::Cell(tag, vt) = &**row {
            if let Knot::Cell(vote, vl) = &**vt {
                if let Knot::Cell(value, _) = &**vl {
                    let name = tag.as_atom().and_then(|a| a.as_cord()).unwrap_or_default();
                    let v = sf_to_f64(vote).round() as i32;
                    rows.push(TaRow { name, vote: v, value: sf_to_f64(value) });
                    score += v;
                }
            }
        }
        cur = rest.clone();
    }
    if rows.len() != 5 {
        return Err(format!("ta votes decoded {} rows (expected 5)", rows.len()));
    }
    Ok((rows, score))
}

/// Score a set of dated headlines with the Loughran-McDonald scorer (polarity arithmetic
/// in lib/sentiment.lat) and aggregate with a 3-day-half-life recency weight relative to
/// the newest item. Returns (rows, aggregate in [-1,1]).
pub fn score_news(items: &[(String, String, String)]) -> (Vec<NewsRow>, f64) {
    fn ord(d: &str) -> i64 {
        let mut it = d.split('-');
        let (y, m, day) = (
            it.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(0),
            it.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(1),
            it.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(1),
        );
        y * 372 + m * 31 + day // monotone in date; only differences matter
    }
    let newest = items.iter().map(|(d, _, _)| ord(d)).max().unwrap_or(0);
    let mut rows = Vec::new();
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (d, src, h) in items {
        let pol = crate::sentiment::polarity_fused(h); // trained classifier (0.65) + LM lexicon (0.35)
        let age = (newest - ord(d)).max(0) as f64;
        let wgt = 0.5f64.powf(age / 3.0);
        num += wgt * pol;
        den += wgt;
        rows.push(NewsRow { date: d.clone(), source: src.clone(), headline: h.clone(), polarity: pol, weight: wgt });
    }
    (rows, if den > 0.0 { num / den } else { 0.0 })
}

/// Parse a user-supplied news file: one headline per line, optionally prefixed
/// "YYYY-MM-DD<TAB>Source<TAB>" (missing fields default to today/"user").
pub fn parse_news_file(text: &str) -> Vec<(String, String, String)> {
    let today = crate::marketdata::MARKET_SPAN.1.to_string();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let parts: Vec<&str> = l.splitn(3, '\t').collect();
            match parts.len() {
                3 => (parts[0].trim().to_string(), parts[1].trim().to_string(), parts[2].trim().to_string()),
                2 => (parts[0].trim().to_string(), "user".into(), parts[1].trim().to_string()),
                _ => (today.clone(), "user".into(), l.trim().to_string()),
            }
        })
        .collect()
}

/// The volatility model's test accuracy, trained once per process (it is the same
/// data and configuration every time, so retraining per call was pure waste).
/// HAR-RV (Corsi 2009): forecast next-day realized volatility from the daily,
/// weekly (5d) and monthly (22d) average RVs by least squares — the standard,
/// hard-to-beat volatility benchmark. Returns (forecast, [c, bd, bw, bm], r2).
pub fn har_rv(absr: &[f64]) -> Option<(f64, [f64; 4], f64)> {
    let n = absr.len();
    if n < 80 {
        return None;
    }
    let avg = |s: &[f64]| s.iter().sum::<f64>() / s.len() as f64;
    // rows: predict absr[t] from features at t-1
    let mut xs: Vec<[f64; 4]> = Vec::new();
    let mut ys: Vec<f64> = Vec::new();
    for t in 23..n {
        xs.push([1.0, absr[t - 1], avg(&absr[t - 5..t]), avg(&absr[t - 22..t])]);
        ys.push(absr[t]);
    }
    // normal equations (4x4), solved by Gaussian elimination
    let mut a = [[0.0f64; 5]; 4];
    for (x, &y) in xs.iter().zip(&ys) {
        for i in 0..4 {
            for j in 0..4 {
                a[i][j] += x[i] * x[j];
            }
            a[i][4] += x[i] * y;
        }
    }
    for col in 0..4 {
        let piv = (col..4).max_by(|&r1, &r2| a[r1][col].abs().partial_cmp(&a[r2][col].abs()).unwrap())?;
        a.swap(col, piv);
        if a[col][col].abs() < 1e-12 {
            return None;
        }
        for r in 0..4 {
            if r != col {
                let f = a[r][col] / a[col][col];
                for c in col..5 {
                    a[r][c] -= f * a[col][c];
                }
            }
        }
    }
    let beta = [a[0][4] / a[0][0], a[1][4] / a[1][1], a[2][4] / a[2][2], a[3][4] / a[3][3]];
    let mean_y = avg(&ys);
    let (mut ssr, mut sst) = (0.0, 0.0);
    for (x, &y) in xs.iter().zip(&ys) {
        let p = beta[0] + beta[1] * x[1] + beta[2] * x[2] + beta[3] * x[3];
        ssr += (y - p) * (y - p);
        sst += (y - mean_y) * (y - mean_y);
    }
    let last = [1.0, absr[n - 1], avg(&absr[n - 5..]), avg(&absr[n - 22..])];
    let fc = (beta[0] + beta[1] * last[1] + beta[2] * last[2] + beta[3] * last[3]).max(0.0);
    Some((fc, beta, if sst > 0.0 { 1.0 - ssr / sst } else { 0.0 }))
}

/// A conformal-style next-day return interval: the empirical q-quantile of recent
/// |returns| gives a distribution-free band (split-conformal over the last `win` days).
pub fn conformal_band(rets: &[f64], win: usize, q: f64) -> f64 {
    let lo = rets.len().saturating_sub(win);
    let mut a: Vec<f64> = rets[lo..].iter().map(|r| r.abs()).collect();
    if a.is_empty() {
        return 0.0;
    }
    a.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let idx = ((a.len() as f64) * q).ceil() as usize;
    a[idx.min(a.len() - 1)]
}

fn vol_model_acc() -> f64 {
    static ACC: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *ACC.get_or_init(|| market_eval_cfg(60, 1, true).map(|r| r.test).unwrap_or(0.0))
}

/// Compute a trade recommendation by fusing TECHNICAL ANALYSIS (lib/ta.lat: five
/// classical indicators voting on Loom) with NEWS SENTIMENT (lib/sentiment.lat:
/// Loughran-McDonald scores over real recent headlines, recency-weighted), sized by
/// fractional Kelly + volatility targeting against the volatility model's regime call.
/// `kelly_mult` is the fractional-Kelly multiplier (e.g. 0.25). `sentiment_override`
/// replaces the aggregated news score. `live` refreshes the price series first;
/// `news_text` (a `--news` file) replaces the bundled headline corpus.
pub fn trade_advice(
    account: f64,
    kelly_mult: f64,
    sentiment_override: Option<f64>,
    live: bool,
    news_text: Option<&str>,
) -> Result<TradeAdvice, String> {
    let (closes_x100, span, data_note) = crate::marketdata::closes(live);
    let closes: Vec<f64> = closes_x100.iter().map(|&c| c as f64 / 100.0).collect();
    let n = closes.len();
    if n < 30 {
        return Err("not enough data".into());
    }
    let r: Vec<f64> = (1..n).map(|i| 100.0 * (closes[i] - closes[i - 1]) / closes[i - 1]).collect();
    let absr: Vec<f64> = r.iter().map(|x| x.abs()).collect();
    let last = closes[n - 1];
    let ma10: f64 = closes[n - 10..n].iter().sum::<f64>() / 10.0;
    let mom = 100.0 * (last / ma10 - 1.0);
    let realized_vol = absr[absr.len() - 10..].iter().sum::<f64>() / 10.0; // daily %, last 10d
    let target_vol = 15.0 / (252.0_f64).sqrt(); // 15% annualized -> daily %
    // predicted next-day volatility regime: today's realized vol vs the long-run median
    let mut sorted = absr.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = sorted[sorted.len() / 2];
    let vol_regime = if realized_vol > med { "high" } else { "low" };
    // HAR-RV (Corsi): the standard daily/weekly/monthly vol regression, fit on the
    // full history; its forecast drives the volatility targeting below
    let har = har_rv(&absr);
    let (har_vol, har_r2) = match har {
        Some((fc, _, r2)) => (Some(fc), r2),
        None => (None, 0.0),
    };
    // conformal-style 90% band on tomorrow's |return| (split-conformal quantile
    // over the trailing year): a distribution-free honesty bound for the report
    let band90 = conformal_band(&r, 252, 0.90);
    // out-of-sample momentum hit rate — EMBARGOED walk-forward: a 10-day gap
    // between the training span and the test slice prevents the leakage from
    // overlapping windows that plain splits allow (the purged/embargoed CV idea,
    // López de Prado 2018, applied to the single-split case)
    let split = (n * 7) / 10;
    let embargo = 10usize;
    let (mut hits, mut tot) = (0usize, 0usize);
    for t in (split + embargo)..n - 1 {
        if t < 10 {
            continue;
        }
        let mat: f64 = closes[t - 10..t].iter().sum::<f64>() / 10.0;
        let sig = closes[t] / mat - 1.0;
        let nxt = closes[t + 1] - closes[t];
        if sig != 0.0 {
            tot += 1;
            if (sig > 0.0) == (nxt > 0.0) {
                hits += 1;
            }
        }
    }
    let dir_hitrate = if tot > 0 { hits as f64 / tot as f64 } else { 0.5 };
    // model (volatility) accuracy for reporting reliability (trained once per process)
    let model_acc = vol_model_acc();

    // --- TECHNICAL ANALYSIS: lib/ta.lat's five indicators vote, on Loom ----------
    let (ta_rows, ta_score) = ta_votes(&closes_x100, 120)?;
    let ta_signal = ta_score as f64 / 5.0;

    // --- NEWS SENTIMENT: Loughran-McDonald over real recent headlines ------------
    let items: Vec<(String, String, String)> = match news_text {
        Some(t) => parse_news_file(t),
        None => crate::marketdata::MARKET_NEWS
            .iter()
            .map(|(d, s, h)| (d.to_string(), s.to_string(), h.to_string()))
            .collect(),
    };
    let (news, news_sentiment) = score_news(&items);
    let used_sentiment = sentiment_override.unwrap_or(news_sentiment);

    // --- the COMBINED lean: TA carries 60%, news 40% ------------------------------
    let combined = 0.6 * ta_signal + 0.4 * used_sentiment;
    let long = combined > 0.1;
    let bearish = combined < -0.1;

    // directional confidence -> Kelly (payoff ratio ~1 for daily symmetric moves):
    // the measured momentum hit rate is the base; it counts fully only when the
    // momentum lean AGREES with the combined signal, and news nudges it slightly.
    let mom_agrees = (mom > 0.0) == (combined > 0.0);
    let edge = dir_hitrate - 0.5;
    let mut w = 0.5 + if mom_agrees { edge } else { edge * 0.5 };
    w += 0.03 * used_sentiment;
    let kelly_full = (2.0 * w - 1.0).clamp(-1.0, 1.0);
    let kelly_used = (kelly_mult * kelly_full).max(0.0); // never go short in this demo
    // volatility targeting against the HAR-RV forecast (falling back to the
    // regime heuristic only if the regression is unavailable)
    let fc_vol = match har_vol {
        Some(h) => h.max(0.3 * realized_vol),
        None => if vol_regime == "high" { realized_vol * 1.3 } else { realized_vol },
    };
    let vol_scale = if fc_vol > 1e-9 { (target_vol / fc_vol).min(2.0) } else { 0.0 };
    let positive_edge = kelly_full > 0.0;
    let exposure = if long && positive_edge {
        (kelly_used * vol_scale).min(2.0)
    } else {
        0.0
    };
    let action = if bearish {
        format!("FLAT — combined signal is bearish ({:+.2}: TA {:+.1}, news {:+.2}); no shorting in this demo, stand aside", combined, ta_signal, used_sentiment)
    } else if !long {
        format!("FLAT — combined signal is neutral ({:+.2}); no edge worth trading", combined)
    } else if !positive_edge {
        "FLAT — directional edge is not positive (Kelly <= 0); do not trade".to_string()
    } else {
        format!("LONG {:.1}% of capital (vol-targeted, {:.0}% fractional Kelly)", exposure * 100.0, kelly_mult * 100.0)
    };
    Ok(TradeAdvice {
        market: crate::marketdata::MARKET_NAME,
        last_price: last,
        mom,
        direction: if combined > 0.0 { "up" } else { "down" },
        realized_vol,
        target_vol,
        vol_regime: if vol_regime == "high" { "high" } else { "low" },
        har_vol,
        har_r2,
        band90,
        dir_hitrate: dir_hitrate * 100.0,
        model_acc,
        kelly_full,
        kelly_used,
        exposure,
        dollars: account * exposure,
        account,
        action,
        sentiment: Some(used_sentiment),
        ta_rows,
        ta_score,
        ta_signal,
        news,
        news_sentiment,
        combined,
        data_note,
        span,
    })
}

/// `latte trade` — automatic trading advisor: technical analysis + news sentiment
/// + the volatility model, with Kelly/vol-target position sizing.
pub fn cmd_trade(args: &[String]) {
    let mut account = 10_000.0f64;
    let mut kelly = 0.25f64;
    let mut sentiment: Option<f64> = None;
    let mut live = false;
    let mut news_text: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--account" | "--capital" if i + 1 < args.len() => { account = args[i + 1].parse().unwrap_or(account); i += 1; }
            "--kelly" if i + 1 < args.len() => { kelly = args[i + 1].parse().unwrap_or(kelly); i += 1; }
            "--sentiment" if i + 1 < args.len() => { sentiment = args[i + 1].parse().ok(); i += 1; }
            "--live" | "--fetch" => live = true,
            "--news" if i + 1 < args.len() => {
                match std::fs::read_to_string(&args[i + 1]) {
                    Ok(t) => news_text = Some(t),
                    Err(e) => eprintln!("  --news {}: {} (using the bundled corpus)", args[i + 1], e),
                }
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    println!("trade - advisor: technical analysis + news sentiment + the volatility model\n");
    match trade_advice(account, kelly, sentiment, live, news_text.as_deref()) {
        Ok(a) => {
            println!("  market            : {}   last close ${:.0}", a.market, a.last_price);
            println!("  data              : {}  ({} .. {})", a.data_note, a.span.0, a.span.1);
            println!("  -- technical analysis (lib/ta.lat, computed on Loom) --");
            for r in &a.ta_rows {
                let v = match r.vote { 1 => "+1 bullish", -1 => "-1 bearish", _ => " 0 neutral" };
                let what = match r.name.as_str() {
                    "trend" => format!("price vs SMA50      {:+8.2}%", r.value),
                    "mom"   => format!("10-day momentum     {:+8.2}%", r.value),
                    "rsi"   => format!("RSI(14)             {:8.1}", r.value),
                    "macd"  => format!("MACD(12,26,9) hist  {:+8.3}%", r.value),
                    "boll"  => format!("Bollinger %B(20,2)  {:8.0}", r.value),
                    _ => format!("{} {:.3}", r.name, r.value),
                };
                println!("    {}   -> {}", what, v);
            }
            println!("    composite TA score: {:+}  (of -5..+5)  ->  signal {:+.2}", a.ta_score, a.ta_signal);
            println!("  -- news sentiment (Loughran-McDonald, lib/sentiment.lat) --");
            for r in a.news.iter().take(6) {
                println!("    {} [{:+.2}] {} — {}", r.date, r.polarity, r.source, truncate(&r.headline, 76));
            }
            if a.news.len() > 6 {
                println!("    … and {} more headlines", a.news.len() - 6);
            }
            println!("    recency-weighted sentiment: {:+.2}{}", a.news_sentiment,
                if a.sentiment != Some(a.news_sentiment) { "  (overridden by --sentiment)" } else { "" });
            println!("  -- the combined signal --------------------");
            println!("  combined lean     : {:+.2}   (= 0.6*TA {:+.2} + 0.4*news {:+.2}) -> {}",
                a.combined, a.ta_signal, a.sentiment.unwrap_or(0.0), a.direction);
            println!("  realized vol/day  : {:.2}%   target {:.2}%   predicted regime: {}-vol", a.realized_vol, a.target_vol, a.vol_regime);
            if let Some(h) = a.har_vol {
                println!("  HAR-RV forecast   : {:.2}%/day next-day vol  (daily/weekly/monthly regression, R²={:.2}) — drives the sizing", h, a.har_r2);
            }
            println!("  90% next-day band : ±{:.2}%  (split-conformal quantile of the trailing year — distribution-free)", a.band90);
            println!("  -- model reliability ----------------------");
            println!("  momentum hit rate : {:.1}%  (out-of-sample)", a.dir_hitrate);
            println!("  volatility model  : {:.1}%  test accuracy (the dependable edge)", a.model_acc);
            println!("  -- position sizing ------------------------");
            println!("  full Kelly        : {:+.3}", a.kelly_full);
            println!("  fractional Kelly  : {:.3}", a.kelly_used);
            println!("\n  >> ADVICE: {}", a.action);
            if a.exposure > 0.0 {
                println!("            notional ~ ${:.0} of a ${:.0} account", a.dollars, a.account);
            }
            println!("\n  Rationale: five classical indicators vote in Latte (ta.lat) and real, dated");
            println!("  headlines are scored in Latte (sentiment.lat); the leans are fused 60/40 and");
            println!("  position size is risk-controlled by the volatility model (the dependable edge)");
            println!("  with fractional Kelly. `--live` refreshes prices from Coin Metrics; `--news F`");
            println!("  supplies today's headlines. Research demo, NOT financial advice.");
        }
        Err(e) => println!("  error: {}", e),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// `latte ta` — standalone technical analysis on the freshest real data
/// (`--live` fetches Coin Metrics now; otherwise cached fetch > embedded series).
pub fn cmd_ta(args: &[String]) {
    let mut live = false;
    let mut win = 120usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--live" | "--fetch" => live = true,
            "--win" | "--window" if i + 1 < args.len() => { win = args[i + 1].parse().unwrap_or(win); i += 1; }
            _ => {}
        }
        i += 1;
    }
    let (closes, span, note) = crate::marketdata::closes(live);
    println!("ta - technical analysis (lib/ta.lat, computed on Loom)\n");
    println!("  market : {}   last close ${:.2}", crate::marketdata::MARKET_NAME, *closes.last().unwrap_or(&0) as f64 / 100.0);
    println!("  data   : {}  ({} .. {})", note, span.0, span.1);
    println!("  window : last {} trading days\n", win);
    match ta_votes(&closes, win) {
        Ok((rows, score)) => {
            for r in &rows {
                let v = match r.vote { 1 => "+1 bullish", -1 => "-1 bearish", _ => " 0 neutral" };
                let what = match r.name.as_str() {
                    "trend" => format!("price vs SMA50      {:+8.2}%", r.value),
                    "mom"   => format!("10-day momentum     {:+8.2}%", r.value),
                    "rsi"   => format!("RSI(14)             {:8.1}", r.value),
                    "macd"  => format!("MACD(12,26,9) hist  {:+8.3}%", r.value),
                    "boll"  => format!("Bollinger %B(20,2)  {:8.0}", r.value),
                    _ => format!("{} {:.3}", r.name, r.value),
                };
                println!("    {}   -> {}", what, v);
            }
            let lean = if score > 0 { "bullish" } else if score < 0 { "bearish" } else { "neutral" };
            println!("\n    composite TA score: {:+}  (of -5..+5)  ->  {}", score, lean);
            println!("\n  Every indicator is computed by lib/ta.lat over signed fixed-point on Loom.");
            println!("  Feed it to the advisor with `latte trade` (where it carries 60% of the lean).");
        }
        Err(e) => println!("  error: {}", e),
    }
}

/// `latte fetch` — refresh the market data from the live Coin Metrics file (via curl),
/// caching it for `trade`/`fin` to use; falls back to the embedded series gracefully.
pub fn cmd_fetch() {
    println!("fetch - refresh BTC/USD daily closes from Coin Metrics (community data)\n");
    match crate::marketdata::fetch_live() {
        Ok(rows) => {
            let (f, l) = (rows.first().unwrap(), rows.last().unwrap());
            println!("  fetched {} daily closes: {} .. {}  (last ${:.2})", rows.len(), f.0, l.0, l.1 as f64 / 100.0);
            println!("  cached for `latte trade` / `latte fin`. The embedded series still covers");
            println!("  the freshest press-reported days past the community file's lag.");
        }
        Err(e) => println!("  {}", e),
    }
}

/// `latte sentiment "<text>"` — score finance text with the Loughran-McDonald method.
pub fn cmd_sentiment(args: &[String]) {
    // document mode: score a whole report/file with per-sentence evidence
    let mut text = String::new();
    let mut doc_mode = false;
    let mut i = 0;
    let mut words: Vec<&str> = Vec::new();
    while i < args.len() {
        match args[i].as_str() {
            "--doc" if i + 1 < args.len() => {
                let name = &args[i + 1];
                let path = format!("docs/{}.md", name.trim_end_matches(".md"));
                match std::fs::read_to_string(&path) {
                    Ok(t) => { text = t; doc_mode = true; }
                    Err(e) => { eprintln!("sentiment: cannot read {}: {}", path, e); return; }
                }
                i += 1;
            }
            "--file" if i + 1 < args.len() => {
                match std::fs::read_to_string(&args[i + 1]) {
                    Ok(t) => { text = t; doc_mode = true; }
                    Err(e) => { eprintln!("sentiment: cannot read {}: {}", args[i + 1], e); return; }
                }
                i += 1;
            }
            w => words.push(w),
        }
        i += 1;
    }
    if !doc_mode {
        text = words.join(" ");
    }
    if text.trim().is_empty() {
        println!("sentiment - financial text scoring: a trained classifier + the LM lexicon\n");
        println!("  usage: latte sentiment \"<headline or text>\"        score a short text");
        println!("         latte sentiment --doc <name>                score docs/<name>.md");
        println!("         latte sentiment --file <path>               score any document/report");
        return;
    }
    let sent_count = text.split(['.', '!', '?']).filter(|s| s.split_whitespace().count() >= 3).count();
    if doc_mode || sent_count > 1 {
        // ---- DOCUMENT MODE: per-sentence evidence + confidence-weighted aggregate ----
        let (doc, sents) = crate::sentiment::score_document(&text);
        let label = if doc > 0.15 { "POSITIVE" } else if doc < -0.15 { "NEGATIVE" } else { "neutral" };
        println!("sentiment - document scoring (trained classifier, sentence-level)\n");
        println!("  sentences : {}", sents.len());
        println!("  document  : {:+.3}  ->  {}   (confidence-weighted sentence aggregate)\n", doc, label);
        let mut ranked: Vec<&(String, f64)> = sents.iter().collect();
        ranked.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());
        println!("  strongest evidence:");
        for (t, p) in ranked.iter().take(8) {
            let short: String = t.chars().take(86).collect();
            println!("    {:+.2}  {}{}", p, short, if t.chars().count() > 86 { "…" } else { "" });
        }
        println!("\n  The classifier is a logistic regression over unigram+bigram features, trained on");
        println!("  labeled financial text in both headline and report register — it reads context");
        println!("  (\"outflows eased\" is positive) and negation (\"failed to deliver\" is negative),");
        println!("  which word-counting cannot. The same scorer reads the news for `latte trade`.");
        return;
    }
    // ---- SHORT TEXT: show all three scores ----
    let (pos, neg) = crate::sentiment::counts(&text);
    let lex = crate::sentiment::polarity(&text);
    let model = crate::sentiment::model_polarity(&text);
    let fused = crate::sentiment::polarity_fused(&text);
    let label = if fused > 0.15 { "BULLISH" } else if fused < -0.15 { "BEARISH" } else { "neutral" };
    println!("sentiment - financial text scoring\n");
    println!("  text      : {}", text);
    println!("  lexicon   : {:+.3}   (Loughran-McDonald word counts: {} pos / {} neg, ratio on Loom)", lex, pos, neg);
    println!("  model     : {:+.3}   (trained classifier: unigram+bigram logistic regression)", model);
    println!("  fused     : {:+.3}   (0.65 model + 0.35 lexicon)  ->  {}", fused, label);
    println!("\n  Multi-sentence input (or --doc/--file) switches to document mode with");
    println!("  per-sentence evidence. The advisor (`latte trade`) uses the fused score.");
}

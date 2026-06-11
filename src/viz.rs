//! Data visualization. The chart *layout* is computed in Latte (`lib/plot.lat`): data
//! values are scaled to pixel geometry. This host turns that geometry into an SVG string,
//! powers the `/api/plot` endpoint and the chart GUI page, and the `latte chart` command.

use crate::knot::{Knot, N};
use crate::latte;

const W: u128 = 320;
const H: u128 = 130;

/// Render a chart (`"bar"`, `"line"`, or `"scatter"`) of `values` as a standalone SVG.
pub fn render_chart(kind: &str, values: &[u128]) -> String {
    if values.is_empty() {
        return svg_wrap("<text x='12' y='24' font-family='monospace' font-size='12'>no data</text>".into());
    }
    let inner = match kind {
        "line" => match geom("points", values) {
            Ok(g) => {
                let pts = rows(&g);
                let poly: Vec<String> = pts.iter().filter(|p| p.len() >= 2).map(|p| format!("{},{}", p[0], p[1])).collect();
                let dots: String = pts.iter().filter(|p| p.len() >= 2).map(|p| format!("<circle cx='{}' cy='{}' r='3' fill='#3a3f7a'/>", p[0], p[1])).collect();
                format!("<polyline points='{}' fill='none' stroke='#3a3f7a' stroke-width='2'/>{}", poly.join(" "), dots)
            }
            Err(e) => err_text(&e),
        },
        "scatter" => match geom("points", values) {
            Ok(g) => rows(&g).iter().filter(|p| p.len() >= 2).map(|p| format!("<circle cx='{}' cy='{}' r='4' fill='#3a3f7a'/>", p[0], p[1])).collect(),
            Err(e) => err_text(&e),
        },
        _ => match geom("bars", values) {
            Ok(g) => rows(&g)
                .iter()
                .filter(|r| r.len() >= 4)
                .map(|r| {
                    let w = r[2].saturating_sub(2).max(1);
                    format!("<rect x='{}' y='{}' width='{}' height='{}' fill='#3a3f7a'/>", r[0] + 1, r[1], w, r[3])
                })
                .collect(),
            Err(e) => err_text(&e),
        },
    };
    svg_wrap(inner)
}

fn svg_wrap(inner: String) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' width='{w}' height='{h}'>\
<rect x='0' y='0' width='{w}' height='{h}' fill='#f4f1e8'/>\
{inner}\
<line x1='0' y1='{base}' x2='{w}' y2='{base}' stroke='#1a1a2e' stroke-width='1'/>\
</svg>",
        w = W,
        h = H + 12,
        base = H,
        inner = inner
    )
}

fn err_text(e: &str) -> String {
    format!("<text x='12' y='24' font-family='monospace' font-size='11' fill='#b00020'>{}</text>", e)
}

/// Run a `plot` layout function over the values, returning the geometry noun.
fn geom(func: &str, values: &[u128]) -> Result<N, String> {
    let expr = format!("({} {} {} {})", func, build_list(values), W, H);
    latte::run_with_libs(&expr, &["std", "plot"])
}

/// Build a 0-terminated Latte list literal from naturals: `[1 [2 [3 0]]]`.
fn build_list(values: &[u128]) -> String {
    let mut out = String::from("0");
    for v in values.iter().rev() {
        out = format!("[{} {}]", v, out);
    }
    out
}

/// Flatten a 0-terminated cons list of atoms into a Vec.
fn nat_list(n: &N) -> Vec<u128> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        if let Some(a) = h.as_atom().and_then(|a| a.to_u128()) {
            out.push(a);
        }
        cur = t.clone();
    }
    out
}

/// A list of rows, each a list of naturals.
fn rows(n: &N) -> Vec<Vec<u128>> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        out.push(nat_list(h));
        cur = t.clone();
    }
    out
}

/// Render several labelled series as a multi-line chart with axes, a legend and a
/// title — the kind of plot machine-learning work needs (loss curves, an equity
/// curve against a benchmark, train-vs-test metrics). Series share one y-scale.
pub fn render_lines(title: &str, labels: &[&str], series: &[Vec<f64>]) -> String {
    render_lines_dated(title, labels, series, None)
}

/// Like `render_lines`, with optional per-point x-axis date labels: ~5 ticks are
/// drawn along the axis (with light vertical gridlines) instead of "step 0 → n".
pub fn render_lines_dated(title: &str, labels: &[&str], series: &[Vec<f64>], dates: Option<&[String]>) -> String {
    let cw = 640.0;
    let ch = 360.0;
    let (ml, mr, mt, mb) = (54.0, 130.0, 34.0, 30.0); // margins (right margin holds legend)
    let pw = cw - ml - mr;
    let ph = ch - mt - mb;
    let palette = ["#3a3f7a", "#b5651d", "#2e7d4f", "#8a2846", "#5a5a5a"];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut maxlen = 0usize;
    for s in series {
        maxlen = maxlen.max(s.len());
        for &v in s {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return svg_wrap("<text x='12' y='24' font-family='monospace' font-size='12'>no data</text>".into());
    }
    if (hi - lo).abs() < 1e-9 {
        hi = lo + 1.0;
    }
    let xat = |i: usize, n: usize| ml + if n <= 1 { 0.0 } else { pw * (i as f64) / ((n - 1) as f64) };
    let yat = |v: f64| mt + ph * (1.0 - (v - lo) / (hi - lo));
    let mut body = String::new();
    // plot frame + a few horizontal gridlines with value labels
    body.push_str(&format!(
        "<rect x='{ml}' y='{mt}' width='{pw}' height='{ph}' fill='#ffffff' stroke='#c8c2b0'/>"
    ));
    for g in 0..=4 {
        let v = lo + (hi - lo) * (g as f64) / 4.0;
        let y = yat(v);
        body.push_str(&format!(
            "<line x1='{ml}' y1='{y:.1}' x2='{:.1}' y2='{y:.1}' stroke='#eceadf'/>\
<text x='{:.1}' y='{:.1}' font-family='monospace' font-size='10' fill='#555' text-anchor='end'>{v:.3}</text>",
            ml + pw, ml - 6.0, y + 3.0
        ));
    }
    // each series as a polyline, plus a legend entry
    for (si, s) in series.iter().enumerate() {
        let col = palette[si % palette.len()];
        let pts: Vec<String> = s.iter().enumerate().map(|(i, &v)| format!("{:.1},{:.1}", xat(i, s.len()), yat(v))).collect();
        body.push_str(&format!("<polyline points='{}' fill='none' stroke='{col}' stroke-width='2'/>", pts.join(" ")));
        let ly = mt + 6.0 + (si as f64) * 18.0;
        let lx = ml + pw + 14.0;
        let lab = labels.get(si).copied().unwrap_or("series");
        body.push_str(&format!(
            "<line x1='{lx}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='{col}' stroke-width='3'/>\
<text x='{:.1}' y='{:.1}' font-family='monospace' font-size='11' fill='#222'>{lab}</text>",
            ly, lx + 22.0, ly, lx + 28.0, ly + 4.0
        ));
    }
    body.push_str(&format!(
        "<text x='{ml}' y='20' font-family='monospace' font-size='13' fill='#1a1a2e'>{title}</text>"
    ));
    match dates {
        Some(ds) if !ds.is_empty() => {
            // ~5 date ticks with light vertical gridlines
            let nt = 5usize.min(ds.len());
            for k in 0..nt {
                let i = if nt == 1 { 0 } else { k * (ds.len() - 1) / (nt - 1) };
                let x = xat(i, ds.len());
                body.push_str(&format!(
                    "<line x1='{x:.1}' y1='{mt}' x2='{x:.1}' y2='{:.1}' stroke='#eceadf'/>\
<text x='{x:.1}' y='{:.1}' font-family='monospace' font-size='9' fill='#555' text-anchor='middle'>{}</text>",
                    mt + ph, ch - 10.0, ds[i]
                ));
            }
        }
        _ => {
            body.push_str(&format!(
                "<text x='{:.1}' y='{:.1}' font-family='monospace' font-size='10' fill='#555' text-anchor='middle'>step {} → {}</text>",
                ml + pw / 2.0, ch - 10.0, 0, maxlen.saturating_sub(1)
            ));
        }
    }
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {cw} {ch}' width='{cw}' height='{ch}'>\
<rect x='0' y='0' width='{cw}' height='{ch}' fill='#f4f1e8'/>{body}</svg>"
    )
}

/// Chart the (real, optionally live-refreshed) market series: daily closes with
/// SMA20 and SMA50 overlays over the last `days` days.
pub fn market_chart(days: usize, live: bool) -> String {
    market_chart_sym("btc", days, live)
}
pub fn market_chart_sym(market: &str, days: usize, live: bool) -> String {
    let (closes_x100, span, note) = match crate::marketdata::closes_market(market, live) {
        Ok(t) => t,
        Err(e) => {
            return format!("<svg xmlns='http://www.w3.org/2000/svg' width='460' height='40'><text x='6' y='24' font-family='monospace' font-size='12'>{}</text></svg>", e);
        }
    };
    let all: Vec<f64> = closes_x100.iter().map(|&c| c as f64 / 100.0).collect();
    let sma = |w: usize| -> Vec<f64> {
        (0..all.len())
            .map(|i| {
                let lo = i.saturating_sub(w - 1);
                let win = &all[lo..=i];
                win.iter().sum::<f64>() / win.len() as f64
            })
            .collect()
    };
    let (s20, s50) = (sma(20), sma(50));
    let n = all.len();
    let lo = n.saturating_sub(days);
    // real calendar dates for the x-axis: the series ends on span.1, one row per day
    let dates: Option<Vec<String>> = crate::marketdata::date_ordinal(&span.1).map(|end| {
        (lo..n).map(|i| crate::marketdata::ordinal_date(end - (n - 1 - i) as i64)).collect()
    });
    let title = format!(
        "{} — last {} days to {}  ({})",
        crate::marketdata::market_label(market),
        n - lo,
        span.1,
        note
    );
    render_lines_dated(
        &title,
        &["close", "SMA20", "SMA50"],
        &[all[lo..].to_vec(), s20[lo..].to_vec(), s50[lo..].to_vec()],
        dates.as_deref(),
    )
}

/// Decode `render`'s rows-of-packed-RGB noun into a flat u32 field.
fn trace_field(n: &crate::knot::N) -> (Vec<u32>, usize, usize) {
    use crate::knot::Knot;
    let mut field = Vec::new();
    let (mut w, mut h) = (0usize, 0usize);
    let mut rows = n.clone();
    while let Knot::Cell(row, rest) = &*rows.clone() {
        let mut rw = 0usize;
        let mut cur = row.clone();
        while let Knot::Cell(px, t) = &*cur.clone() {
            let v = px.as_atom().and_then(|a| a.to_u128()).unwrap_or(0) as u32;
            field.push(0xFF00_0000 | v);
            rw += 1;
            cur = t.clone();
        }
        w = w.max(rw);
        h += 1;
        rows = rest.clone();
    }
    (field, w, h)
}

/// Run the Latte ray tracer (lib/trace.lat) at `w`x`h` and return (SVG, engine, ms).
/// Tries the Anvil native compiler first; falls back to the (jet-accelerated) interpreter.
pub fn ray_trace(w: usize, h: usize) -> (String, &'static str, u128) {
    ray_trace_scene(None, w, h)
}

/// Render an ARBITRARY scene: `scene` is a Latte expression evaluating to a
/// sphere list `[ [center [radius [color [reflectivity 0]]]] … 0 ]` (centers
/// and colors via `v3`, reflectivity 0..1000). The scene is data, so any tool
/// — or any text in the GUI — can construct one and hand it to the tracer.
pub fn ray_trace_scene(scene: Option<&str>, w: usize, h: usize) -> (String, &'static str, u128) {
    let expr = match scene {
        Some(sc) => format!("(rendersc {} {} {})", sc.trim(), w, h),
        None => format!("(render {} {})", w, h),
    };
    let libs: Vec<String> = crate::latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    let t0 = std::time::Instant::now();
    let (noun, engine) = match crate::rustgen::run_native_noun_opts(&expr, &refs, false) {
        Some(n) => (Some(n), "Anvil (compiled to native code)"),
        None => (
            crate::latte::run_with_libs(&expr, &refs).ok(),
            "Loom interpreter + audited vector jets",
        ),
    };
    let ms = t0.elapsed().as_millis();
    match noun {
        Some(n) => {
            let (field, fw, fh) = trace_field(&n);
            let cell = (520 / fw.max(1)).clamp(2, 8);
            (crate::gpu::field_to_svg(&field, fw, fh, cell), engine, ms)
        }
        None => ("<svg xmlns='http://www.w3.org/2000/svg' width='320' height='40'><text x='6' y='22'>trace failed</text></svg>".into(), engine, ms),
    }
}

/// `latte trace [--w N] [--h N]` — render the Latte ray tracer to SVG on stdout.
pub fn cmd_trace(args: &[String]) {
    let mut w = 96usize;
    let mut h = 72usize;
    let mut scene: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--w" | "--width" if i + 1 < args.len() => { w = args[i + 1].parse().unwrap_or(w); i += 1; }
            "--h" | "--height" if i + 1 < args.len() => { h = args[i + 1].parse().unwrap_or(h); i += 1; }
            "--scene" if i + 1 < args.len() => {
                // a Latte scene expression, or a file containing one
                let a = &args[i + 1];
                scene = Some(std::fs::read_to_string(a).unwrap_or_else(|_| a.clone()));
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let (svg, engine, ms) = ray_trace_scene(scene.as_deref(), w.clamp(16, 480), h.clamp(12, 360));
    println!("{}", svg);
    eprintln!("\nray-traced {}x{} in {} ms — {} — scene, shading and camera all in lib/trace.lat", w, h, ms, engine);
}

pub fn cmd_chart(args: &[String]) {
    // `latte chart market [--live] [--days N]`: the real price series with SMA overlays
    if args.first().map(|s| s.as_str()) == Some("market") {
        let mut live = false;
        let mut days = 180usize;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--live" | "--fetch" => live = true,
                "--days" if i + 1 < args.len() => { days = args[i + 1].parse().unwrap_or(days); i += 1; }
                _ => {}
            }
            i += 1;
        }
        println!("{}", market_chart(days, live));
        eprintln!("\n(redirect stdout to a .svg file to view, or use the chart GUI at /chart)");
        return;
    }
    let kinds = ["bar", "line", "scatter"];
    let (kind, nums): (&str, &[String]) = match args.first() {
        Some(a) if kinds.contains(&a.as_str()) => (a.as_str(), &args[1..]),
        _ => ("bar", args),
    };
    let mut values: Vec<u128> = nums.iter().filter_map(|s| s.parse().ok()).collect();
    if values.is_empty() {
        values = vec![3, 1, 4, 1, 5, 9, 2, 6];
        eprintln!("chart: no data given; using demo data 3 1 4 1 5 9 2 6");
    }
    println!("{}", render_chart(kind, &values));
    eprintln!("\n(redirect stdout to a .svg file to view, or use the chart GUI at /chart)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_chart_has_rects_scaled_to_max() {
        let svg = render_chart("bar", &[1, 2, 4]);
        assert!(svg.starts_with("<svg"));
        // tallest bar (value 4) reaches the full height H; shortest (1) is a quarter
        assert!(svg.contains("height='130'"), "max bar should fill H: {}", svg);
        assert_eq!(svg.matches("<rect").count(), 4); // background + 3 bars
    }

    #[test]
    fn line_chart_has_polyline() {
        let svg = render_chart("line", &[1, 2, 3]);
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn multiline_chart_has_series_and_legend() {
        let svg = render_lines("loss", &["a", "b"], &[vec![3.0, 2.0, 1.0], vec![1.0, 1.0, 1.0]]);
        assert!(svg.starts_with("<svg"));
        assert_eq!(svg.matches("<polyline").count(), 2, "one polyline per series");
        assert!(svg.contains(">a<") && svg.contains(">b<"), "legend labels present");
    }
}

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

pub fn cmd_chart(args: &[String]) {
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
}

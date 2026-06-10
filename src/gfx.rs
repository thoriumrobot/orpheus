//! A small graphics library for Latte. A *scene* is built in Latte (`lib/gfx.lat`) as an
//! ordinary list of shape values — `[%line …]`, `[%rect …]`, `[%circle …]`, `[%poly …]`,
//! `[%text …]` — and this host walks that noun and serializes it to an SVG image. This is
//! the same "compute the structure in Latte, render in the host" split that `viz.rs` uses
//! for charts, generalized to free-form drawing.

use crate::knot::{Knot, N};

/// Read a natural from a noun (0 if it is not an atom).
fn natof(n: &N) -> u128 {
    n.as_atom().and_then(|a| a.to_u128()).unwrap_or(0)
}

/// Flatten a 0-terminated cons list of atoms into a Vec of naturals.
fn nat_list(n: &N) -> Vec<u128> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        out.push(natof(h));
        cur = t.clone();
    }
    out
}

/// A list of rows, each itself a list of naturals (used for polyline points).
fn rows(n: &N) -> Vec<Vec<u128>> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        out.push(nat_list(h));
        cur = t.clone();
    }
    out
}

/// Turn a packed RGB natural (r*65536 + g*256 + b) into a CSS hex colour.
fn color(c: u128) -> String {
    let r = (c >> 16) & 0xff;
    let g = (c >> 8) & 0xff;
    let b = c & 0xff;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Walk a scene noun (a list of `[tag payload]` shapes) and render it as a standalone SVG.
pub fn render_scene(scene: &N, w: u128, h: u128) -> String {
    let body = render_scene_body(scene);
    body
}

/// Render the shapes of a scene to SVG elements (no outer <svg> wrapper).
fn render_scene_body(scene: &N) -> String {
    let mut body = String::new();
    let mut cur = scene.clone();
    while let Knot::Cell(shape, rest) = &*cur {
        if let Knot::Cell(tag, payload) = &**shape {
            let t = tag.as_atom().and_then(|a| a.as_cord()).unwrap_or_default();
            match t.as_str() {
                "line" => {
                    let p = nat_list(payload);
                    if p.len() >= 5 {
                        body += &format!(
                            "<line x1='{}' y1='{}' x2='{}' y2='{}' stroke='{}' stroke-width='2'/>",
                            p[0], p[1], p[2], p[3], color(p[4])
                        );
                    }
                }
                "rect" => {
                    let p = nat_list(payload);
                    if p.len() >= 5 {
                        body += &format!(
                            "<rect x='{}' y='{}' width='{}' height='{}' rx='2' fill='{}'/>",
                            p[0], p[1], p[2], p[3], color(p[4])
                        );
                    }
                }
                "circle" => {
                    let p = nat_list(payload);
                    if p.len() >= 4 {
                        body += &format!(
                            "<circle cx='{}' cy='{}' r='{}' fill='{}'/>",
                            p[0], p[1], p[2], color(p[3])
                        );
                    }
                }
                "poly" => {
                    if let Knot::Cell(pts, prest) = &**payload {
                        let pstr: Vec<String> = rows(pts)
                            .iter()
                            .filter(|r| r.len() >= 2)
                            .map(|r| format!("{},{}", r[0], r[1]))
                            .collect();
                        let c = if let Knot::Cell(ch, _) = &**prest { natof(ch) } else { 0 };
                        body += &format!(
                            "<polygon points='{}' fill='{}' stroke='#222' stroke-width='1'/>",
                            pstr.join(" "), color(c)
                        );
                    }
                }
                "ellipse" => {
                    // payload = [ x y rx ry c ]
                    let p = nat_list(payload);
                    if p.len() >= 5 {
                        body += &format!(
                            "<ellipse cx='{}' cy='{}' rx='{}' ry='{}' fill='{}'/>",
                            p[0], p[1], p[2], p[3], color(p[4])
                        );
                    }
                }
                "pline" => {
                    // an OPEN polyline: payload = [ pts w c ] (stroke width w)
                    if let Knot::Cell(pts, prest) = &**payload {
                        let pstr: Vec<String> = rows(pts)
                            .iter()
                            .filter(|r| r.len() >= 2)
                            .map(|r| format!("{},{}", r[0], r[1]))
                            .collect();
                        let rest_v = nat_list(prest);
                        let w = rest_v.first().copied().unwrap_or(2).max(1);
                        let c = rest_v.get(1).copied().unwrap_or(0);
                        body += &format!(
                            "<polyline points='{}' fill='none' stroke='{}' stroke-width='{}' stroke-linejoin='round' stroke-linecap='round'/>",
                            pstr.join(" "), color(c), w
                        );
                    }
                }
                "ring" => {
                    // an unfilled circle: payload = [ x y r w c ]
                    let p = nat_list(payload);
                    if p.len() >= 5 {
                        body += &format!(
                            "<circle cx='{}' cy='{}' r='{}' fill='none' stroke='{}' stroke-width='{}'/>",
                            p[0], p[1], p[2], color(p[4]), p[3].max(1)
                        );
                    }
                }
                "fade" => {
                    // translucency wrapper: payload = [ alpha(0..100) shape ] — renders the
                    // inner shape inside a group with opacity alpha/100.
                    if let Knot::Cell(ah, sh) = &**payload {
                        let a = natof(ah).min(100);
                        if let Knot::Cell(inner, _) = &**sh {
                            let one = crate::knot::cell(inner.clone(), crate::knot::num(0));
                            let sub = render_scene_body(&one);
                            body += &format!("<g opacity='{}.{:02}'>{}</g>", a / 100, a % 100, sub);
                        }
                    }
                }
                "text" => {
                    // payload = [ x y s c ]
                    if let Knot::Cell(xh, r1) = &**payload {
                        if let Knot::Cell(yh, r2) = &**r1 {
                            if let Knot::Cell(sh, r3) = &**r2 {
                                let s = sh.as_atom().and_then(|a| a.as_cord()).unwrap_or_default();
                                let c = if let Knot::Cell(ch, _) = &**r3 { natof(ch) } else { 0 };
                                body += &format!(
                                    "<text x='{}' y='{}' font-family='monospace' font-size='13' fill='{}'>{}</text>",
                                    natof(xh), natof(yh), color(c), escape(&s)
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        cur = rest.clone();
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latte;

    #[test]
    fn gfx_demo_scene_renders_all_primitives() {
        let scene = latte::run_with_libs("(demo 0)", &["std", "gfx"]).expect("demo scene");
        let svg = render_scene(&scene, 320, 260);
        assert!(svg.contains("<rect"), "has a rectangle");
        assert!(svg.contains("<circle"), "has a circle");
        assert!(svg.contains("<line"), "has a line");
        assert!(svg.contains("<polygon"), "has a polygon");
        assert!(svg.contains("<text"), "has text");
        assert!(svg.contains("#"), "uses hex colours");
    }

    #[test]
    fn gfx_rgb_packs_into_hex() {
        // rgb 200 40 40 = 200*65536 + 40*256 + 40 = 13117480
        let scene = latte::run_with_libs("[ (rect 0 0 10 10 (rgb 200 40 40)) 0 ]", &["std", "gfx"])
            .expect("one-rect scene");
        let svg = render_scene(&scene, 20, 20);
        assert!(svg.contains("#c82828"), "200,40,40 -> #c82828; got {}", svg);
    }
}

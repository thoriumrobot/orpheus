//! Facet — a markup language for the Mocha environment.
//!
//! A Facet document is plain markup (HTML passes through) with two kinds of holes,
//! both delimited by braces:
//!
//!   {{ EXPR }}                evaluate a Facet expression, insert its text
//!   {{each NAME in EXPR}} .. {{end}}   repeat the enclosed template over a list
//!
//! Facet expressions are functional and Latte-flavored: string/number literals,
//! lists `[a, b, c]`, `let NAME = EXPR in EXPR`, variables, and — the key idea —
//! **tool calls written `Module.procedure(args)`**, exactly the Oberon
//! `Module.Procedure` convention the rest of Mocha uses. Any environment tool is
//! reachable this way; the sound-change applier is `SCArs.evolve("zelā")`. Expressions
//! are pure and side-effect-free, so a page is a deterministic function of its tool
//! outputs — the same design philosophy as Latte itself.

use crate::sca;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ------------------------------- AST ----------------------------------------
#[derive(Debug, Clone)]
pub enum Expr {
    Str(String),
    Num(u128),
    Var(String),
    List(Vec<Expr>),
    Call(String, String, Vec<Expr>), // Module.proc(args)
    Let(String, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Seg {
    Text(String),
    Hole(Expr),
    Each(String, Expr, Vec<Seg>),  // each NAME in EXPR -> template
    If(Expr, Vec<Seg>, Vec<Seg>),  // if EXPR -> then-template [else-template]
}

#[derive(Debug, Clone)]
pub enum Val {
    Text(String),
    Num(u128),
    List(Vec<Val>),
}

impl Val {
    pub fn to_text(&self) -> String {
        match self {
            Val::Text(s) => s.clone(),
            Val::Num(n) => n.to_string(),
            Val::List(vs) => vs.iter().map(|v| v.to_text()).collect::<Vec<_>>().join(""),
        }
    }
}

// ------------------------------- document parser ----------------------------
/// Why a run of segments stopped: end of input, a `{{end}}`, or a `{{else}}`.
enum Stop {
    Eof,
    End,
    Else,
}

/// Parse a whole document into segments.
pub fn parse_document(src: &str) -> Result<Vec<Seg>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut pos = 0;
    let (segs, stop) = parse_segments(&chars, &mut pos)?;
    match stop {
        Stop::Eof => Ok(segs),
        Stop::End => Err("unexpected {{end}} with no matching {{each}}/{{if}}".into()),
        Stop::Else => Err("unexpected {{else}} with no matching {{if}}".into()),
    }
}

/// Parse segments until EOF, a closing `{{end}}`, or an `{{else}}` (returns why it stopped).
fn parse_segments(chars: &[char], pos: &mut usize) -> Result<(Vec<Seg>, Stop), String> {
    let mut segs = Vec::new();
    let mut text = String::new();
    while *pos < chars.len() {
        if chars[*pos] == '{' && *pos + 1 < chars.len() && chars[*pos + 1] == '{' {
            if !text.is_empty() {
                segs.push(Seg::Text(std::mem::take(&mut text)));
            }
            *pos += 2; // past '{{'
            let directive = read_until_braces(chars, pos)?; // consumes the '}}'
            let d = directive.trim();
            if d == "end" {
                return Ok((segs, Stop::End));
            } else if d == "else" {
                return Ok((segs, Stop::Else));
            } else if let Some(rest) = d.strip_prefix("each ") {
                let (name, list_expr) = parse_each_header(rest)?;
                let (body, stop) = parse_segments(chars, pos)?;
                if !matches!(stop, Stop::End) {
                    return Err(format!("{{{{each {} ...}}}} missing {{{{end}}}}", name));
                }
                segs.push(Seg::Each(name, list_expr, body));
            } else if let Some(rest) = d.strip_prefix("if ") {
                let cond = parse_expr_str(rest.trim())?;
                let (then_body, stop) = parse_segments(chars, pos)?;
                let else_body = match stop {
                    Stop::Else => {
                        let (eb, stop2) = parse_segments(chars, pos)?;
                        if !matches!(stop2, Stop::End) {
                            return Err("{{if ...}} / {{else}} missing {{end}}".into());
                        }
                        eb
                    }
                    Stop::End => Vec::new(),
                    Stop::Eof => return Err("{{if ...}} missing {{end}}".into()),
                };
                segs.push(Seg::If(cond, then_body, else_body));
            } else {
                let expr = parse_expr_str(d)?;
                segs.push(Seg::Hole(expr));
            }
        } else {
            text.push(chars[*pos]);
            *pos += 1;
        }
    }
    if !text.is_empty() {
        segs.push(Seg::Text(text));
    }
    Ok((segs, Stop::Eof))
}

fn read_until_braces(chars: &[char], pos: &mut usize) -> Result<String, String> {
    let mut s = String::new();
    while *pos < chars.len() {
        if chars[*pos] == '}' && *pos + 1 < chars.len() && chars[*pos + 1] == '}' {
            *pos += 2;
            return Ok(s);
        }
        s.push(chars[*pos]);
        *pos += 1;
    }
    Err("unterminated '{{' (missing '}}')".into())
}

fn parse_each_header(rest: &str) -> Result<(String, Expr), String> {
    // NAME in EXPR
    let (name, expr_s) = rest
        .split_once(" in ")
        .ok_or_else(|| "each: expected 'each NAME in EXPR'".to_string())?;
    Ok((name.trim().to_string(), parse_expr_str(expr_s.trim())?))
}

// ------------------------------- expression parser --------------------------
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(u128),
    Dot,
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Eq,
}

fn lex_expr(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' {
            i += 1;
            let mut buf = String::new();
            while i < cs.len() && cs[i] != '"' {
                if cs[i] == '\\' && i + 1 < cs.len() {
                    // escapes let a string contain a quote — needed for expressions that embed
                    // string literals, e.g. Live.box("Txt.split(s, \";\")", …)
                    i += 1;
                    buf.push(match cs[i] {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other, // \" \\ and any other escaped char → the char itself
                    });
                } else {
                    buf.push(cs[i]);
                }
                i += 1;
            }
            if i >= cs.len() {
                return Err("unterminated string".into());
            }
            i += 1; // past closing quote
            out.push(Tok::Str(buf));
        } else if c == '.' {
            out.push(Tok::Dot);
            i += 1;
        } else if c == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else if c == '[' {
            out.push(Tok::LBrack);
            i += 1;
        } else if c == ']' {
            out.push(Tok::RBrack);
            i += 1;
        } else if c == ',' {
            out.push(Tok::Comma);
            i += 1;
        } else if c == '=' {
            out.push(Tok::Eq);
            i += 1;
        } else if c.is_ascii_digit() {
            let mut n = 0u128;
            while i < cs.len() && cs[i].is_ascii_digit() {
                n = n * 10 + (cs[i] as u128 - '0' as u128);
                i += 1;
            }
            out.push(Tok::Num(n));
        } else if c.is_alphabetic() || c == '_' {
            let mut buf = String::new();
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                buf.push(cs[i]);
                i += 1;
            }
            out.push(Tok::Ident(buf));
        } else {
            return Err(format!("unexpected character '{}' in expression", c));
        }
    }
    Ok(out)
}

struct EP {
    toks: Vec<Tok>,
    pos: usize,
}
impl EP {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn expr(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == "let") {
            self.next();
            let name = match self.next() {
                Some(Tok::Ident(s)) => s,
                _ => return Err("let: expected name".into()),
            };
            match self.next() {
                Some(Tok::Eq) => {}
                _ => return Err("let: expected '='".into()),
            }
            let val = self.expr()?;
            match self.next() {
                Some(Tok::Ident(s)) if s == "in" => {}
                _ => return Err("let: expected 'in'".into()),
            }
            let body = self.expr()?;
            return Ok(Expr::Let(name, Box::new(val), Box::new(body)));
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LBrack) => {
                let mut items = Vec::new();
                if !matches!(self.peek(), Some(Tok::RBrack)) {
                    loop {
                        items.push(self.expr()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RBrack) => break,
                            other => return Err(format!("list: expected ',' or ']', got {:?}", other)),
                        }
                    }
                } else {
                    self.next();
                }
                Ok(Expr::List(items))
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::Dot)) {
                    // Module.proc(args)
                    self.next();
                    let proc = match self.next() {
                        Some(Tok::Ident(p)) => p,
                        _ => return Err("expected procedure name after '.'".into()),
                    };
                    match self.next() {
                        Some(Tok::LParen) => {}
                        _ => return Err(format!("call {}.{}: expected '('", name, proc)),
                    }
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.expr()?);
                            match self.next() {
                                Some(Tok::Comma) => continue,
                                Some(Tok::RParen) => break,
                                other => return Err(format!("args: expected ',' or ')', got {:?}", other)),
                            }
                        }
                    } else {
                        self.next();
                    }
                    Ok(Expr::Call(name, proc, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(format!("unexpected token {:?} in expression", other)),
        }
    }
}

fn parse_expr_str(s: &str) -> Result<Expr, String> {
    let toks = lex_expr(s)?;
    let mut p = EP { toks, pos: 0 };
    let e = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(format!("trailing tokens in expression '{}'", s));
    }
    Ok(e)
}

// ------------------------------- evaluation ---------------------------------
type Env = Vec<(String, Val)>;

fn eval(e: &Expr, env: &Env) -> Result<Val, String> {
    match e {
        Expr::Str(s) => Ok(Val::Text(s.clone())),
        Expr::Num(n) => Ok(Val::Num(*n)),
        Expr::Var(name) => env
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| format!("unbound variable '{}'", name)),
        Expr::List(items) => {
            let mut vs = Vec::new();
            for it in items {
                vs.push(eval(it, env)?);
            }
            Ok(Val::List(vs))
        }
        Expr::Let(name, val, body) => {
            let v = eval(val, env)?;
            let mut env2 = env.clone();
            env2.push((name.clone(), v));
            eval(body, &env2)
        }
        Expr::Call(module, proc, args) => {
            let mut av = Vec::new();
            for a in args {
                av.push(eval(a, env)?);
            }
            // `In.field(name, default)` reads a user-supplied input (a query parameter seeded into
            // the env by `render_with`) or falls back to `default` — the seam that lets a Facet
            // page accept input and become an interactive interface, not just a static demo.
            if module == "In" && proc == "field" {
                let name = av.first().map(|v| v.to_text()).unwrap_or_default();
                let default = av.get(1).cloned().unwrap_or(Val::Text(String::new()));
                return Ok(env
                    .iter()
                    .rev()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(default));
            }
            dispatch_tool(module, proc, &av)
        }
    }
}

/// Intra-environment tool dispatch. This is the single seam through which Facet
/// pages reach every tool in the system; new tools are added here.
// ---- the tool registry -----------------------------------------------------
// One table is the single source of truth for every tool the environment exposes to a Facet
// page: its handler, its signature, and a one-line summary. `dispatch_tool` routes through it, and
// the tools-page test iterates it to guarantee EVERY tool has a live interface in its own frame —
// so a tool can never ship without a documented, exercised interface.
pub struct ToolSpec {
    pub module: &'static str,
    pub proc: &'static str,
    pub sig: &'static str,
    pub summary: &'static str,
    handler: fn(&[Val]) -> Result<Val, String>,
}

fn tool_scars_evolve(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(sca::evolve(&arg_text(args, 0)?)?))
}
fn tool_scars_pie(args: &[Val]) -> Result<Val, String> {
    // the earlier evolution stage: Proto-Indo-European -> Solar Speech
    Ok(Val::Text(sca::pie_to_solar(&arg_text(args, 0)?)?))
}
/// Pull a rule list from arg `idx`: either a single list (from a form field) or rest-args.
fn rules_from(args: &[Val], idx: usize) -> Vec<String> {
    match args.get(idx) {
        Some(Val::List(vs)) => vs.iter().map(|v| v.to_text()).filter(|s| !s.is_empty()).collect(),
        _ => args
            .get(idx..)
            .unwrap_or(&[])
            .iter()
            .map(|v| v.to_text())
            .filter(|s| !s.is_empty())
            .collect(),
    }
}
fn tool_scars_trace(args: &[Val]) -> Result<Val, String> {
    // show the word after each successive rule: w → r0(w) → r1(r0(w)) → …
    let w = arg_text(args, 0)?;
    let rules = rules_from(args, 1);
    let mut path = vec![w.clone()];
    let mut acc: Vec<String> = Vec::new();
    for r in &rules {
        acc.push(r.clone());
        path.push(sca::run_sca(&w, &acc)?);
    }
    Ok(Val::Text(path.join(" → ")))
}

// ------------------------------- Phono: phonology ---------------------------
/// Look up a named phonology preset, returning its inventory (or a helpful error listing the names).
fn phono_preset(name: &str) -> Result<crate::conlang::Phonology, String> {
    let name = name.trim();
    crate::conlang::presets()
        .into_iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, ph)| ph)
        .ok_or_else(|| {
            let names: Vec<&str> = crate::conlang::presets().into_iter().map(|(n, _, _)| n).collect();
            format!("no phonology preset '{}' (try: {})", name, names.join(", "))
        })
}
fn tool_phono_presets(_args: &[Val]) -> Result<Val, String> {
    let names: Vec<String> =
        crate::conlang::presets().into_iter().map(|(n, _, _)| n.to_string()).collect();
    Ok(Val::Text(names.join(" · ")))
}
fn tool_phono_inventory(args: &[Val]) -> Result<Val, String> {
    let ph = phono_preset(&arg_text(args, 0)?)?;
    Ok(Val::Text(format!(
        "C: {} · V: {} · syllables: {}",
        ph.consonants.join(" "),
        ph.vowels.join(" "),
        ph.patterns.join(" ")
    )))
}
fn tool_phono_words(args: &[Val]) -> Result<Val, String> {
    let ph = phono_preset(&arg_text(args, 0)?)?;
    let n = arg_num_or(args, 1, 8).clamp(1, 40) as usize;
    let seed = arg_num_or(args, 2, 1) as u64; // never time-seeded → deterministic and cacheable
    Ok(Val::Text(crate::conlang::generate_words(&ph, n, seed).join(" ")))
}
fn tool_phono_report(args: &[Val]) -> Result<Val, String> {
    let ph = phono_preset(&arg_text(args, 0)?)?;
    let findings = crate::conlang::phonology_report(&ph);
    if findings.is_empty() {
        return Ok(Val::Text("(no typological notes)".into()));
    }
    Ok(Val::Text(
        findings.iter().map(|(tag, t)| format!("[{}] {}", tag, t)).collect::<Vec<_>>().join("  •  "),
    ))
}
fn tool_phono_coin(args: &[Val]) -> Result<Val, String> {
    // build a phonology straight from three space-separated inventories and coin words from it
    let consonants: Vec<String> =
        arg_text(args, 0)?.split_whitespace().map(String::from).collect();
    let vowels: Vec<String> = arg_text(args, 1)?.split_whitespace().map(String::from).collect();
    let patterns: Vec<String> =
        arg_text(args, 2).unwrap_or_default().split_whitespace().map(String::from).collect();
    if consonants.is_empty() || vowels.is_empty() {
        return Ok(Val::Text("(needs at least one consonant and one vowel)".into()));
    }
    let patterns = if patterns.is_empty() {
        vec!["CV".into(), "CVC".into()]
    } else {
        patterns
    };
    let n = arg_num_or(args, 3, 8).clamp(1, 40) as usize;
    let seed = arg_num_or(args, 4, 1) as u64;
    let ph = crate::conlang::Phonology { consonants, vowels, patterns };
    Ok(Val::Text(crate::conlang::generate_words(&ph, n, seed).join(" ")))
}

// ------------------------------- Viz: charts --------------------------------
/// Extract a list of non-negative integers from a value: a list of numbers, or text of numbers
/// separated by spaces or commas (so a live text field can feed a chart).
fn nums_from(v: &Val) -> Vec<u128> {
    match v {
        Val::List(xs) => xs
            .iter()
            .filter_map(|x| match x {
                Val::Num(n) => Some(*n),
                other => other.to_text().trim().parse::<u128>().ok(),
            })
            .collect(),
        Val::Num(n) => vec![*n],
        Val::Text(t) => t
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<u128>().ok())
            .collect(),
    }
}
fn tool_viz_chart(args: &[Val]) -> Result<Val, String> {
    let kind = arg_text(args, 0)?;
    let blank = Val::Text(String::new());
    let values = nums_from(args.get(1).unwrap_or(&blank));
    Ok(Val::Text(crate::viz::render_chart(kind.trim(), &values)))
}

// ------------------------------- Mkt: market data lab -----------------------
// A read-only, deterministic interface over the built-in daily BTC/USD close series: query and
// aggregate it (the database role), chart it (visualization), and run the fast quantitative signals
// the trading advisor is built on (realized volatility, momentum). The heavyweight ML model
// (logistic-regression vol-regime classifier trained in Latte) takes seconds and stays a CLI tool.
fn mkt_closes() -> Vec<f64> {
    crate::marketdata::MARKET_CLOSE_X100.iter().map(|&c| c as f64 / 100.0).collect()
}
fn mkt_returns(s: &[f64]) -> Vec<f64> {
    (1..s.len()).map(|i| (s[i] - s[i - 1]) / s[i - 1]).collect()
}
fn mkt_stdev(r: &[f64]) -> f64 {
    if r.is_empty() {
        return 0.0;
    }
    let mean = r.iter().sum::<f64>() / r.len() as f64;
    (r.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / r.len() as f64).sqrt()
}
fn tool_mkt_span(_args: &[Val]) -> Result<Val, String> {
    let (a, b) = crate::marketdata::MARKET_SPAN;
    Ok(Val::Text(format!(
        "{} … {} ({} daily closes)",
        a,
        b,
        crate::marketdata::MARKET_CLOSE_X100.len()
    )))
}
fn tool_mkt_series(args: &[Val]) -> Result<Val, String> {
    // the last n whole-dollar closes, space-separated — ready to pipe into Viz.chart
    let s = crate::marketdata::MARKET_CLOSE_X100;
    let n = arg_num_or(args, 0, 60).clamp(2, s.len() as u128) as usize;
    Ok(Val::Text(
        s[s.len() - n..].iter().map(|&c| (c / 100).to_string()).collect::<Vec<_>>().join(" "),
    ))
}
fn tool_mkt_chart(args: &[Val]) -> Result<Val, String> {
    let kind = arg_text(args, 0)?;
    let s = crate::marketdata::MARKET_CLOSE_X100;
    let n = arg_num_or(args, 1, 90).clamp(2, s.len() as u128) as usize;
    let tail: Vec<u128> = s[s.len() - n..].iter().map(|&c| (c / 100) as u128).collect();
    Ok(Val::Text(crate::viz::render_chart(kind.trim(), &tail)))
}
/// The trading advisor as a page widget — the SAME advice_text the CLI prints
/// (src/numerics.rs), so the GUI and the terminal can never drift. The market
/// argument switches between the bond-desk duration model and the TA/vol
/// engine for any price tape; the report renders preformatted.
fn tool_trade_advice(args: &[Val]) -> Result<Val, String> {
    let market = arg_text(args, 0)?.to_lowercase();
    let account = arg_num_or(args, 1, 10_000) as f64;
    let report = crate::numerics::advice_text(&market, account, 0.25, None, false, None)
        .map_err(|e| format!("Trade.advice: {}", e))?;
    Ok(Val::Text(format!("<pre style=\"margin:0;white-space:pre-wrap;font-size:12px\">{}</pre>",
        html_escape(&report))))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn tool_mkt_stat(args: &[Val]) -> Result<Val, String> {
    let op = arg_text(args, 0)?;
    let s = mkt_closes();
    if s.is_empty() {
        return Ok(Val::Text("(no data)".into()));
    }
    let v = match op.trim() {
        "last" => s[s.len() - 1],
        "first" => s[0],
        "min" => s.iter().cloned().fold(f64::INFINITY, f64::min),
        "max" => s.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        "mean" => s.iter().sum::<f64>() / s.len() as f64,
        "range" => {
            let mn = s.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = s.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            mx - mn
        }
        "count" => return Ok(Val::Text(format!("{}", s.len()))),
        other => {
            return Ok(Val::Text(format!(
                "unknown stat '{}' (try: last, first, min, max, mean, range, count)",
                other
            )))
        }
    };
    Ok(Val::Text(format!("{:.2}", v)))
}
fn tool_mkt_vol(args: &[Val]) -> Result<Val, String> {
    let s = mkt_closes();
    let w = arg_num_or(args, 0, 30).clamp(2, s.len() as u128) as usize;
    let sd = mkt_stdev(&mkt_returns(&s[s.len() - w..]));
    Ok(Val::Text(format!(
        "{:.2}% per day · {:.1}% annualized  (realized over the last {} days)",
        sd * 100.0,
        sd * 100.0 * 365f64.sqrt(),
        w
    )))
}
fn tool_mkt_advice(args: &[Val]) -> Result<Val, String> {
    // the advisor's core fast signal: price vs its w-day moving average (momentum) plus realized
    // volatility → a direction. (The full pipeline also adds a trained vol-regime model and Kelly
    // sizing; those are the heavier CLI features.)
    let s = mkt_closes();
    let w = arg_num_or(args, 0, 10).clamp(2, (s.len() - 1) as u128) as usize;
    let last = s[s.len() - 1];
    let ma = s[s.len() - w..].iter().sum::<f64>() / w as f64;
    let mom = (last / ma - 1.0) * 100.0;
    let vol = mkt_stdev(&mkt_returns(&s[s.len() - w..])) * 100.0;
    let dir = if mom > 0.5 {
        "LONG"
    } else if mom < -0.5 {
        "SHORT"
    } else {
        "FLAT"
    };
    Ok(Val::Text(format!(
        "{} · momentum {:+.2}% vs {}-day MA · realized vol {:.2}%/day · last {:.2}",
        dir, mom, w, vol, last
    )))
}

fn tool_mkt_forecast(_args: &[Val]) -> Result<Val, String> {
    // HAR-RV: regress |return| on its 1-, 5-, and 22-day averages → next-day volatility forecast.
    // This is the volatility model the trading advisor reports (here over the built-in series).
    let s = mkt_closes();
    let absr: Vec<f64> = mkt_returns(&s).iter().map(|x| (x * 100.0).abs()).collect();
    match crate::numerics::har_rv(&absr) {
        Some((f, _coefs, r2)) => Ok(Val::Text(format!(
            "HAR-RV next-day |return| forecast: {:.2}%/day (in-sample R² {:.3})",
            f, r2
        ))),
        None => Ok(Val::Text("(not enough data for HAR-RV)".into())),
    }
}

// ------------------------------- Date: civil dates --------------------------
fn tool_date_ordinal(args: &[Val]) -> Result<Val, String> {
    let d = arg_text(args, 0)?;
    match crate::dates::date_ordinal(d.trim()) {
        Some(z) => Ok(Val::Text(z.to_string())),
        None => Ok(Val::Text(format!("(bad date '{}'; use YYYY-MM-DD)", d.trim()))),
    }
}
fn tool_date_fromordinal(args: &[Val]) -> Result<Val, String> {
    let z: i64 = arg_text(args, 0)?
        .trim()
        .parse()
        .map_err(|_| "Date.fromordinal expects an integer day number".to_string())?;
    Ok(Val::Text(crate::dates::ordinal_date(z)))
}
fn tool_date_add(args: &[Val]) -> Result<Val, String> {
    let d = arg_text(args, 0)?;
    let days: i64 = arg_text(args, 1)?.trim().parse().unwrap_or(0);
    match crate::dates::add_days(d.trim(), days) {
        Some(s) => Ok(Val::Text(s)),
        None => Ok(Val::Text(format!("(bad date '{}'; use YYYY-MM-DD)", d.trim()))),
    }
}
fn tool_date_between(args: &[Val]) -> Result<Val, String> {
    let a = arg_text(args, 0)?;
    let b = arg_text(args, 1)?;
    match crate::dates::days_between(a.trim(), b.trim()) {
        Some(n) => Ok(Val::Text(format!("{} days", n))),
        None => Ok(Val::Text("(bad date; use YYYY-MM-DD)".into())),
    }
}
fn tool_date_weekday(args: &[Val]) -> Result<Val, String> {
    let d = arg_text(args, 0)?;
    match crate::dates::date_ordinal(d.trim()) {
        // the civil epoch 1970-01-01 (ordinal 0) was a Thursday
        Some(z) => {
            const NAMES: [&str; 7] = [
                "Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday",
            ];
            Ok(Val::Text(NAMES[(((z % 7) + 7) % 7) as usize].to_string()))
        }
        None => Ok(Val::Text(format!("(bad date '{}'; use YYYY-MM-DD)", d.trim()))),
    }
}

// ------------------------------- Sent: sentiment ----------------------------
fn tool_sent_polarity(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(format!("{:.3}", crate::sentiment::polarity_fused(&arg_text(args, 0)?))))
}
fn tool_sent_bond(args: &[Val]) -> Result<Val, String> {
    let t = arg_text(args, 0)?;
    let p = crate::sentiment::bond_polarity(&t);
    let (dove, hawk) = crate::sentiment::rate_counts(&t);
    let label = if p > 0.15 { "bullish for bonds" } else if p < -0.15 { "bearish for bonds" } else { "neutral for bonds" };
    Ok(Val::Text(format!("{:+.3} — {} ({} dovish / {} hawkish)", p, label, dove, hawk)))
}
fn tool_sent_label(args: &[Val]) -> Result<Val, String> {
    let p = crate::sentiment::polarity_fused(&arg_text(args, 0)?);
    let label = if p > 0.05 {
        "positive"
    } else if p < -0.05 {
        "negative"
    } else {
        "neutral"
    };
    Ok(Val::Text(format!("{} ({:+.3})", label, p)))
}
fn tool_sent_counts(args: &[Val]) -> Result<Val, String> {
    let (pos, neg) = crate::sentiment::counts(&arg_text(args, 0)?);
    Ok(Val::Text(format!("{} positive · {} negative sentiment word(s)", pos, neg)))
}

// ------------------------------- Hash: digests ------------------------------
fn tool_hash_sha3(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(crate::sha3::hex(&crate::sha3::sha3_256(arg_text(args, 0)?.as_bytes()))))
}
fn tool_hash_short(args: &[Val]) -> Result<Val, String> {
    let h = crate::sha3::hex(&crate::sha3::sha3_256(arg_text(args, 0)?.as_bytes()));
    let n = arg_num_or(args, 1, 12).clamp(1, h.len() as u128) as usize;
    Ok(Val::Text(h[..n].to_string()))
}

// ------------------------------- Meta: self-description ---------------------
// The environment describing itself: these read the very `module_specs`/`tool_specs` registries
// that drive this page, so the catalogue can never drift from what is actually callable.
fn tool_meta_modules(_args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(module_specs().iter().map(|m| m.name).collect::<Vec<_>>().join(" · ")))
}
fn tool_meta_tools(args: &[Val]) -> Result<Val, String> {
    let m = arg_text(args, 0)?;
    let m = m.trim();
    let sigs: Vec<&str> = tool_specs().iter().filter(|t| t.module == m).map(|t| t.sig).collect();
    if sigs.is_empty() {
        let names: Vec<&str> = module_specs().iter().map(|x| x.name).collect();
        return Ok(Val::Text(format!("no module '{}' (try: {})", m, names.join(", "))));
    }
    Ok(Val::Text(sigs.join("  ·  ")))
}
fn tool_meta_count(_args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(format!(
        "{} tools across {} modules",
        tool_specs().len(),
        module_specs().len()
    )))
}
fn tool_scars_apply(args: &[Val]) -> Result<Val, String> {
    let w = arg_text(args, 0)?;
    let rules = rules_from(args, 1);
    Ok(Val::Text(sca::run_sca(&w, &rules)?))
}
fn tool_txt_split(args: &[Val]) -> Result<Val, String> {
    let s = arg_text(args, 0)?;
    let sep = arg_text(args, 1).unwrap_or_default();
    let parts: Vec<Val> = if sep.is_empty() {
        s.chars().map(|c| Val::Text(c.to_string())).collect()
    } else {
        s.split(sep.as_str())
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .map(Val::Text)
            .collect()
    };
    Ok(Val::List(parts))
}
fn tool_txt_esc(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(esc_attr(&arg_text(args, 0)?)))
}
/// HTML-escape for safe placement in text/attribute contexts.
fn esc_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
/// The shared client runtime for live widgets, emitted once per page. It wires every
/// `[data-live-expr]` widget on the page: each input change is debounced, results are cached
/// client-side (so re-typing a prior value costs no request), and only genuinely new inputs hit
/// `/api/eval`. Factoring this out of each widget keeps the page small and the network quiet.
const LIVE_RUNTIME: &str = "<script>(function(){if(window.__facetLive)return;window.__facetLive=true;\
function wire(box){var expr=box.getAttribute('data-live-expr'),out=box.querySelector('.out'),\
htm=box.hasAttribute('data-live-html'),\
ins=box.querySelectorAll('.li'),cache={},timer=null,seq=0,last=null;\
function set(t){if(htm){out.innerHTML=t;}else{out.textContent=t;}}\
function run(force){var b='expr='+encodeURIComponent(expr);\
ins.forEach(function(i){b+='&'+encodeURIComponent(i.getAttribute('data-n'))+'='+encodeURIComponent(i.value);});\
if(!force){if(b===last)return;last=b;\
if(cache[b]!==undefined){set(cache[b]);return;}}\
var my=++seq;\
fetch('/api/eval',{method:'POST',headers:{'Content-Type':'application/x-www-form-urlencoded'},body:b})\
.then(function(r){return r.text();}).then(function(t){if(!force)cache[b]=t;if(my===seq)set(t);\
if(force){window.dispatchEvent(new Event('facet-action'));}});}\
var btn=box.querySelector('.lgo');\
if(btn){btn.addEventListener('click',function(){set('…');run(true);});}\
else{ins.forEach(function(i){['input','change'].forEach(function(ev){i.addEventListener(ev,function(){if(i.nextElementSibling&&i.nextElementSibling.className==='liv'){i.nextElementSibling.textContent=i.value;}clearTimeout(timer);timer=setTimeout(run,140);});});});\
window.addEventListener('facet-action',function(){cache={};last=null;run(false);});}}\
document.querySelectorAll('[data-live-expr]').forEach(wire);})();</script>";

/// `Live.box(expr, [name, default, …])` renders a LIVE, client-updating widget: one text input per
/// name (seeded with its default) and a result span showing `expr` evaluated against the inputs.
/// The widget is purely declarative markup carrying the expression in a `data-live-expr` attribute;
/// the shared runtime (emitted once per page) wires it with debouncing and client-side caching, so
/// typing is cheap. The initial value is rendered server-side (the no-JS fallback), and the markup
/// carries no counters or ids, so the served HTML stays byte-for-byte deterministic.
/// Shared builder for live widgets. `html_out` selects how the result is shown: as text
/// (`Live.box`) or as raw HTML/SVG (`Live.view`, e.g. for charts). Fields may be flat
/// `[name, default, …]` pairs (text inputs) or structured `[[name, default, opt…], …]` (text inputs
/// and dropdowns).
fn build_live_widget(expr: &str, fields_val: &[Val], html_out: bool) -> Result<String, String> {
    build_live_widget_mode(expr, fields_val, html_out, false)
}

/// `form = true` is the ACTION widget (`Live.form`): the expression runs ONLY when
/// its button is clicked — never at render time (a page view must not perform the
/// action), never on keystrokes, and never from the client cache (a second click
/// must re-execute). Made for side-effecting tools like Db.post and Db.sync.
fn build_live_widget_mode(expr: &str, fields_val: &[Val], html_out: bool, form: bool) -> Result<String, String> {
    if fields_val.is_empty() {
        return Err("a live widget needs a non-empty field list".into());
    }
    struct Field {
        name: String,
        default: String,
        options: Vec<String>, // empty → free-text input; non-empty → a <select> dropdown
    }
    let fields: Vec<Field> = if matches!(fields_val.first(), Some(Val::List(_))) {
        let mut out = Vec::new();
        for f in fields_val {
            let parts: Vec<String> = match f {
                Val::List(xs) => xs.iter().map(|x| x.to_text()).collect(),
                other => {
                    return Err(format!("live widget: each field must be a list, got {}", other.to_text()))
                }
            };
            if parts.len() < 2 {
                return Err("live widget: each field needs at least [name, default]".into());
            }
            out.push(Field {
                name: parts[0].clone(),
                default: parts[1].clone(),
                options: parts[2..].to_vec(),
            });
        }
        out
    } else {
        let pairs: Vec<String> = fields_val.iter().map(|v| v.to_text()).collect();
        if pairs.len() % 2 != 0 {
            return Err("live widget: the flat [name, default, …] list must have even length".into());
        }
        pairs
            .chunks(2)
            .map(|c| Field { name: c[0].clone(), default: c[1].clone(), options: Vec::new() })
            .collect()
    };

    let seed: Vec<(String, String)> =
        fields.iter().map(|f| (f.name.clone(), f.default.clone())).collect();
    // A FORM never evaluates at render time: viewing the page must not perform
    // the action. Its result area starts as an instruction instead.
    let initial = if form { String::new() } else { eval_live(expr, &seed) };

    let mut controls = String::new();
    for f in &fields {
        if f.options.is_empty() {
            controls.push_str(&format!(
                "<input class=\"li\" data-n=\"{n}\" value=\"{v}\" placeholder=\"{n}\" aria-label=\"{n}\">",
                n = esc_attr(&f.name),
                v = esc_attr(&f.default)
            ));
        } else if f.options[0] == "~" {
            // a range slider: options are ["~", min, max, step?]. The adjacent <output> shows the
            // live value; the shared runtime keeps it in sync as the slider moves.
            let min = f.options.get(1).cloned().unwrap_or_else(|| "0".into());
            let max = f.options.get(2).cloned().unwrap_or_else(|| "100".into());
            let step = f.options.get(3).cloned().unwrap_or_else(|| "1".into());
            controls.push_str(&format!(
                "<input type=\"range\" class=\"li\" data-n=\"{n}\" min=\"{min}\" max=\"{max}\" step=\"{step}\" value=\"{v}\" aria-label=\"{n}\"><output class=\"liv\">{v}</output>",
                n = esc_attr(&f.name),
                min = esc_attr(&min),
                max = esc_attr(&max),
                step = esc_attr(&step),
                v = esc_attr(&f.default)
            ));
        } else {
            controls.push_str(&format!(
                "<select class=\"li\" data-n=\"{n}\" aria-label=\"{n}\">",
                n = esc_attr(&f.name)
            ));
            let mut opts = f.options.clone();
            if !opts.iter().any(|o| o == &f.default) {
                opts.insert(0, f.default.clone());
            }
            for o in &opts {
                controls.push_str(&format!(
                    "<option value=\"{v}\"{sel}>{v}</option>",
                    v = esc_attr(o),
                    sel = if o == &f.default { " selected" } else { "" }
                ));
            }
            controls.push_str("</select>");
        }
    }
    // For a view widget the result is raw markup (so SVG/HTML renders); the runtime is told to apply
    // updates via innerHTML by the data-live-html marker. For a box widget the result is escaped text.
    let (out_html, marker) =
        if html_out { (initial.clone(), " data-live-html=\"1\"") } else { (esc_attr(&initial), "") };
    // the form's button carries class "lgo": the runtime wires click -> run(force)
    let button = if form { "<button class=\"lgo\" type=\"button\">Go</button>" } else { "" };
    let widget = format!(
        "<span class=\"live\" data-live-expr=\"{expr}\"{marker}>{controls}{button} <span class=\"out\">{out}</span></span>",
        expr = esc_attr(expr),
        marker = marker,
        controls = controls,
        button = button,
        out = out_html
    );
    // the first live widget on the page also carries the shared runtime
    Ok(format!("{}{}", widget, claim_live_runtime()))
}
// ---------------------------------------------------------------------------
// Db.* — the PERSISTENT, SHARED, NETWORKED state, surfaced to pages.
//
// These tools read and write src/dbservice.rs (WAL-backed, survives restarts)
// through the same service every browser tab, every CLI, and every peer node
// reaches — which is what makes a Facet page with a Db widget a genuinely
// multi-user interface: one user's Db.post is in every other user's next
// render (the data generation keys the page memo, so nothing stale is
// served). Db.sync reconciles a table with ANOTHER Orpheus system.
// ---------------------------------------------------------------------------

/// Post an entry to a board table. The key is a LAMPORT PAIR rendered so that
/// plain string order equals lib/lamport.lat's `lam_lt` total order — millis
/// since the epoch (zero-padded), then this node's id as the deterministic
/// tie-break. Keys are unique and records immutable, so boards merge across
/// nodes as a G-Set: sync in any order, converge to the union.
fn tool_db_post(args: &[Val]) -> Result<Val, String> {
    let board = arg_text(args, 0)?;
    let author = arg_text(args, 1)?;
    let text = arg_text(args, 2)?;
    if !board.chars().all(|c| c.is_ascii_alphanumeric()) || board.is_empty() {
        return Err("Db.post: board name must be alphanumeric".into());
    }
    if author.trim().is_empty() || text.trim().is_empty() {
        return Err("Db.post: need an author and a message".into());
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let key = format!("{:016}-{}", ms, crate::dbsync::node_id());
    // record = [ [1 author] [ [2 text] 0 ] ] — the service's row shape
    let rec = format!("[ [1 \"{}\"] [ [2 \"{}\"] 0 ] ]", lat_str(&author), lat_str(&text));
    let svc = crate::dbservice::service();
    let mut s = svc.lock().unwrap();
    s.put(&board, &key, &rec).map_err(|e| format!("Db.post: {}", e))?;
    Ok(Val::Text(format!("<span style=\"color:#7a9\">posted as {} · {}</span>", html_escape(&author), key)))
}

/// escape a string for inclusion in a Latte string literal
fn lat_str(s: &str) -> String {
    s.replace('\\', " ").replace('"', "'").replace('\n', " ")
}

/// The last `n` entries of a board, newest first, as HTML. Reads through the
/// same generation-cached service reads every dashboard uses.
fn tool_db_board(args: &[Val]) -> Result<Val, String> {
    let board = arg_text(args, 0)?;
    let n = arg_num_or(args, 1, 20) as usize;
    if !board.chars().all(|c| c.is_ascii_alphanumeric()) || board.is_empty() {
        return Err("Db.board: board name must be alphanumeric".into());
    }
    let svc = crate::dbservice::service();
    let mut s = svc.lock().unwrap();
    let mut keys = s.keys(&board).map_err(|e| format!("Db.board: {}", e))?;
    keys.reverse(); // Lamport-pair keys sort oldest-first; show newest first
    let mut out = String::from("<div class=\"board\">");
    let mut shown = 0usize;
    for k in keys.iter().take(n) {
        // fields decoded straight from the row noun (author = 1, message = 2)
        let author = s.field_text(&board, k, 1).unwrap_or_default();
        let text = s.field_text(&board, k, 2).unwrap_or_default();
        let when = k.get(..16).and_then(|t| t.parse::<u64>().ok()).map(fmt_when).unwrap_or_default();
        let who_node = k.get(17..).unwrap_or("");
        out.push_str(&format!(
            "<div style=\"margin:6px 0;padding:6px 8px;background:#22222c;border-radius:4px\">\
             <b>{}</b> <span style=\"color:#8a8676;font-size:11px\">{} · node {}</span><br>{}</div>",
            html_escape(&author), when, html_escape(who_node), html_escape(&text)
        ));
        shown += 1;
    }
    if shown == 0 {
        out.push_str("<i>no posts yet — be the first</i>");
    }
    out.push_str("</div>");
    Ok(Val::Text(out))
}

/// Reconcile a board/table with a peer Orpheus node — the page-widget face of
/// `latte db sync` (src/dbsync.rs; G-Set union, conflicts kept local).
fn tool_db_sync(args: &[Val]) -> Result<Val, String> {
    let board = arg_text(args, 0)?;
    let url = arg_text(args, 1)?;
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Db.sync: peer must be an http(s) URL like http://host:8088".into());
    }
    let report = crate::dbsync::sync(&url, &board).map_err(|e| format!("Db.sync: {}", e))?;
    Ok(Val::Text(format!("<span style=\"color:#7a9\">{}</span>", html_escape(&report))))
}

fn fmt_when(ms: u64) -> String {
    let secs = ms / 1000;
    // days since 1970-01-01 -> the shared civil-date renderer (src/dates.rs,
    // ordinal 0 = 1970-01-01), plus the time of day
    let date = crate::dates::ordinal_date((secs / 86_400) as i64);
    let hh = (secs % 86_400) / 3600;
    let mm = (secs % 3600) / 60;
    format!("{} {:02}:{:02} UTC", date, hh, mm)
}

fn tool_live_form(args: &[Val]) -> Result<Val, String> {
    let expr = arg_text(args, 0)?;
    let fields = match args.get(1) {
        Some(Val::List(vs)) => vs.as_slice(),
        _ => return Err("Live.form(expr, fields) needs a field list".into()),
    };
    Ok(Val::Text(build_live_widget_mode(&expr, fields, true, true)?))
}
fn tool_live_box(args: &[Val]) -> Result<Val, String> {
    let expr = arg_text(args, 0)?;
    let fields = match args.get(1) {
        Some(Val::List(vs)) => vs.as_slice(),
        _ => return Err("Live.box(expr, fields) needs a field list".into()),
    };
    Ok(Val::Text(build_live_widget(&expr, fields, false)?))
}
fn tool_live_view(args: &[Val]) -> Result<Val, String> {
    let expr = arg_text(args, 0)?;
    let fields = match args.get(1) {
        Some(Val::List(vs)) => vs.as_slice(),
        _ => return Err("Live.view(expr, fields) needs a field list".into()),
    };
    Ok(Val::Text(build_live_widget(&expr, fields, true)?))
}
fn tool_txt_upper(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(arg_text(args, 0)?.to_uppercase()))
}
fn tool_txt_lower(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(arg_text(args, 0)?.to_lowercase()))
}
fn tool_txt_rev(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(arg_text(args, 0)?.chars().rev().collect()))
}
fn tool_txt_len(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Num(arg_text(args, 0)?.chars().count() as u128))
}
fn tool_txt_trim(args: &[Val]) -> Result<Val, String> {
    Ok(Val::Text(arg_text(args, 0)?.trim().to_string()))
}
fn tool_txt_words(args: &[Val]) -> Result<Val, String> {
    Ok(Val::List(arg_text(args, 0)?.split_whitespace().map(|w| Val::Text(w.to_string())).collect()))
}
fn tool_txt_replace(args: &[Val]) -> Result<Val, String> {
    let s = arg_text(args, 0)?;
    let from = arg_text(args, 1)?;
    let to = arg_text(args, 2).unwrap_or_default();
    if from.is_empty() {
        return Ok(Val::Text(s));
    }
    Ok(Val::Text(s.replace(&from, &to)))
}
fn tool_txt_cap(args: &[Val]) -> Result<Val, String> {
    let s = arg_text(args, 0)?;
    let mut c = s.chars();
    Ok(Val::Text(match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }))
}
fn tool_txt_join(args: &[Val]) -> Result<Val, String> {
    let sep = arg_text(args, 1).unwrap_or_default();
    match args.first() {
        Some(Val::List(vs)) => {
            Ok(Val::Text(vs.iter().map(|v| v.to_text()).collect::<Vec<_>>().join(&sep)))
        }
        Some(v) => Ok(Val::Text(v.to_text())),
        None => Err("Txt.join: missing list".into()),
    }
}

/// Render an evaluation result as readable text: atoms as decimals (or quoted cords), and proper
/// lists flattened to `[a b c]` instead of nested cons pairs.
fn render_noun(n: &crate::knot::N) -> String {
    use crate::knot::Knot;
    match &**n {
        Knot::Atom(_) => crate::dbservice::noun_to_latte(n).unwrap_or_else(|| "?".into()),
        Knot::Cell(_, _) => {
            let mut elems = Vec::new();
            let mut cur = n.clone();
            loop {
                match &*cur {
                    Knot::Cell(h, t) => {
                        elems.push(render_noun(h));
                        cur = t.clone();
                    }
                    Knot::Atom(a) => {
                        if a.to_u128() != Some(0) {
                            // improper tail: show it after the elements
                            elems.push(crate::dbservice::noun_to_latte(&cur).unwrap_or_else(|| "?".into()));
                        }
                        break;
                    }
                }
            }
            format!("[{}]", elems.join(" "))
        }
    }
}

fn tool_latte_eval(args: &[Val]) -> Result<Val, String> {
    let expr = arg_text(args, 0)?;
    // The FULL scope, not just std: a `def` made in the System console, a module
    // compiled into the running image, and every shipped library are all visible
    // to a page widget — one scope, everywhere. (This used to be pinned to std
    // for speed; the scope-core cache made the full scope effectively free.)
    let libs = crate::latte::all_libs();
    let refs: Vec<&str> = libs.iter().map(|s| s.as_str()).collect();
    let n = crate::latte::run_with_libs(&expr, &refs).map_err(|e| format!("Latte.eval: {}", e))?;
    Ok(Val::Text(render_noun(&n)))
}

pub fn tool_specs() -> &'static [ToolSpec] {
    &[
        ToolSpec {
            module: "SCArs",
            proc: "evolve",
            sig: "SCArs.evolve(word)",
            summary: "derive the modern Heart Speech form of an ancient word by running the full historical rule set",
            handler: tool_scars_evolve,
        },
        ToolSpec {
            module: "SCArs",
            proc: "pie",
            sig: "SCArs.pie(word)",
            summary: "evolve a Proto-Indo-European word forward into Solar Speech (the stage before evolve)",
            handler: tool_scars_pie,
        },
        ToolSpec {
            module: "SCArs",
            proc: "apply",
            sig: "SCArs.apply(word, rule…)",
            summary: "run an arbitrary ordered rule list: substitution, environments, deletion, clusters, boundaries, metathesis",
            handler: tool_scars_apply,
        },
        ToolSpec {
            module: "SCArs",
            proc: "trace",
            sig: "SCArs.trace(word, rule…)",
            summary: "show the word after each successive rule, as an arrow-joined derivation",
            handler: tool_scars_trace,
        },
        ToolSpec {
            module: "Txt",
            proc: "upper",
            sig: "Txt.upper(s)",
            summary: "upper-case every letter",
            handler: tool_txt_upper,
        },
        ToolSpec {
            module: "Txt",
            proc: "lower",
            sig: "Txt.lower(s)",
            summary: "lower-case every letter",
            handler: tool_txt_lower,
        },
        ToolSpec {
            module: "Txt",
            proc: "cap",
            sig: "Txt.cap(s)",
            summary: "upper-case the first character only",
            handler: tool_txt_cap,
        },
        ToolSpec {
            module: "Txt",
            proc: "rev",
            sig: "Txt.rev(s)",
            summary: "reverse the characters of a string",
            handler: tool_txt_rev,
        },
        ToolSpec {
            module: "Txt",
            proc: "trim",
            sig: "Txt.trim(s)",
            summary: "strip leading and trailing whitespace",
            handler: tool_txt_trim,
        },
        ToolSpec {
            module: "Txt",
            proc: "len",
            sig: "Txt.len(s)",
            summary: "count the characters in a string",
            handler: tool_txt_len,
        },
        ToolSpec {
            module: "Txt",
            proc: "words",
            sig: "Txt.words(s)",
            summary: "split a string into a list of whitespace-separated words",
            handler: tool_txt_words,
        },
        ToolSpec {
            module: "Txt",
            proc: "replace",
            sig: "Txt.replace(s, from, to)",
            summary: "replace every occurrence of a substring",
            handler: tool_txt_replace,
        },
        ToolSpec {
            module: "Txt",
            proc: "join",
            sig: "Txt.join(list, sep)",
            summary: "join a list of values with a separator",
            handler: tool_txt_join,
        },
        ToolSpec {
            module: "Txt",
            proc: "split",
            sig: "Txt.split(s, sep)",
            summary: "split a string on a separator into a list (drops empties; trims parts)",
            handler: tool_txt_split,
        },
        ToolSpec {
            module: "Txt",
            proc: "esc",
            sig: "Txt.esc(s)",
            summary: "HTML-escape a string for safe display of user input",
            handler: tool_txt_esc,
        },
        ToolSpec {
            module: "Latte",
            proc: "eval",
            sig: "Latte.eval(expr)",
            summary: "evaluate a Latte expression and render the result",
            handler: tool_latte_eval,
        },
        ToolSpec {
            module: "Db",
            proc: "post",
            sig: "Db.post(board, author, text)",
            summary: "append a message to a shared persistent board (Lamport-pair key: converges across nodes as a G-Set) — use inside Live.form",
            handler: tool_db_post,
        },
        ToolSpec {
            module: "Db",
            proc: "board",
            sig: "Db.board(board, n)",
            summary: "the last n posts of a shared board, newest first — persistent (WAL), visible to every user of this node",
            handler: tool_db_board,
        },
        ToolSpec {
            module: "Db",
            proc: "sync",
            sig: "Db.sync(board, url)",
            summary: "reconcile a board with a peer Orpheus node over http (pull+push union; conflicts kept local) — use inside Live.form",
            handler: tool_db_sync,
        },
        ToolSpec {
            module: "Live",
            proc: "form",
            sig: "Live.form(expr, fields)",
            summary: "an ACTION widget: inputs + a Go button; expr runs only on click (for Db.post, Db.sync, and other side-effecting tools)",
            handler: tool_live_form,
        },
        ToolSpec {
            module: "Live",
            proc: "box",
            sig: "Live.box(expr, fields)",
            summary: "a live, client-updating widget; fields are text inputs or dropdowns, and changes re-run expr via /api/eval",
            handler: tool_live_box,
        },
        ToolSpec {
            module: "Live",
            proc: "view",
            sig: "Live.view(expr, fields)",
            summary: "like Live.box, but renders the result as HTML/SVG (for charts and rich output)",
            handler: tool_live_view,
        },
        ToolSpec {
            module: "Viz",
            proc: "chart",
            sig: "Viz.chart(kind, numbers)",
            summary: "render numbers as an SVG chart; kind is bars, line, or scatter",
            handler: tool_viz_chart,
        },
        ToolSpec {
            module: "AlgoViz",
            proc: "frame",
            sig: "AlgoViz.frame(algo, xs, k, t)",
            summary: "step k of an instrumented algorithm (bubble, insert, quick, binsearch, bfs) over the list xs, drawn as SVG; t is the binsearch target",
            handler: tool_algoviz_frame,
        },
        ToolSpec {
            module: "AlgoViz",
            proc: "steps",
            sig: "AlgoViz.steps(algo, xs, t)",
            summary: "how many frames the algorithm's trace has — the slider range for AlgoViz.frame",
            handler: tool_algoviz_steps,
        },
        ToolSpec {
            module: "Mkt",
            proc: "span",
            sig: "Mkt.span()",
            summary: "the date range and length of the built-in BTC/USD daily close series",
            handler: tool_mkt_span,
        },
        ToolSpec {
            module: "Trade",
            proc: "advice",
            sig: "Trade.advice(market, account)",
            summary: "the full trading-advisor report — bonds (duration model) or any price market (TA + volatility + news), one engine with the CLI",
            handler: tool_trade_advice,
        },
        ToolSpec {
            module: "Mkt",
            proc: "stat",
            sig: "Mkt.stat(op)",
            summary: "aggregate the close series: last, first, min, max, mean, range, or count",
            handler: tool_mkt_stat,
        },
        ToolSpec {
            module: "Mkt",
            proc: "series",
            sig: "Mkt.series(n)",
            summary: "the last n closes as space-separated numbers, ready to chart",
            handler: tool_mkt_series,
        },
        ToolSpec {
            module: "Mkt",
            proc: "chart",
            sig: "Mkt.chart(kind, n)",
            summary: "an SVG chart of the last n closes (kind: bars, line, scatter)",
            handler: tool_mkt_chart,
        },
        ToolSpec {
            module: "Mkt",
            proc: "vol",
            sig: "Mkt.vol(window)",
            summary: "realized volatility over the last window days, daily and annualized",
            handler: tool_mkt_vol,
        },
        ToolSpec {
            module: "Mkt",
            proc: "advice",
            sig: "Mkt.advice(window)",
            summary: "the advisor's core signal: momentum vs a moving average plus realized vol → a direction",
            handler: tool_mkt_advice,
        },
        ToolSpec {
            module: "Mkt",
            proc: "forecast",
            sig: "Mkt.forecast()",
            summary: "HAR-RV next-day volatility forecast (the advisor's volatility model) over the series",
            handler: tool_mkt_forecast,
        },
        ToolSpec {
            module: "Date",
            proc: "add",
            sig: "Date.add(date, days)",
            summary: "add (or subtract) whole days to a YYYY-MM-DD date",
            handler: tool_date_add,
        },
        ToolSpec {
            module: "Date",
            proc: "between",
            sig: "Date.between(a, b)",
            summary: "whole days from date a to date b (negative if b is earlier)",
            handler: tool_date_between,
        },
        ToolSpec {
            module: "Date",
            proc: "weekday",
            sig: "Date.weekday(date)",
            summary: "the day of the week for a YYYY-MM-DD date",
            handler: tool_date_weekday,
        },
        ToolSpec {
            module: "Date",
            proc: "ordinal",
            sig: "Date.ordinal(date)",
            summary: "the day number (days since 1970-01-01) of a date",
            handler: tool_date_ordinal,
        },
        ToolSpec {
            module: "Date",
            proc: "fromordinal",
            sig: "Date.fromordinal(n)",
            summary: "the date n days after 1970-01-01 (the inverse of ordinal)",
            handler: tool_date_fromordinal,
        },
        ToolSpec {
            module: "Sent",
            proc: "polarity",
            sig: "Sent.polarity(text)",
            summary: "sentiment polarity of text in [-1, 1], fusing a lexicon and a model",
            handler: tool_sent_polarity,
        },
        ToolSpec {
            module: "Sent",
            proc: "label",
            sig: "Sent.label(text)",
            summary: "classify text as positive, negative, or neutral with its score",
            handler: tool_sent_label,
        },
        ToolSpec {
            module: "Sent",
            proc: "counts",
            sig: "Sent.counts(text)",
            summary: "count the positive and negative sentiment words in text",
            handler: tool_sent_counts,
        },
        ToolSpec {
            module: "Sent",
            proc: "bond",
            sig: "Sent.bond(text)",
            summary: "sentiment for BOND PRICES: the hawkish/dovish policy axis, with risk-off scored as a Treasury bid",
            handler: tool_sent_bond,
        },
        ToolSpec {
            module: "Hash",
            proc: "sha3",
            sig: "Hash.sha3(text)",
            summary: "the SHA3-256 digest of text, as 64 hex characters",
            handler: tool_hash_sha3,
        },
        ToolSpec {
            module: "Hash",
            proc: "short",
            sig: "Hash.short(text, n)",
            summary: "the first n hex characters of the SHA3-256 digest (a short fingerprint)",
            handler: tool_hash_short,
        },
        ToolSpec {
            module: "Meta",
            proc: "modules",
            sig: "Meta.modules()",
            summary: "the names of every module the environment exposes",
            handler: tool_meta_modules,
        },
        ToolSpec {
            module: "Meta",
            proc: "tools",
            sig: "Meta.tools(module)",
            summary: "the signatures of every tool in a given module",
            handler: tool_meta_tools,
        },
        ToolSpec {
            module: "Meta",
            proc: "count",
            sig: "Meta.count()",
            summary: "how many tools and modules the environment exposes",
            handler: tool_meta_count,
        },
        ToolSpec {
            module: "Phono",
            proc: "presets",
            sig: "Phono.presets()",
            summary: "list the names of the built-in phonology presets",
            handler: tool_phono_presets,
        },
        ToolSpec {
            module: "Phono",
            proc: "inventory",
            sig: "Phono.inventory(preset)",
            summary: "show a preset's consonants, vowels, and syllable patterns",
            handler: tool_phono_inventory,
        },
        ToolSpec {
            module: "Phono",
            proc: "words",
            sig: "Phono.words(preset, n, seed)",
            summary: "coin n words from a named phonology preset, deterministically from seed",
            handler: tool_phono_words,
        },
        ToolSpec {
            module: "Phono",
            proc: "report",
            sig: "Phono.report(preset)",
            summary: "a typological sanity check of a preset's inventory against cross-linguistic norms",
            handler: tool_phono_report,
        },
        ToolSpec {
            module: "Phono",
            proc: "coin",
            sig: "Phono.coin(consonants, vowels, patterns, n, seed)",
            summary: "coin words from a custom inventory: space-separated consonants, vowels, and CV-patterns",
            handler: tool_phono_coin,
        },
    ]
}

/// The modules the environment exposes to a Facet page. Every module here MUST have its own frame
/// on the tools page, and every tool in `tool_specs` MUST belong to one of these modules — the
/// tools-page test enforces both directions, so modules and their interfaces can't drift apart.
pub struct ModuleSpec {
    pub name: &'static str,
    pub summary: &'static str,
}
pub fn module_specs() -> &'static [ModuleSpec] {
    &[
        ModuleSpec { name: "SCArs", summary: "the sound-change applier" },
        ModuleSpec { name: "Txt", summary: "composable text helpers" },
        ModuleSpec { name: "Latte", summary: "the Latte language, evaluated live" },
        ModuleSpec { name: "Live", summary: "live, client-updating widgets" },
        ModuleSpec { name: "Viz", summary: "render data as SVG charts" },
        ModuleSpec { name: "AlgoViz", summary: "instrumented algorithms drawn a step at a time" },
        ModuleSpec { name: "Mkt", summary: "a market-data lab: query, chart, and analyze a price series" },
        ModuleSpec { name: "Date", summary: "civil date arithmetic" },
        ModuleSpec { name: "Sent", summary: "text sentiment scoring" },
        ModuleSpec { name: "Hash", summary: "SHA3-256 digests and short fingerprints" },
        ModuleSpec { name: "Meta", summary: "the environment describing its own tools" },
        ModuleSpec { name: "Phono", summary: "phonology: inventories, word generation, typology" },
        ModuleSpec { name: "Trade", summary: "the trading advisor — one engine, every market, switchable live" },
        ModuleSpec { name: "Db", summary: "shared persistent state: boards, posts, and node-to-node sync" },
    ]
}

fn dispatch_tool(module: &str, proc: &str, args: &[Val]) -> Result<Val, String> {
    match tool_specs().iter().find(|s| s.module == module && s.proc == proc) {
        Some(s) => (s.handler)(args),
        None => Err(format!("no such tool: {}.{}", module, proc)),
    }
}

// ------------------------------- AlgoViz: algorithms, played ---------------
// The instrumented algorithms of lib/algoviz.lat, rendered a frame at a time.
// The heavy lifting is all in Latte (the traces) and the one host SVG
// serializer (src/gfx.rs — the same one `latte gfx` uses); these tools just
// build the call, evaluate it through the adaptive engine, and hand the SVG
// to the page. A Live.view with a slider on `k` plays the algorithm.

/// Parse a number list from a page field ("5 2 8 1" or "5,2,8,1") into a
/// Latte proper-list literal ("[5 2 8 1 0]"); empty input gets a default.
fn algoviz_list(v: &Val) -> String {
    let ns = nums_from(v);
    if ns.is_empty() {
        "[5 2 8 1 9 3 0]".to_string()
    } else {
        let mut s = String::from("[");
        for n in ns.iter().take(16) {
            s.push_str(&n.to_string());
            s.push(' ');
        }
        s.push_str("0]");
        s
    }
}

fn algoviz_eval(expr: &str) -> Result<crate::knot::N, String> {
    crate::rustgen::run_adaptive(expr, &["std", "gfx", "algoviz"])
}

/// AlgoViz.frame(algo, xs, k, t) -> the k-th frame as inline SVG.
fn tool_algoviz_frame(args: &[Val]) -> Result<Val, String> {
    let algo = arg_text(args, 0)?.trim().trim_start_matches('%').to_string();
    if !matches!(algo.as_str(), "bubble" | "insert" | "quick" | "binsearch" | "bfs") {
        return Err("algo must be bubble, insert, quick, binsearch, or bfs".into());
    }
    let blank = Val::Text(String::new());
    let xs = algoviz_list(args.get(1).unwrap_or(&blank));
    let k = arg_num_or(args, 2, 0);
    let t = arg_num_or(args, 3, 8);
    let scene = algoviz_eval(&format!("(av_frame %{} {} {} {})", algo, xs, t, k))?;
    Ok(Val::Text(crate::gfx::render_scene(&scene, 340, 270)))
}

/// AlgoViz.steps(algo, xs, t) -> how many frames the trace has (the slider range).
fn tool_algoviz_steps(args: &[Val]) -> Result<Val, String> {
    let algo = arg_text(args, 0)?.trim().trim_start_matches('%').to_string();
    if !matches!(algo.as_str(), "bubble" | "insert" | "quick" | "binsearch" | "bfs") {
        return Err("algo must be bubble, insert, quick, binsearch, or bfs".into());
    }
    let blank = Val::Text(String::new());
    let xs = algoviz_list(args.get(1).unwrap_or(&blank));
    let t = arg_num_or(args, 2, 8);
    let n = algoviz_eval(&format!("(av_steps %{} {} {})", algo, xs, t))?;
    Ok(Val::Num(n.as_atom().and_then(|a| a.to_u128()).unwrap_or(0)))
}

fn arg_text(args: &[Val], i: usize) -> Result<String, String> {
    args.get(i).map(|v| v.to_text()).ok_or_else(|| format!("missing argument {}", i + 1))
}

/// Read argument `i` as an unsigned number. Live inputs arrive as text, so a numeric string is
/// accepted as well as a `Num`; returns `default` when the argument is absent or unparseable
/// (e.g. a field the user cleared), so widgets degrade gracefully rather than erroring.
fn arg_num_or(args: &[Val], i: usize, default: u128) -> u128 {
    match args.get(i) {
        Some(Val::Num(n)) => *n,
        Some(Val::Text(t)) => t.trim().parse::<u128>().unwrap_or(default),
        _ => default,
    }
}

/// Render a parsed document to its output text (HTML), given an initial environment.
pub fn render_segments(segs: &[Seg], env: &Env) -> Result<String, String> {
    let mut out = String::new();
    for seg in segs {
        match seg {
            Seg::Text(t) => out.push_str(t),
            Seg::Hole(e) => out.push_str(&eval(e, env)?.to_text()),
            Seg::Each(name, list_expr, body) => {
                let lst = eval(list_expr, env)?;
                let items = match lst {
                    Val::List(vs) => vs,
                    other => vec![other],
                };
                for item in items {
                    let mut env2 = env.clone();
                    env2.push((name.clone(), item));
                    out.push_str(&render_segments(body, &env2)?);
                }
            }
            Seg::If(cond, then_body, else_body) => {
                let v = eval(cond, env)?;
                let body = if truthy(&v) { then_body } else { else_body };
                out.push_str(&render_segments(body, env)?);
            }
        }
    }
    Ok(out)
}

/// Template truthiness: empty text, the number 0, and the empty list are false.
fn truthy(v: &Val) -> bool {
    match v {
        Val::Text(s) => !s.is_empty(),
        Val::Num(n) => *n != 0,
        Val::List(vs) => !vs.is_empty(),
    }
}

/// Parse + render a Facet document string.
pub fn render(src: &str) -> Result<String, String> {
    render_with(src, &[])
}

// Live widgets share ONE client runtime script per page (emitted by the first Live.box in document
// order) instead of duplicating wiring per widget. These thread-locals track render nesting so the
// "already emitted" flag is reset once per top-level render — not on the re-entrant renders Live.box
// performs to compute a widget's initial value.
thread_local! {
    static LIVE_EMITTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static RENDER_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Claim the once-per-page live runtime: returns the runtime `<script>` for the first caller in a
/// render, and an empty string thereafter.
fn claim_live_runtime() -> &'static str {
    LIVE_EMITTED.with(|e| {
        if e.get() {
            ""
        } else {
            e.set(true);
            LIVE_RUNTIME
        }
    })
}

/// A process-wide memo for single-expression evaluation. Facet rendering is a pure, deterministic
/// function of (expression, inputs), so identical calls always yield identical text — safe to cache.
/// The cache is keyed within the current library generation and cleared if libraries change.
/// (generation, day) -> memoized results. The DAY joins the key for the same
/// reason it keys the page memo: `Date.*` holes are deterministic within a day,
/// and a long-running server must not serve yesterday's date after midnight.
fn eval_cache() -> &'static Mutex<((u64, u64), HashMap<String, String>)> {
    static C: OnceLock<Mutex<((u64, u64), HashMap<String, String>)>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(((u64::MAX, 0), HashMap::new())))
}

/// Days since the epoch — the second component of the memo keys.
fn today_stamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

/// A cache of *parsed* single-expression snippets, keyed by the expression text. Parsing is purely
/// syntactic — it doesn't depend on the library generation — so a parsed expression never goes
/// stale and can be shared across every input value the user types. This is what makes typing into
/// a live widget cheap on a result-cache miss: only the evaluation runs, never the lexer/parser.
fn parse_cache() -> &'static Mutex<HashMap<String, Arc<Vec<Seg>>>> {
    static P: OnceLock<Mutex<HashMap<String, Arc<Vec<Seg>>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Parse `{{ expr }}` once and memoize the resulting segments by `expr`. Returns shared,
/// already-parsed segments so repeated evaluation against different inputs skips parsing entirely.
fn cached_segments(expr: &str) -> Result<Arc<Vec<Seg>>, String> {
    if let Ok(p) = parse_cache().lock() {
        if let Some(segs) = p.get(expr) {
            return Ok(segs.clone());
        }
    }
    let segs = Arc::new(parse_document(&format!("{{{{ {} }}}}", expr))?);
    if let Ok(mut p) = parse_cache().lock() {
        if p.len() >= 4096 {
            p.clear(); // simple bound; the live path reuses a small set of expressions
        }
        p.insert(expr.to_string(), segs.clone());
    }
    Ok(segs)
}

/// Evaluate a single Facet expression against named inputs and return its text, memoized.
///
/// This is the hot path for live widgets: every keystroke re-evaluates one expression, and the
/// tools page re-renders every widget's initial value on every request. It is cached at two levels:
/// the *result* of `(expr, inputs)` is memoized (keyed by library generation), so re-typed values
/// and shared defaults are free; and on a result miss, the *parsed* expression is reused from
/// `parse_cache`, so a fresh input value pays only for evaluation, never for re-parsing.
pub fn eval_live(expr: &str, inputs: &[(String, String)]) -> String {
    let gen = (crate::latte::lib_generation() ^ (crate::dbservice::data_generation() << 32), today_stamp());
    let mut key = String::with_capacity(expr.len() + inputs.len() * 8 + 1);
    key.push_str(expr);
    for (n, v) in inputs {
        key.push('\u{1}');
        key.push_str(n);
        key.push('\u{2}');
        key.push_str(v);
    }
    if let Ok(mut g) = eval_cache().lock() {
        if g.0 != gen {
            g.1.clear();
            g.0 = gen;
        }
        if let Some(out) = g.1.get(&key) {
            return out.clone();
        }
    }
    // result-cache miss: evaluate, reusing the parsed expression and replicating render_with's
    // top-level bookkeeping (so a nested live widget, if any, still emits its runtime exactly once)
    let out = match cached_segments(expr) {
        Ok(segs) => {
            let top = RENDER_DEPTH.with(|d| {
                let depth = d.get();
                d.set(depth + 1);
                depth == 0
            });
            if top {
                LIVE_EMITTED.with(|e| e.set(false));
            }
            let env: Env =
                inputs.iter().map(|(k, v)| (k.clone(), Val::Text(v.clone()))).collect();
            let r = render_segments(&segs, &env).unwrap_or_else(|e| format!("error: {}", e));
            RENDER_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
            r
        }
        Err(e) => format!("error: {}", e),
    };
    if let Ok(mut g) = eval_cache().lock() {
        if g.1.len() >= 8192 {
            g.1.clear(); // simple bound: drop the table wholesale rather than track LRU
        }
        g.1.insert(key, out.clone());
    }
    out
}

/// Render a Facet page with user-supplied inputs (e.g. HTTP query parameters) seeded into the
/// environment, so `In.field(name, default)` can read them. This is what turns a Facet page into
/// an interactive interface: an HTML form submits `?name=value`, the page re-renders, and the
/// form's tools run against the user's input.
pub fn render_with(src: &str, params: &[(String, String)]) -> Result<String, String> {
    // THE PAGE MEMO. A Facet render is a pure, deterministic function of the page
    // source, the parameters, and the tool registry (the module doc says exactly
    // this), so a whole rendered page can be memoized outright. The key carries
    // the library generation (a `def` or module compile invalidates every page)
    // and the current DAY (so `Date.today`-style holes stay correct without any
    // tool-level special-casing). Repeated visits to a widget-heavy page — the
    // /tools shelf, the /learn tutorials — become a single map lookup.
    let day = today_stamp();
    let gen = crate::latte::lib_generation() ^ (crate::dbservice::data_generation() << 32);
    let mut pkey = String::with_capacity(src.len() / 8 + 64);
    pkey.push_str(&crate::sha3::hex(&crate::sha3::sha3_256(src.as_bytes()))[..32]);
    for (k, v) in params {
        pkey.push('\u{1}');
        pkey.push_str(k);
        pkey.push('\u{2}');
        pkey.push_str(v);
    }
    {
        let mut g = page_cache().lock().unwrap();
        if g.0 != (gen, day) {
            g.1.clear();
            g.0 = (gen, day);
        }
        if let Some(hit) = g.1.get(&pkey) {
            return Ok(hit.clone());
        }
    }
    let top = RENDER_DEPTH.with(|d| {
        let depth = d.get();
        d.set(depth + 1);
        depth == 0
    });
    if top {
        LIVE_EMITTED.with(|e| e.set(false));
    }
    let result = (|| {
        let segs = parse_document(src)?;
        let env: Env = params.iter().map(|(k, v)| (k.clone(), Val::Text(v.clone()))).collect();
        render_segments(&segs, &env)
    })();
    RENDER_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    if let Ok(html) = &result {
        let mut g = page_cache().lock().unwrap();
        if g.1.len() >= 128 {
            g.1.clear();
        }
        g.1.insert(pkey, html.clone());
    }
    result
}

/// (generation, day) -> { page key -> rendered HTML }: see render_with.
fn page_cache() -> &'static std::sync::Mutex<((u64, u64), std::collections::HashMap<String, String>)> {
    static C: std::sync::OnceLock<std::sync::Mutex<((u64, u64), std::collections::HashMap<String, String>)>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(((u64::MAX, 0), std::collections::HashMap::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(render("<h1>hello</h1>").unwrap(), "<h1>hello</h1>");
    }

    #[test]
    fn if_then_else_branches() {
        // non-empty string is truthy
        assert_eq!(render(r#"{{if "x"}}yes{{else}}no{{end}}"#).unwrap(), "yes");
        // empty string is falsey
        assert_eq!(render(r#"{{if ""}}yes{{else}}no{{end}}"#).unwrap(), "no");
        // 0 is falsey, non-zero truthy; if with no else
        assert_eq!(render(r#"{{if 0}}shown{{end}}"#).unwrap(), "");
        assert_eq!(render(r#"{{if 7}}shown{{end}}"#).unwrap(), "shown");
    }

    #[test]
    fn if_nested_in_each() {
        let doc = r#"{{each w in ["", "ligā"]}}{{if w}}<b>{{ SCArs.evolve(w) }}</b>{{else}}-{{end}}{{end}}"#;
        assert_eq!(render(doc).unwrap(), "-<b>liɣō</b>");
    }

    #[test]
    fn calls_the_sca_tool() {
        // the page asks the environment to evolve a Solar word into Heart Speech
        let out = render(r#"<b>{{ SCArs.evolve("zelā") }}</b>"#).unwrap();
        assert_eq!(out, "<b>zealō</b>".replacen("zealō", &sca::evolve("zelā").unwrap(), 1));
        // and it must equal the SCA's own result
        assert_eq!(render(r#"{{ SCArs.evolve("dīwā") }}"#).unwrap(), "nīvō");
        assert_eq!(render(r#"{{ SCArs.evolve("bazdā") }}"#).unwrap(), "mazdō");
    }

    #[test]
    fn let_and_text_helpers_compose() {
        let out = render(r#"{{ let w = SCArs.evolve("ligā") in Txt.cap(w) }}"#).unwrap();
        assert_eq!(out, "Liɣō");
    }

    #[test]
    fn each_builds_a_table() {
        let doc = r#"<table>{{each w in ["ligā", "dīwā", "asā"]}}<tr><td>{{w}}</td><td>{{ SCArs.evolve(w) }}</td></tr>{{end}}</table>"#;
        let out = render(doc).unwrap();
        assert_eq!(
            out,
            "<table><tr><td>ligā</td><td>liɣō</td></tr><tr><td>dīwā</td><td>nīvō</td></tr><tr><td>asā</td><td>azō</td></tr></table>"
        );
    }

    #[test]
    fn deterministic_repeated_tool_calls() {
        // "no problems when the server interacts with the SCA": many calls, identical results
        let doc = r#"{{ SCArs.evolve("wōkā") }}-{{ SCArs.evolve("wōkā") }}-{{ SCArs.evolve("wōkā") }}"#;
        assert_eq!(render(doc).unwrap(), "vūɣō-vūɣō-vūɣō");
    }

    #[test]
    fn shipped_site_renders_with_conlang_words() {
        // The actual page Hymn hosts: it must render and contain the words SCArs
        // produces — including breaking and the stacked macron+tilde form.
        let src = include_str!("../lib/site/index.facet");
        let html = render(src).expect("shipped index.facet renders cleanly");
        for w in ["liɣō", "nīvō", "mazdō", "zealō", "heardō", "gẽtõ", "nū\u{0303}"] {
            assert!(html.contains(w), "rendered page is missing conlang word '{}'", w);
        }
        // and it must reference the bundled font
        assert!(html.contains("ligos-regular.woff2"), "page should @font-face the Ligos font");
    }

    #[test]
    fn tools_page_gives_every_tool_a_full_interface() {
        // The tools page must render and exercise EVERY environment tool's complete interface,
        // each in its own frame. We assert the live, computed output of each procedure.
        let src = include_str!("../lib/site/tools.facet");
        let html = render(src).expect("shipped tools.facet renders cleanly");

        // Extract every live hole {{ ... }} from the source so we can prove each tool is actually
        // INVOKED on the page, not merely mentioned in a <code> sample.
        let mut holes: Vec<&str> = Vec::new();
        let mut rest = src;
        while let Some(i) = rest.find("{{") {
            rest = &rest[i + 2..];
            if let Some(j) = rest.find("}}") {
                holes.push(&rest[..j]);
                rest = &rest[j + 2..];
            } else {
                break;
            }
        }

        // REGISTRY-DRIVEN COMPLETENESS: every tool the environment dispatches must have a frame on
        // the page (documented) AND be invoked live (exercised). A tool added to `tool_specs`
        // without a page interface fails here — interfaces can't silently drift from the tool set.
        for spec in super::tool_specs() {
            let mp = format!("{}.{}", spec.module, spec.proc);
            assert!(html.contains(&mp), "tool {} has no interface frame on the tools page", mp);
            assert!(
                holes.iter().any(|h| h.contains(&format!("{}(", mp))),
                "tool {} is documented but never invoked live on the page",
                mp
            );
        }

        // MODULE-LEVEL COMPLETENESS: every module the environment exposes must have its OWN frame
        // (a "Module —" panel title), and every module must declare at least one tool. Conversely
        // every tool must belong to a declared module — the two registries can't drift apart.
        for m in super::module_specs() {
            assert!(
                html.contains(&format!("{} —", m.name)),
                "module {} has no frame on the tools page",
                m.name
            );
            assert!(
                super::tool_specs().iter().any(|t| t.module == m.name),
                "module {} declares no tools",
                m.name
            );
        }
        for spec in super::tool_specs() {
            assert!(
                super::module_specs().iter().any(|m| m.name == spec.module),
                "tool {}.{} belongs to an undeclared module",
                spec.module,
                spec.proc
            );
        }

        // each module is presented in its own framed panel
        assert!(html.contains("SCArs — the sound-change applier"), "missing SCArs frame");
        assert!(html.contains("Txt — composable text helpers"), "missing Txt frame");
        assert!(html.contains("Latte — the language"), "missing Latte frame");
        assert!(html.contains("Live — client-updating"), "missing Live frame");
        assert!(html.contains("Phono — phonology"), "missing Phono frame");
        assert!(html.contains("Viz — charts"), "missing Viz frame");
        assert!(html.contains("Mkt — market-data lab"), "missing Mkt frame");
        assert!(html.contains("Date — civil date arithmetic"), "missing Date frame");
        assert!(html.contains("Sent — text sentiment"), "missing Sent frame");
        assert!(html.contains("Hash — digests"), "missing Hash frame");
        assert!(html.contains("Meta — the catalogue"), "missing Meta frame");

        // Every tool is a live widget whose server-rendered INITIAL value (the no-JS reading) must
        // be present — i.e. each tool actually computed on the page, not merely mentioned.
        // SCArs: full pipeline (pie → evolve), custom rules (apply), and derivation (trace)
        assert!(html.contains("nakt"), "SCArs.pie initial (nokʷt→nakt) missing");
        assert!(html.contains("liɣō"), "SCArs.evolve initial (ligā→liɣō) missing");
        assert!(html.contains("goso"), "SCArs.apply initial (kasa via k>g;a>o → goso) missing");
        assert!(html.contains("kasa → gasa → goso"), "SCArs.trace derivation missing");
        // Txt: a representative spread of the eleven helpers, each computed live
        assert!(html.contains("HEART SPEECH"), "Txt.upper initial missing");
        assert!(html.contains("Heardō"), "Txt.cap initial missing");
        assert!(html.contains("ralos"), "Txt.rev initial (solar→ralos) missing");
        assert!(html.contains("bonono"), "Txt.replace initial (banana→bonono) missing");
        assert!(html.contains("sol · luna · stella"), "Txt.join initial missing");
        // Latte: the live evaluator's default expression
        assert!(html.contains("[0 1 4 9 16 25]"), "Latte.eval initial (squares of range 6) missing");
        // Phono: presets list, an inventory, and a typological note all computed live
        assert!(html.contains("polynesian · pie · semitic · sinitic"), "Phono.presets missing");
        assert!(html.contains("C: p k m n l h w"), "Phono.inventory initial missing");
        assert!(html.contains("a-i-u triangle"), "Phono.report initial (semitic) missing");
        // Viz/Mkt: a chart actually renders, and the market series is queried live
        assert!(html.contains("<svg"), "page should render at least one SVG chart");
        assert!(html.contains("1318 daily closes"), "Mkt.span initial missing");
        assert!(html.contains(r#"data-live-html="1""#), "an interactive (HTML-rendering) chart is expected");
        // Date/Sent computed live, and the slider control ships
        assert!(html.contains("Wednesday"), "Date.weekday initial (2026-06-10) missing");
        assert!(html.contains("1317 days"), "Date.between initial missing");
        assert!(html.contains("positive"), "Sent.label initial (positive) missing");
        assert!(html.contains(r#"type="range""#), "page should ship a slider control");
        assert!(html.contains("HAR-RV"), "Mkt.forecast initial (HAR-RV) missing");

        // The live runtime and its endpoint are present precisely because the page emits live boxes.
        assert!(html.contains("/api/eval"), "live runtime endpoint missing");
        assert!(html.contains("data-live-expr"), "live widgets not emitted");
        // and at least one widget offers curated choices through a dropdown, not just free text
        assert!(html.contains("<select class=\"li\""), "page should ship a dropdown control");
    }

    #[test]
    fn facet_pages_accept_user_input() {
        // In.field returns the default when unbound, the supplied value when bound
        assert_eq!(render(r#"{{ In.field("w", "ligā") }}"#).unwrap(), "ligā");
        assert_eq!(
            render_with(r#"{{ In.field("w", "ligā") }}"#, &[("w".into(), "kósmā".into())]).unwrap(),
            "kósmā"
        );
        // a supplied word actually drives the tool
        assert_eq!(
            render_with(
                r#"{{ SCArs.evolve(In.field("w", "ligā")) }}"#,
                &[("w".into(), "kósmā".into())]
            )
            .unwrap(),
            render(r#"{{ SCArs.evolve("kósmā") }}"#).unwrap()
        );
        // multi-rule apply via a split form field
        assert_eq!(
            render_with(
                r#"{{ SCArs.apply(In.field("w", "x"), Txt.split(In.field("r", ""), ";")) }}"#,
                &[("w".into(), "kasa".into()), ("r".into(), "k>g; a>o".into())]
            )
            .unwrap(),
            "goso"
        );
        // Txt.esc neutralizes markup in user input
        assert_eq!(render(r#"{{ Txt.esc("<x>&") }}"#).unwrap(), "&lt;x&gt;&amp;");

        // In.field / render_with remain available as the no-JS, query-parameter input path; the
        // shipped page now drives interactivity through live widgets instead, but the primitive
        // still works and the page renders cleanly with no input.
        let src = include_str!("../lib/site/tools.facet");
        assert!(render(src).is_ok(), "page must render with no input (defaults)");
    }

    #[test]
    fn string_literals_support_escapes() {
        // a string can now contain an escaped quote — needed for exprs that embed string literals
        assert_eq!(render(r#"{{ "a\"b" }}"#).unwrap(), "a\"b");
        assert_eq!(render(r#"{{ "tab\tend" }}"#).unwrap(), "tab\tend");
        // an unknown escape is just the char
        assert_eq!(render(r#"{{ "x\;y" }}"#).unwrap(), "x;y");
    }

    #[test]
    fn live_box_generates_interactive_widget() {
        let html = render(r#"{{ Live.box("SCArs.evolve(word)", ["word", "ligā"]) }}"#).unwrap();
        // one seeded input per name
        assert!(html.contains(r#"data-n="word""#), "input missing");
        assert!(html.contains(r#"value="ligā""#), "default not seeded");
        // server-rendered initial value (the no-JS fallback)
        let evolved = render(r#"{{ SCArs.evolve("ligā") }}"#).unwrap();
        assert!(html.contains(&evolved), "initial value missing: {}", evolved);
        // wired to the live endpoint, with the expression embedded for the client
        assert!(html.contains("/api/eval"), "live endpoint missing");
        assert!(html.contains("<script>"), "wiring script missing");
        assert!(html.contains("SCArs.evolve(word)"), "expr not embedded");

        // a multi-input widget whose expr embeds a string literal (exercises string escapes)
        let apply = render(
            r#"{{ Live.box("SCArs.apply(word, Txt.split(rules, \";\"))", ["word", "kasa", "rules", "k>g; a>o"]) }}"#,
        )
        .unwrap();
        assert!(apply.contains(r#"data-n="word""#) && apply.contains(r#"data-n="rules""#), "two inputs expected");
        assert!(apply.contains("goso"), "initial apply result (kasa via k>g;a>o) missing");
        // the rule default is attribute-escaped
        assert!(apply.contains("k&gt;g"), "input value should be HTML-escaped");

        // two identical boxes render identically (no counters/ids → deterministic output)
        let a = render(r#"{{ Live.box("Txt.upper(s)", ["s", "hi"]) }}"#).unwrap();
        let b = render(r#"{{ Live.box("Txt.upper(s)", ["s", "hi"]) }}"#).unwrap();
        assert_eq!(a, b, "identical widgets must render identically");
    }

    #[test]
    fn live_box_supports_select_controls() {
        // structured fields: a text input alongside a dropdown of curated choices
        let html = render(
            r#"{{ Live.box("Txt.join(Txt.words(s), sep)", [["s", "a b c"], ["sep", "-", "-", " · ", "_"]]) }}"#,
        )
        .unwrap();
        assert!(html.contains(r#"<input class="li" data-n="s""#), "text input missing");
        assert!(html.contains(r#"<select class="li" data-n="sep""#), "select missing");
        assert!(
            html.contains(r#"<option value="-" selected>-</option>"#),
            "default option must be selected"
        );
        assert!(html.contains(r#"<option value=" · ">"#), "a listed option is missing");
        // the initial value reflects the default selection: words("a b c") joined with "-"
        assert!(html.contains("a-b-c"), "initial value with default separator missing");

        // a default that isn't among the options is prepended so the control still shows it
        let html2 = render(r#"{{ Live.box("Txt.upper(x)", [["x", "z", "a", "b"]]) }}"#).unwrap();
        assert!(
            html2.contains(r#"<option value="z" selected>z</option>"#),
            "absent default should be added and selected"
        );

        // the original flat [name, default, …] form is unchanged (backward compatible)
        let flat = render(r#"{{ Live.box("Txt.upper(s)", ["s", "hi"]) }}"#).unwrap();
        assert!(
            flat.contains(r#"<input class="li" data-n="s""#) && flat.contains("HI"),
            "flat form regressed"
        );
    }

    #[test]
    fn scars_pie_and_trace_expose_the_full_pipeline() {
        // PIE → Solar Speech: the evolution stage before `evolve`, matching the engine exactly
        assert_eq!(
            render("{{ SCArs.pie(\"nokʷt\") }}").unwrap(),
            sca::pie_to_solar("nokʷt").unwrap()
        );
        assert_eq!(render("{{ SCArs.pie(\"nokʷt\") }}").unwrap(), "nakt");
        // trace shows the word after each successive rule
        assert_eq!(
            render("{{ SCArs.trace(\"kasa\", Txt.split(\"k>g; a>o\", \";\")) }}").unwrap(),
            "kasa → gasa → goso"
        );
        // a trace with no rules is just the input
        assert_eq!(render("{{ SCArs.trace(\"kasa\", []) }}").unwrap(), "kasa");
    }

    #[test]
    fn phono_module_generates_and_describes() {
        // presets are discoverable
        assert_eq!(
            render("{{ Phono.presets() }}").unwrap(),
            "polynesian · pie · semitic · sinitic"
        );
        // inventory of a preset
        assert!(render(r#"{{ Phono.inventory("polynesian") }}"#)
            .unwrap()
            .contains("C: p k m n l h w"));
        // word generation is deterministic for a fixed (preset, n, seed)
        let a = render(r#"{{ Phono.words("polynesian", "8", "7") }}"#).unwrap();
        let b = render(r#"{{ Phono.words("polynesian", "8", "7") }}"#).unwrap();
        assert_eq!(a, b, "same seed must yield the same words");
        assert_eq!(a.split_whitespace().count(), 8, "should coin n words");
        // a different seed yields a different batch
        let c = render(r#"{{ Phono.words("polynesian", "8", "99") }}"#).unwrap();
        assert_ne!(a, c, "different seed should differ");
        // a custom inventory works too
        assert_eq!(
            render(r#"{{ Phono.coin("p t k", "a i", "CV CVC", "5", "1") }}"#)
                .unwrap()
                .split_whitespace()
                .count(),
            5
        );
        // an unknown preset surfaces an error
        assert!(render(r#"{{ Phono.inventory("klingon") }}"#).is_err());
        // the report flags the canonical a-i-u triangle of the 3-vowel semitic system
        assert!(render(r#"{{ Phono.report("semitic") }}"#).unwrap().contains("a-i-u triangle"));
    }

    #[test]
    fn viz_and_mkt_modules_compute() {
        // Viz renders an SVG from numbers parsed out of a text field
        let c = render(r#"{{ Viz.chart("bars", "3 1 4 1 5") }}"#).unwrap();
        assert!(c.trim_start().starts_with("<svg"), "Viz.chart should emit SVG");
        // Mkt queries/aggregates the built-in series (the read-only database role)
        assert!(render("{{ Mkt.span() }}").unwrap().contains("daily closes"));
        assert_eq!(render(r#"{{ Mkt.stat("count") }}"#).unwrap(), "1318");
        assert!(render(r#"{{ Mkt.stat("mean") }}"#).unwrap().contains('.'));
        // series feeds a chart; both Mkt.chart and the composition render SVG
        assert_eq!(render(r#"{{ Mkt.series("5") }}"#).unwrap().split_whitespace().count(), 5);
        assert!(render(r#"{{ Mkt.chart("line", "60") }}"#).unwrap().contains("<svg"));
        let composed = render_with(
            r#"{{ Viz.chart(kind, Mkt.series(n)) }}"#,
            &[("kind".into(), "line".into()), ("n".into(), "40".into())],
        )
        .unwrap();
        assert!(composed.contains("<svg"), "Mkt.series piped into Viz.chart should render");
        // the advisor's signal is deterministic and labeled
        let a = render(r#"{{ Mkt.advice("10") }}"#).unwrap();
        assert_eq!(a, render(r#"{{ Mkt.advice("10") }}"#).unwrap(), "advice must be deterministic");
        assert!(
            a.contains("momentum")
                && (a.contains("LONG") || a.contains("SHORT") || a.contains("FLAT")),
            "advice should give a labeled direction: {}",
            a
        );
        assert!(render(r#"{{ Mkt.vol("30") }}"#).unwrap().contains("annualized"));
    }

    #[test]
    fn live_view_renders_html_not_text() {
        // a view widget marks itself for innerHTML and leaves its result unescaped (so SVG renders)
        let html = render(
            r#"{{ Live.view("Viz.chart(kind, nums)", [["kind", "bars", "bars", "line"], ["nums", "1 2 3"]]) }}"#,
        )
        .unwrap();
        assert!(
            html.contains(r#"data-live-html="1""#),
            "view widget must mark its span for HTML output"
        );
        assert!(html.contains("<svg"), "view widget's initial SVG must be raw (unescaped)");
        // a box widget does the opposite: escapes and does not mark its span for HTML output
        let boxed = render(r#"{{ Live.box("Txt.esc(s)", ["s", "<x>"]) }}"#).unwrap();
        assert!(
            !boxed.contains(r#"data-live-html="1""#),
            "box widget must not request HTML output"
        );
    }

    #[test]
    fn date_and_sent_modules_compute() {
        // Date — deterministic civil arithmetic (no clock)
        assert_eq!(render(r#"{{ Date.add("2026-06-10", "-7") }}"#).unwrap(), "2026-06-03");
        assert_eq!(render(r#"{{ Date.weekday("1970-01-01") }}"#).unwrap(), "Thursday");
        assert_eq!(
            render(r#"{{ Date.between("2022-11-01", "2026-06-10") }}"#).unwrap(),
            "1317 days"
        );
        // ordinal and fromordinal are inverses
        let ord = render(r#"{{ Date.ordinal("2026-06-10") }}"#).unwrap();
        assert_eq!(
            render_with(r#"{{ Date.fromordinal(n) }}"#, &[("n".into(), ord)]).unwrap(),
            "2026-06-10"
        );
        // a malformed date is reported, not panicked
        assert!(render(r#"{{ Date.weekday("nope") }}"#).unwrap().contains("bad date"));

        // Sent — deterministic; clearly-positive text scores positive
        assert!(render(r#"{{ Sent.label("a wonderful and great result") }}"#)
            .unwrap()
            .starts_with("positive"));
        let p = render(r#"{{ Sent.polarity("a wonderful and great result") }}"#).unwrap();
        assert_eq!(
            p,
            render(r#"{{ Sent.polarity("a wonderful and great result") }}"#).unwrap(),
            "sentiment must be deterministic"
        );
        assert_eq!(
            render(r#"{{ Sent.counts("good good bad") }}"#).unwrap(),
            "2 positive · 1 negative sentiment word(s)"
        );
    }

    #[test]
    fn live_box_supports_slider_controls() {
        let html =
            render(r#"{{ Live.box("Mkt.vol(w)", [["w", "30", "~", "5", "120", "5"]]) }}"#).unwrap();
        assert!(html.contains(r#"type="range""#), "slider input missing");
        assert!(
            html.contains(r#"min="5""#) && html.contains(r#"max="120""#) && html.contains(r#"step="5""#),
            "slider bounds missing"
        );
        assert!(html.contains(r#"value="30""#), "slider default missing");
        assert!(html.contains(r#"<output class="liv">30</output>"#), "slider value display missing");
        // it still posts through the same data-n channel the runtime reads
        assert!(html.contains(r#"data-n="w""#), "slider data-n missing");
    }

    #[test]
    fn hash_and_forecast_compute() {
        // SHA3-256: 64 hex chars, deterministic, avalanches on a one-character change
        let full = render(r#"{{ Hash.sha3("Orpheus") }}"#).unwrap();
        assert_eq!(full.len(), 64, "sha3-256 is 64 hex chars");
        assert!(full.chars().all(|c| c.is_ascii_hexdigit()), "digest must be hex");
        assert_eq!(full, render(r#"{{ Hash.sha3("Orpheus") }}"#).unwrap(), "must be deterministic");
        let short = render(r#"{{ Hash.short("Orpheus", "8") }}"#).unwrap();
        assert_eq!(short.len(), 8);
        assert!(full.starts_with(&short), "short fingerprint is a prefix of the full digest");
        assert_ne!(full, render(r#"{{ Hash.sha3("orpheus") }}"#).unwrap(), "case change must avalanche");

        // HAR-RV next-day volatility forecast over the built-in series
        let f = render("{{ Mkt.forecast() }}").unwrap();
        assert!(f.contains("HAR-RV") && f.contains("R²"), "forecast should report model and R²: {}", f);
        assert_eq!(f, render("{{ Mkt.forecast() }}").unwrap(), "forecast must be deterministic");
    }

    #[test]
    fn meta_describes_the_registry() {
        // Meta reads the same registries the page is built from
        let mods = render("{{ Meta.modules() }}").unwrap();
        for m in super::module_specs() {
            assert!(mods.contains(m.name), "Meta.modules omits {}", m.name);
        }
        // count matches the registries exactly
        assert_eq!(
            render("{{ Meta.count() }}").unwrap(),
            format!("{} tools across {} modules", super::tool_specs().len(), super::module_specs().len())
        );
        // tools of a module list its signatures; an unknown module is reported
        let date_tools = render(r#"{{ Meta.tools("Date") }}"#).unwrap();
        assert!(date_tools.contains("Date.add(") && date_tools.contains("Date.weekday("));
        assert!(render(r#"{{ Meta.tools("Nope") }}"#).unwrap().contains("no module"));
    }

    #[test]
    fn txt_helpers_cover_their_options() {
        assert_eq!(render(r#"{{ Txt.lower("HEART") }}"#).unwrap(), "heart");
        assert_eq!(render(r#"{{ Txt.rev("solar") }}"#).unwrap(), "ralos");
        assert_eq!(render(r#"{{ Txt.len("heardō") }}"#).unwrap(), "6");
        assert_eq!(render(r#"{{ Txt.trim("  x  ") }}"#).unwrap(), "x");
        assert_eq!(render(r#"{{ Txt.replace("banana", "a", "o") }}"#).unwrap(), "bonono");
        // words collapses runs of whitespace; join puts them back
        assert_eq!(render(r#"{{ Txt.join(Txt.words("a b  c"), "-") }}"#).unwrap(), "a-b-c");
    }

    #[test]
    fn eval_live_caches_and_matches_uncached() {
        // the live hot path: memoized, deterministic, and identical to the uncached render
        let inp = vec![
            ("word".to_string(), "kasa".to_string()),
            ("rules".to_string(), "k>g; a>o".to_string()),
        ];
        let e = "SCArs.apply(word, Txt.split(rules, \";\"))";
        let a = eval_live(e, &inp);
        assert_eq!(a, "goso");
        assert_eq!(eval_live(e, &inp), a, "second call must be served from cache, identical");
        assert_eq!(
            a,
            render_with(&format!("{{{{ {} }}}}", e), &inp).unwrap(),
            "cached result must match the uncached render"
        );
    }

    #[test]
    fn eval_live_reuses_parsed_expr_across_inputs() {
        // The same expression evaluated against many distinct inputs must always match the
        // uncached render — this is the path where the parsed expression is reused but evaluation
        // re-runs for each new value (the actual typing experience).
        let e = "Txt.cap(Txt.lower(s))";
        for w in ["A", "BB", "héart", "LIGĀ", "speech"] {
            let inp = vec![("s".to_string(), w.to_string())];
            assert_eq!(
                eval_live(e, &inp),
                render_with(&format!("{{{{ {} }}}}", e), &inp).unwrap(),
                "mismatch for input {:?}",
                w
            );
        }
        // a malformed expression surfaces an error rather than panicking
        let bad = eval_live("Txt.upper(", &[("s".into(), "x".into())]);
        assert!(bad.starts_with("error:"), "parse error should surface: {}", bad);
    }

    #[test]
    fn live_runtime_is_shared_once_and_debounced() {
        // a page with several widgets must share ONE runtime script, not one per widget
        let page = render(concat!(
            r#"{{ Live.box("Txt.upper(s)", ["s", "a"]) }}"#,
            r#"{{ Live.box("Txt.cap(s)", ["s", "b"]) }}"#,
            r#"{{ Live.box("SCArs.evolve(word)", ["word", "ligā"]) }}"#,
        ))
        .unwrap();
        assert_eq!(page.matches("function wire(").count(), 1, "runtime must be emitted exactly once");
        assert_eq!(page.matches("<script>").count(), 1, "only the shared runtime carries a script");
        // every widget is still present, wired declaratively via its expression attribute
        assert_eq!(
            page.matches(r#"class="live" data-live-expr="#).count(),
            3,
            "all three widgets must be present"
        );
        // performance characteristics: keystrokes are debounced and results cached client-side
        assert!(page.contains("setTimeout"), "runtime should debounce input");
        assert!(page.contains("cache["), "runtime should cache results client-side");
        // determinism is preserved across renders even with the shared-runtime bookkeeping
        let again = render(concat!(
            r#"{{ Live.box("Txt.upper(s)", ["s", "a"]) }}"#,
            r#"{{ Live.box("Txt.cap(s)", ["s", "b"]) }}"#,
            r#"{{ Live.box("SCArs.evolve(word)", ["word", "ligā"]) }}"#,
        ))
        .unwrap();
        assert_eq!(page, again, "the same page must render byte-for-byte identically");
    }
}

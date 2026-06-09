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
                buf.push(cs[i]);
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
            dispatch_tool(module, proc, &av)
        }
    }
}

/// Intra-environment tool dispatch. This is the single seam through which Facet
/// pages reach every tool in the system; new tools are added here.
fn dispatch_tool(module: &str, proc: &str, args: &[Val]) -> Result<Val, String> {
    match (module, proc) {
        // SCArs — the sound change applier; runs on Loom via the Latte engine.
        ("SCArs", "evolve") => {
            let w = arg_text(args, 0)?;
            Ok(Val::Text(sca::evolve(&w)?))
        }
        ("SCArs", "apply") => {
            // SCArs.apply(word, rule1, rule2, ...)
            let w = arg_text(args, 0)?;
            let rules: Vec<String> = args[1..].iter().map(|v| v.to_text()).collect();
            Ok(Val::Text(sca::run_sca(&w, &rules)?))
        }
        // Small text helpers, to show composable functional tooling.
        ("Txt", "upper") => Ok(Val::Text(arg_text(args, 0)?.to_uppercase())),
        ("Txt", "cap") => {
            let s = arg_text(args, 0)?;
            let mut c = s.chars();
            Ok(Val::Text(match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }))
        }
        ("Txt", "join") => {
            // Txt.join(list, sep)
            let sep = arg_text(args, 1).unwrap_or_default();
            match args.get(0) {
                Some(Val::List(vs)) => {
                    Ok(Val::Text(vs.iter().map(|v| v.to_text()).collect::<Vec<_>>().join(&sep)))
                }
                Some(v) => Ok(Val::Text(v.to_text())),
                None => Err("Txt.join: missing list".into()),
            }
        }
        _ => Err(format!("no such tool: {}.{}", module, proc)),
    }
}

fn arg_text(args: &[Val], i: usize) -> Result<String, String> {
    args.get(i).map(|v| v.to_text()).ok_or_else(|| format!("missing argument {}", i + 1))
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
    let segs = parse_document(src)?;
    render_segments(&segs, &Vec::new())
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
}

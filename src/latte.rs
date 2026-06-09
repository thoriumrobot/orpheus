//! Latte — the keyword surface language. This is a real, if deliberately small,
//! compiler: lexer -> recursive-descent parser -> code generator that emits Loom
//! formulas (which are themselves knots). It implements the subset of the spec
//! needed to write non-trivial agents:
//!
//!   nil                      the atom 0
//!   123        %incr         literal atom / tag (cord)
//!   name                     a face (resolved to a tree address)
//!   [a b c]                  a right-nested cell/tuple
//!   +(e)                     increment            (Loom SUCC)
//!   head e   tail e          projections          (Loom THEN + ADDRESS)
//!   (a == b)                 equality             (Loom SAME)
//!   if c then a else b       conditional          (Loom IF)
//!   let x = e in body        binding              (Loom PUSH)
//!   case s of %t -> e ; _ -> e end   tag switch   (nested IF + SAME)
//!   loop with [v = e, ...] : body end / again(...) recursion (a trap: PUSH+CALL)
//!
//! The compiler tracks an environment mapping face names to tree addresses, and
//! rebases those addresses with `peg` every time the subject grows. This is the
//! same technique Hoon uses to compile against "the subject"; here the programmer
//! never writes a raw address.

use crate::knot::{cell, cord, num, N};
use crate::loom::{f_axis, f_quote, peg};

// ----------------------------- AST ------------------------------------------
#[derive(Debug, Clone)]
pub enum Ast {
    Lit(u128),
    Tag(String),
    Nil,
    Var(String),
    Tuple(Vec<Ast>),
    Inc(Box<Ast>),
    Head(Box<Ast>),
    Tail(Box<Ast>),
    IsCell(Box<Ast>),
    Eq(Box<Ast>, Box<Ast>),
    If(Box<Ast>, Box<Ast>, Box<Ast>),
    Let(String, Box<Ast>, Box<Ast>),
    Case(Box<Ast>, Vec<(Option<String>, Ast)>),
    Loop(Vec<(String, Ast)>, Box<Ast>),
    Again(Vec<Ast>),
    Call(String, Vec<Ast>),   // (fname arg1 arg2 ...) — invoke a sibling arm or a gate value
    Fast(String, Box<Ast>),   // fast %name body — wrap body in a jet hint
    Gate(Vec<String>, Box<Ast>), // fn [params] -> body — a first-class closure value
}

// ----------------------------- lexer ----------------------------------------
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(u128),
    Tag(String),   // %ident
    Ident(String), // includes keywords
    LBrack,
    RBrack,
    LParen,
    RParen,
    Comma,
    Colon,
    Eq,    // ==
    Assign, // =
    Arrow, // ->
    Plus,  // +
    Semi,  // ;
}

/// Convert a byte offset into (line, column), both 1-based, for error messages.
fn line_col(src: &str, off: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut col = 1u32;
    for (k, ch) in src.char_indices() {
        if k >= off {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Lex into tokens paired with their start byte offset (for positioned errors).
fn lex(src: &str) -> Result<Vec<(Tok, usize)>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out: Vec<(Tok, usize)> = Vec::new();
    macro_rules! push {
        ($t:expr, $start:expr) => {
            out.push(($t, $start))
        };
    }
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // line comment ::  ... \n
        if c == ':' && i + 1 < b.len() && b[i + 1] == b':' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        match c {
            '[' => { push!(Tok::LBrack, start); i += 1; }
            ']' => { push!(Tok::RBrack, start); i += 1; }
            '(' => { push!(Tok::LParen, start); i += 1; }
            ')' => { push!(Tok::RParen, start); i += 1; }
            ',' => { push!(Tok::Comma, start); i += 1; }
            ';' => { push!(Tok::Semi, start); i += 1; }
            ':' => { push!(Tok::Colon, start); i += 1; }
            '+' => { push!(Tok::Plus, start); i += 1; }
            '-' => {
                if i + 1 < b.len() && b[i + 1] == b'>' {
                    push!(Tok::Arrow, start);
                    i += 2;
                } else {
                    let (l, co) = line_col(src, start);
                    return Err(format!("line {}, col {}: unexpected '-' (did you mean '->'?)", l, co));
                }
            }
            '=' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    push!(Tok::Eq, start);
                    i += 2;
                } else {
                    push!(Tok::Assign, start);
                    i += 1;
                }
            }
            '%' => {
                let s = i + 1;
                let mut j = s;
                while j < b.len() && is_ident(b[j]) {
                    j += 1;
                }
                if j == s {
                    let (l, co) = line_col(src, start);
                    return Err(format!("line {}, col {}: empty %tag", l, co));
                }
                push!(Tok::Tag(src[s..j].to_string()), start);
                i = j;
            }
            _ if c.is_ascii_digit() => {
                // 0x.. (hex) and 0b.. (binary) aura literals, else decimal
                let (radix, skip) = if c == '0' && i + 1 < b.len() && (b[i + 1] | 0x20) == b'x' {
                    (16u32, 2)
                } else if c == '0' && i + 1 < b.len() && (b[i + 1] | 0x20) == b'b' {
                    (2u32, 2)
                } else {
                    (10u32, 0)
                };
                i += skip;
                let ds = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let digits: String = src[ds..i].chars().filter(|c| *c != '_').collect();
                let n = u128::from_str_radix(&digits, radix).map_err(|_| {
                    let (l, co) = line_col(src, start);
                    format!("line {}, col {}: bad number literal", l, co)
                })?;
                push!(Tok::Num(n), start);
            }
            _ if is_ident_start(b[i]) => {
                while i < b.len() && is_ident(b[i]) {
                    i += 1;
                }
                push!(Tok::Ident(src[start..i].to_string()), start);
            }
            _ => {
                let (l, co) = line_col(src, start);
                return Err(format!("line {}, col {}: unexpected character '{}'", l, co, c));
            }
        }
    }
    Ok(out)
}

fn is_ident_start(b: u8) -> bool {
    (b as char).is_ascii_alphabetic() || b == b'_'
}
fn is_ident(b: u8) -> bool {
    (b as char).is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

// ----------------------------- parser ---------------------------------------
struct Parser {
    toks: Vec<Tok>,
    offs: Vec<usize>,
    src_len: usize,
    src: String,
    pos: usize,
}

impl Parser {
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
    /// Source line/col of the current token (or end-of-input).
    fn cur_loc(&self) -> (u32, u32) {
        let off = self.offs.get(self.pos).copied().unwrap_or(self.src_len);
        line_col(&self.src, off)
    }
    fn err(&self, msg: impl AsRef<str>) -> String {
        let (l, c) = self.cur_loc();
        let found = match self.peek() {
            Some(t) => format!("{:?}", t),
            None => "end of input".to_string(),
        };
        format!("line {}, col {}: {} (found {})", l, c, msg.as_ref(), found)
    }
    fn eat(&mut self, t: &Tok) -> Result<(), String> {
        match self.peek() {
            Some(x) if x == t => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.err(format!("expected {:?}", t))),
        }
    }
    fn eat_kw(&mut self, kw: &str) -> Result<(), String> {
        match self.peek() {
            Some(Tok::Ident(s)) if s == kw => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.err(format!("expected keyword '{}'", kw))),
        }
    }
    #[allow(dead_code)]
    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s == kw)
    }

    fn expr(&mut self) -> Result<Ast, String> {
        match self.peek() {
            Some(Tok::Ident(s)) => match s.as_str() {
                "let" => self.parse_let(),
                "if" => self.parse_if(),
                "case" => self.parse_case(),
                "loop" => self.parse_loop(),
                "again" => self.parse_again(),
                "fast" => self.parse_fast(),
                "fn" => {
                    let (params, body) = self.parse_gate()?;
                    Ok(Ast::Gate(params, Box::new(body)))
                }
                _ => self.primary(),
            },
            _ => self.primary(),
        }
    }

    fn parse_let(&mut self) -> Result<Ast, String> {
        self.eat_kw("let")?;
        let name = self.ident()?;
        self.eat(&Tok::Assign)?;
        let val = self.expr()?;
        self.eat_kw("in")?;
        let body = self.expr()?;
        Ok(Ast::Let(name, Box::new(val), Box::new(body)))
    }

    fn parse_if(&mut self) -> Result<Ast, String> {
        self.eat_kw("if")?;
        let c = self.expr()?;
        self.eat_kw("then")?;
        let a = self.expr()?;
        self.eat_kw("else")?;
        let b = self.expr()?;
        Ok(Ast::If(Box::new(c), Box::new(a), Box::new(b)))
    }

    fn parse_case(&mut self) -> Result<Ast, String> {
        self.eat_kw("case")?;
        let scrut = self.expr()?;
        self.eat_kw("of")?;
        let mut arms = Vec::new();
        loop {
            let pat = match self.peek() {
                Some(Tok::Tag(t)) => {
                    let t = t.clone();
                    self.next();
                    Some(t)
                }
                Some(Tok::Ident(s)) if s == "_" => {
                    self.next();
                    None
                }
                _ => return Err(self.err("case arm: expected %tag or _")),
            };
            self.eat(&Tok::Arrow)?;
            let body = self.expr()?;
            arms.push((pat, body));
            match self.peek() {
                Some(Tok::Semi) => {
                    self.next();
                    continue;
                }
                Some(Tok::Ident(s)) if s == "end" => {
                    self.next();
                    break;
                }
                _ => return Err(self.err("case: expected ';' or 'end'")),
            }
        }
        Ok(Ast::Case(Box::new(scrut), arms))
    }

    fn parse_loop(&mut self) -> Result<Ast, String> {
        self.eat_kw("loop")?;
        self.eat_kw("with")?;
        self.eat(&Tok::LBrack)?;
        let mut binds = Vec::new();
        loop {
            let name = self.ident()?;
            self.eat(&Tok::Assign)?;
            let val = self.expr()?;
            binds.push((name, val));
            match self.peek() {
                Some(Tok::Comma) => {
                    self.next();
                    continue;
                }
                Some(Tok::RBrack) => {
                    self.next();
                    break;
                }
                _ => return Err(self.err("loop bindings: expected ',' or ']'")),
            }
        }
        self.eat(&Tok::Colon)?;
        let body = self.expr()?;
        self.eat_kw("end")?;
        Ok(Ast::Loop(binds, Box::new(body)))
    }

    fn parse_again(&mut self) -> Result<Ast, String> {
        self.eat_kw("again")?;
        self.eat(&Tok::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek(), Some(Tok::RParen)) {
            loop {
                args.push(self.expr()?);
                match self.peek() {
                    Some(Tok::Comma) => {
                        self.next();
                        continue;
                    }
                    Some(Tok::RParen) => break,
                    _ => return Err(self.err("again: expected ',' or ')'")),
                }
            }
        }
        self.eat(&Tok::RParen)?;
        Ok(Ast::Again(args))
    }

    fn parse_fast(&mut self) -> Result<Ast, String> {
        self.eat_kw("fast")?;
        let name = match self.next() {
            Some(Tok::Tag(t)) => t,
            _ => return Err(self.err("fast: expected %name")),
        };
        let body = self.expr()?;
        Ok(Ast::Fast(name, Box::new(body)))
    }

    /// Parse a gate `fn [p0 p1 ...] -> body`, returning (params, body).
    fn parse_gate(&mut self) -> Result<(Vec<String>, Ast), String> {
        self.eat_kw("fn")?;
        self.eat(&Tok::LBrack)?;
        let mut params = Vec::new();
        while !matches!(self.peek(), Some(Tok::RBrack)) {
            params.push(self.ident()?);
            if matches!(self.peek(), Some(Tok::Comma)) {
                self.next();
            }
        }
        self.eat(&Tok::RBrack)?;
        self.eat(&Tok::Arrow)?;
        let body = self.expr()?;
        Ok((params, body))
    }

    fn primary(&mut self) -> Result<Ast, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.next();
                Ok(Ast::Lit(n))
            }
            Some(Tok::Tag(t)) => {
                self.next();
                Ok(Ast::Tag(t))
            }
            Some(Tok::Plus) => {
                self.next();
                self.eat(&Tok::LParen)?;
                let e = self.expr()?;
                self.eat(&Tok::RParen)?;
                Ok(Ast::Inc(Box::new(e)))
            }
            Some(Tok::Ident(s)) => {
                match s.as_str() {
                    "nil" => {
                        self.next();
                        Ok(Ast::Nil)
                    }
                    "head" => {
                        self.next();
                        Ok(Ast::Head(Box::new(self.primary()?)))
                    }
                    "tail" => {
                        self.next();
                        Ok(Ast::Tail(Box::new(self.primary()?)))
                    }
                    "iscell" => {
                        self.next();
                        Ok(Ast::IsCell(Box::new(self.primary()?)))
                    }
                    _ => {
                        self.next();
                        Ok(Ast::Var(s))
                    }
                }
            }
            Some(Tok::LBrack) => {
                self.next();
                let mut elems = Vec::new();
                loop {
                    if matches!(self.peek(), Some(Tok::RBrack)) {
                        self.next();
                        break;
                    }
                    elems.push(self.expr()?);
                    if matches!(self.peek(), Some(Tok::Comma)) {
                        self.next();
                    }
                }
                if elems.is_empty() {
                    return Err("empty [] is not a value".into());
                }
                if elems.len() == 1 {
                    Ok(elems.pop().unwrap())
                } else {
                    Ok(Ast::Tuple(elems))
                }
            }
            Some(Tok::LParen) => {
                self.next();
                let a = self.expr()?;
                match self.peek() {
                    Some(Tok::Eq) => {
                        self.next();
                        let b = self.expr()?;
                        self.eat(&Tok::RParen)?;
                        Ok(Ast::Eq(Box::new(a), Box::new(b)))
                    }
                    Some(Tok::RParen) => {
                        self.next();
                        Ok(a) // grouping
                    }
                    _ => {
                        // function application: (fname arg1 arg2 ...)
                        let name = match a {
                            Ast::Var(n) => n,
                            _ => return Err(self.err("call: expected a function name")),
                        };
                        let mut args = Vec::new();
                        while !matches!(self.peek(), Some(Tok::RParen)) {
                            args.push(self.expr()?);
                            if matches!(self.peek(), Some(Tok::Comma)) {
                                self.next();
                            }
                        }
                        self.eat(&Tok::RParen)?;
                        Ok(Ast::Call(name, args))
                    }
                }
            }
            _ => Err(self.err("unexpected token")),
        }
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.peek().cloned() {
            Some(Tok::Ident(s)) => {
                self.pos += 1;
                Ok(s)
            }
            _ => Err(self.err("expected identifier")),
        }
    }
}

pub fn parse(src: &str) -> Result<Ast, String> {
    let lexed = lex(src)?;
    let (toks, offs): (Vec<Tok>, Vec<usize>) = lexed.into_iter().unzip();
    let mut p = Parser {
        toks,
        offs,
        src_len: src.len(),
        src: src.to_string(),
        pos: 0,
    };
    let e = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(p.err("unexpected trailing input"));
    }
    Ok(e)
}

/// Parse a module: `core [name] arm = fn [..] -> .. ; arm2 = .. end`.
fn parse_module(src: &str) -> Result<Vec<(String, Vec<String>, Ast)>, String> {
    let lexed = lex(src)?;
    let (toks, offs): (Vec<Tok>, Vec<usize>) = lexed.into_iter().unzip();
    let mut p = Parser {
        toks,
        offs,
        src_len: src.len(),
        src: src.to_string(),
        pos: 0,
    };
    p.eat_kw("core")?;
    // optional module name
    if let Some(Tok::Ident(s)) = p.peek() {
        if s != "end" {
            // peek ahead: if it's "<ident> =", it's an arm, not a module name
            let is_arm = matches!(p.toks.get(p.pos + 1), Some(Tok::Assign));
            if !is_arm {
                p.next(); // consume module name
            }
        }
    }
    let mut arms = Vec::new();
    loop {
        if p.is_kw("end") {
            p.next();
            break;
        }
        let name = p.ident()?;
        p.eat(&Tok::Assign)?;
        let (params, body) = p.parse_gate()?;
        arms.push((name, params, body));
        match p.peek() {
            Some(Tok::Semi) => {
                p.next();
            }
            _ => {} // `;` between arms is optional
        }
    }
    if p.pos != p.toks.len() {
        return Err(p.err("unexpected trailing input after module"));
    }
    Ok(arms)
}

// --------------------------- code generation --------------------------------
#[derive(Clone)]
struct Env {
    faces: Vec<(String, u128)>,                          // name -> axis in current subject
    loops: Option<LoopCtx>,                              // innermost active loop, if any
    funcs: std::sync::Arc<Vec<(String, u128, usize)>>,   // name -> (arm axis in core, arity)
    module_core: Option<u128>,                           // where the module core sits in subject
}
#[derive(Clone)]
struct LoopCtx {
    core_axis: u128,             // where the trap's core sits in the current subject
    vars: Vec<(String, u128)>,   // loop var name -> axis in current subject
}

impl Env {
    #[allow(dead_code)]
    fn bare(faces: Vec<(String, u128)>) -> Env {
        Env {
            faces,
            loops: None,
            funcs: std::sync::Arc::new(Vec::new()),
            module_core: None,
        }
    }
    fn lookup(&self, name: &str) -> Option<u128> {
        self.faces.iter().rev().find(|(n, _)| n == name).map(|(_, a)| *a)
    }
    fn lookup_func(&self, name: &str) -> Option<(u128, usize)> {
        self.funcs.iter().find(|(n, _, _)| n == name).map(|(_, a, ar)| (*a, *ar))
    }
    /// Everything in the current subject moves into the tail (axis 3) when we PUSH.
    fn rebase_push(&self) -> Env {
        let faces = self.faces.iter().map(|(n, a)| (n.clone(), peg(3, *a))).collect();
        let loops = self.loops.as_ref().map(|lc| LoopCtx {
            core_axis: peg(3, lc.core_axis),
            vars: lc.vars.iter().map(|(n, a)| (n.clone(), peg(3, *a))).collect(),
        });
        Env {
            faces,
            loops,
            funcs: self.funcs.clone(),
            module_core: self.module_core.map(|a| peg(3, a)),
        }
    }
}

/// Compile `src` against an initial face environment (name -> axis from the subject root).
#[allow(dead_code)]
pub fn compile(src: &str, faces: &[(&str, u128)]) -> Result<N, String> {
    let ast = parse(src)?;
    let env = Env::bare(faces.iter().map(|(n, a)| (n.to_string(), *a)).collect());
    gen(&ast, &env)
}

/// The Orpheus standard library, linkable via `import std`.
pub const STD_LAT: &str = include_str!("../lib/std.lat");
/// The mold/aura type system, linkable via `import mold`.
pub const MOLD_LAT: &str = include_str!("../lib/mold.lat");
/// The Mocha application environment support library, linkable via `import mocha`.
pub const MOCHA_LAT: &str = include_str!("../lib/mocha.lat");
/// Planning calculations (Towards a New Socialism), linkable via `import plan`.
pub const PLAN_LAT: &str = include_str!("../lib/plan.lat");
/// Signed fixed-point numbers, linkable via `import num`.
pub const NUM_LAT: &str = include_str!("../lib/num.lat");
/// N-dimensional tensors, linkable via `import tensor`.
pub const TENSOR_LAT: &str = include_str!("../lib/tensor.lat");
/// Machine-learning training, linkable via `import ml`.
pub const ML_LAT: &str = include_str!("../lib/ml.lat");
/// Data visualization layout, linkable via `import plot`.
pub const PLOT_LAT: &str = include_str!("../lib/plot.lat");
/// Small vector library, linkable via `import vec`.
pub const VEC_LAT: &str = include_str!("../lib/vec.lat");
pub const CHESS_LAT: &str = include_str!("../lib/chess.lat");
pub const CHESSML_LAT: &str = include_str!("../lib/chessml.lat");


fn builtin_lib(name: &str) -> Option<&'static str> {
    match name {
        "std" => Some(STD_LAT),
        "mold" => Some(MOLD_LAT),
        "mocha" => Some(MOCHA_LAT),
        "plan" => Some(PLAN_LAT),
        "num" => Some(NUM_LAT),
        "tensor" => Some(TENSOR_LAT),
        "ml" => Some(ML_LAT),
        "plot" => Some(PLOT_LAT),
        "vec" => Some(VEC_LAT),
        "chess" => Some(CHESS_LAT),
        "chessml" => Some(CHESSML_LAT),
        _ => None,
    }
}

// ---- runtime libraries -----------------------------------------------------
// Libraries registered while the program is running (from a file, the network, or a
// connected machine) — no recompile needed. `import NAME` consults these first, so a
// runtime lib can shadow or extend the built-ins and can itself import other libraries.
static RUNTIME_LIBS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::OnceLock::new();
fn runtime_libs() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    RUNTIME_LIBS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}
/// Register (or replace) a library by name at run time. Afterwards `import NAME` resolves to it.
pub fn register_runtime_lib(name: &str, src: &str) {
    runtime_libs().lock().unwrap().insert(name.to_string(), src.to_string());
    // a changed library invalidates the compiled-library cache
    gather_cache().lock().unwrap().clear();
}

// Compiled-library cache: each library's arms are gathered (parsed + imports resolved) once
// and reused, so repeated `eval`/console/REPL calls don't re-parse the standard libraries.
static GATHER_CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Vec<Arm>>>> =
    std::sync::OnceLock::new();
fn gather_cache() -> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<Arm>>> {
    GATHER_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}
/// Gather a named library's arms, caching the (compiled) result.
fn gather_lib(name: &str) -> Result<Vec<Arm>, String> {
    if let Some(a) = gather_cache().lock().unwrap().get(name) {
        return Ok(a.clone());
    }
    let src = resolve_lib(name).ok_or_else(|| format!("unknown import '{}'", name))?;
    let arms = gather(&src)?;
    gather_cache().lock().unwrap().insert(name.to_string(), arms.clone());
    Ok(arms)
}

fn find_core_name(src: &str) -> Option<String> {
    let mut toks = src.split_whitespace();
    while let Some(t) = toks.next() {
        if t == "core" {
            return toks.next().map(|s| s.to_string());
        }
    }
    None
}

/// Compile a Latte module source and register it as a runtime library (Oberon-style: edit a
/// module, compile it, and it is loaded into the running system). Returns a status message.
pub fn compile_and_register(src: &str) -> Result<String, String> {
    let name = find_core_name(src).ok_or("source must contain a `core NAME` block")?;
    // verify it actually compiles before installing it
    let arms = gather(src)?;
    let _ = compile_arms(&arms)?;
    let own = parse_module(&split_imports(src).1).map(|a| a.len()).unwrap_or(0);
    register_runtime_lib(&name, src);
    Ok(format!(
        "compiled module '{}' ({} arms) — loaded into the system; `import {}` or call it in the console",
        name, own, name
    ))
}
/// Names of all runtime-registered libraries (sorted).
pub fn runtime_lib_names() -> Vec<String> {
    let mut v: Vec<String> = runtime_libs().lock().unwrap().keys().cloned().collect();
    v.sort();
    v
}

/// Every library available by default: all built-ins plus anything registered at run time.
/// This is the standard scope for the REPL, `eval`, and the GUI console — everything is in
/// scope without manual `import`s (later libraries shadow earlier ones on name clashes).
pub fn all_libs() -> Vec<String> {
    let mut v: Vec<String> = [
        "std", "mold", "mocha", "plan", "num", "tensor", "ml", "plot", "vec", "chess", "chessml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.extend(runtime_lib_names());
    v
}
/// Resolve a library name to its source: runtime registry first, then the built-ins.
fn resolve_lib(name: &str) -> Option<String> {
    if let Some(s) = runtime_libs().lock().unwrap().get(name) {
        return Some(s.clone());
    }
    builtin_lib(name).map(|s| s.to_string())
}

type Arm = (String, Vec<String>, Ast);

/// Resolve a module's `import`s recursively into a flat, merged arm list.
fn gather(src: &str) -> Result<Vec<Arm>, String> {
    let (imports, body) = split_imports(src);
    let mut lists: Vec<Vec<Arm>> = Vec::new();
    for imp in &imports {
        lists.push(gather_lib(imp)?);
    }
    lists.push(parse_module(&body)?);
    Ok(merge_arms(lists))
}

/// Compile a flat list of arms (possibly gathered from several linked modules)
/// into one core value `[battery 0]` with a single shared namespace.
fn compile_arms(arms: &[Arm]) -> Result<(N, Vec<(String, u128)>), String> {
    let n = arms.len();
    if n == 0 {
        return Err("module has no arms".into());
    }
    let mut funcs: Vec<(String, u128, usize)> = Vec::new();
    for (k, (name, params, _)) in arms.iter().enumerate() {
        if params.is_empty() {
            return Err(format!("arm '{}' must take at least one parameter", name));
        }
        funcs.push((name.clone(), balanced_leaf_axis(2, k, n), params.len()));
    }
    let funcs = std::sync::Arc::new(funcs);
    let mut arm_formulas: Vec<N> = Vec::with_capacity(n);
    for (_name, params, body) in arms {
        let arity = params.len();
        let mut faces = Vec::new();
        for (j, p) in params.iter().enumerate() {
            faces.push((p.clone(), tuple_elem_axis(3, j, arity)));
        }
        let env = Env {
            faces,
            loops: None,
            funcs: funcs.clone(),
            module_core: Some(1),
        };
        arm_formulas.push(gen(body, &env)?);
    }
    let battery = build_battery(&arm_formulas);
    let core = cell(battery, num(0));
    let axes = funcs.iter().map(|(nm, a, _)| (nm.clone(), *a)).collect();
    Ok((core, axes))
}

/// Merge arms from several sources into one namespace. Later definitions of a name
/// replace earlier ones (so a user module can shadow the standard library).
fn merge_arms(lists: Vec<Vec<Arm>>) -> Vec<Arm> {
    let mut out: Vec<Arm> = Vec::new();
    for list in lists {
        for arm in list {
            if let Some(slot) = out.iter_mut().find(|a| a.0 == arm.0) {
                *slot = arm; // shadow
            } else {
                out.push(arm);
            }
        }
    }
    out
}

/// Link several Latte module sources into one core (shared namespace).
#[allow(dead_code)]
pub fn link(srcs: &[&str]) -> Result<(N, Vec<(String, u128)>), String> {
    let mut lists = Vec::new();
    for s in srcs {
        lists.push(gather(s)?);
    }
    compile_arms(&merge_arms(lists))
}

/// Compile a module, honoring leading `import NAME` lines (built-in libraries).
pub fn compile_module(src: &str) -> Result<(N, Vec<(String, u128)>), String> {
    compile_arms(&gather(src)?)
}

/// Split leading `import NAME` lines from the rest of a module source.
fn split_imports(src: &str) -> (Vec<String>, String) {
    let mut imports = Vec::new();
    let mut rest_lines: Vec<&str> = Vec::new();
    let mut still_imports = true;
    for line in src.lines() {
        let t = line.trim();
        if still_imports && (t.is_empty() || t.starts_with("::")) {
            continue; // skip blanks/comments in the import preamble
        }
        if still_imports {
            if let Some(name) = t.strip_prefix("import ") {
                imports.push(name.trim().to_string());
                continue;
            }
            still_imports = false;
        }
        rest_lines.push(line);
    }
    (imports, rest_lines.join("\n"))
}

/// Evaluate a bare expression with the given built-in libraries linked in scope.
pub fn run_with_libs(expr_src: &str, libs: &[&str]) -> Result<N, String> {
    crate::jets::register_std_jets();
    let ast = parse(expr_src)?;
    let mut lists: Vec<Vec<Arm>> = Vec::new();
    for lib in libs {
        lists.push(gather_lib(lib)?);
    }
    lists.push(vec![("__main".to_string(), vec!["_".to_string()], ast)]);
    let arms = merge_arms(lists);
    let (core, axes) = compile_arms(&arms)?;
    let main_axis = axes
        .iter()
        .find(|(n, _)| n == "__main")
        .map(|(_, a)| *a)
        .ok_or("missing __main")?;
    let core2 = crate::loom::edit(&crate::atom::Atom::from_u128(3), &num(0), &core)
        .map_err(|e| format!("{:?}", e))?;
    let armf = crate::loom::slot(&crate::atom::Atom::from_u128(main_axis), &core2)
        .map_err(|e| format!("{:?}", e))?;
    crate::loom::tar(&core2, &armf).map_err(|e| format!("{:?}", e))
}

#[allow(dead_code)]
/// Evaluate a bare expression with the standard library and mold system in scope.
pub fn eval_with_std(expr_src: &str) -> Result<N, String> {
    run_with_libs(expr_src, &["std", "mold"])
}

/// Gather an expression plus its library closure into the full merged arm list (the program),
/// with the expression installed as the `__main` arm. Used by the Latte→Rust compiler.
pub fn gather_program(
    expr_src: &str,
    libs: &[&str],
) -> Result<Vec<(String, Vec<String>, Ast)>, String> {
    let ast = parse(expr_src)?;
    let mut lists: Vec<Vec<Arm>> = Vec::new();
    for lib in libs {
        lists.push(gather_lib(lib)?);
    }
    lists.push(vec![("__main".to_string(), vec!["_".to_string()], ast)]);
    Ok(merge_arms(lists))
}

/// Compile a set of libraries once into a reusable core (battery + axes), so its arms can be
/// invoked many times cheaply (no re-parsing/-compiling per call). This is what lets a
/// long-running "machine" hold a compiled program and answer move after move.
pub fn compile_library_program(libs: &[&str]) -> Result<(N, Vec<(String, u128)>), String> {
    crate::jets::register_std_jets();
    let mut lists: Vec<Vec<Arm>> = Vec::new();
    for lib in libs {
        lists.push(gather_lib(lib)?);
    }
    let arms = merge_arms(lists);
    compile_arms(&arms)
}

/// Invoke arm `name` of a compiled program on the argument tuple `args`.
pub fn call_arm(core: &N, axes: &[(String, u128)], name: &str, args: N) -> Result<N, String> {
    let axis = axes
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, a)| *a)
        .ok_or_else(|| format!("no arm '{}'", name))?;
    let core2 = crate::loom::edit(&crate::atom::Atom::from_u128(3), &args, core)
        .map_err(|e| format!("{:?}", e))?;
    let armf = crate::loom::slot(&crate::atom::Atom::from_u128(axis), &core2)
        .map_err(|e| format!("{:?}", e))?;
    crate::loom::tar(&core2, &armf).map_err(|e| format!("{:?}", e))
}
/// self-hosting REPL). `defs` is the body of a `core` (zero or more `name = …` arms);
/// the expression may reference those arms and the std/mold/plan libraries.
pub fn run_module_expr(defs: &str, expr: &str) -> Result<N, String> {
    crate::jets::register_std_jets();
    let src = format!(
        "import std\nimport mold\nimport plan\ncore __repl\n{}\n  __main = fn [_] -> {}\nend\n",
        defs, expr
    );
    let (core, axes) = compile_module(&src)?;
    let main_axis = axes
        .iter()
        .find(|(n, _)| n == "__main")
        .map(|(_, a)| *a)
        .ok_or("missing __main")?;
    let core2 = crate::loom::edit(&crate::atom::Atom::from_u128(3), &num(0), &core)
        .map_err(|e| format!("{:?}", e))?;
    let armf = crate::loom::slot(&crate::atom::Atom::from_u128(main_axis), &core2)
        .map_err(|e| format!("{:?}", e))?;
    crate::loom::tar(&core2, &armf).map_err(|e| format!("{:?}", e))
}

// The battery is laid out as a *balanced* binary tree of arms, so the axis of the deepest arm
// grows like n rather than 2^n. A right-nested chain would push the deepest arm's axis past
// 2^64 at ~65 arms (corrupting addressing); balancing keeps axes small and slot lookups shallow.
fn balanced_leaf_axis(root: u128, k: usize, n: usize) -> u128 {
    if n <= 1 {
        return root;
    }
    let l = (n + 1) / 2;
    if k < l {
        balanced_leaf_axis(root * 2, k, l)
    } else {
        balanced_leaf_axis(root * 2 + 1, k - l, n - l)
    }
}
fn build_battery(fs: &[N]) -> N {
    if fs.len() == 1 {
        return fs[0].clone();
    }
    let l = (fs.len() + 1) / 2;
    cell(build_battery(&fs[..l]), build_battery(&fs[l..]))
}

fn tuple_elem_axis(root: u128, k: usize, n: usize) -> u128 {
    // element k of a right-nested n-tuple rooted at `root`
    let mut axis = root;
    for _ in 0..k {
        axis = peg(axis, 3);
    }
    if k + 1 < n {
        peg(axis, 2)
    } else {
        axis
    }
}

fn gen(ast: &Ast, env: &Env) -> Result<N, String> {
    match ast {
        Ast::Lit(n) => Ok(f_quote(num(*n))),
        Ast::Nil => Ok(f_quote(num(0))),
        Ast::Tag(t) => Ok(f_quote(cord(t))),
        Ast::Var(name) => match env.lookup(name) {
            Some(axis) => Ok(f_axis(axis)),
            None => Err(format!("unbound face '{}'", name)),
        },
        Ast::Tuple(elems) => {
            // right-nested autocons of the element formulas
            let mut it = elems.iter().rev();
            let last = it.next().unwrap();
            let mut acc = gen(last, env)?;
            for e in it {
                let f = gen(e, env)?;
                acc = cell(f, acc); // [Ff acc] -> AUTOCONS
            }
            Ok(acc)
        }
        Ast::Inc(e) => Ok(cell(num(4), gen(e, env)?)),
        Ast::Head(e) => Ok(cell(num(7), cell(gen(e, env)?, f_axis(2)))),
        Ast::Tail(e) => Ok(cell(num(7), cell(gen(e, env)?, f_axis(3)))),
        Ast::IsCell(e) => Ok(cell(num(3), gen(e, env)?)),
        Ast::Eq(a, b) => Ok(cell(num(5), cell(gen(a, env)?, gen(b, env)?))),
        Ast::If(c, a, b) => Ok(cell(
            num(6),
            cell(gen(c, env)?, cell(gen(a, env)?, gen(b, env)?)),
        )),
        Ast::Let(name, val, body) => {
            let fv = gen(val, env)?;
            let mut env2 = env.rebase_push();
            env2.faces.push((name.clone(), 2)); // freshly pushed value at axis 2
            let fb = gen(body, &env2)?;
            Ok(cell(num(8), cell(fv, fb)))
        }
        Ast::Case(scrut, arms) => {
            let fs = gen(scrut, env)?;
            // default (else) branch
            let mut acc = match arms.iter().find(|(p, _)| p.is_none()) {
                Some((_, dflt)) => gen(dflt, env)?,
                None => crate::loom::f_axis(0), // no match -> slot 0 -> crash
            };
            for (pat, body) in arms.iter().rev() {
                if let Some(tag) = pat {
                    let eq = cell(num(5), cell(fs.clone(), f_quote(cord(tag))));
                    let fb = gen(body, env)?;
                    acc = cell(num(6), cell(eq, cell(fb, acc)));
                }
            }
            Ok(acc)
        }
        Ast::Loop(binds, body) => gen_loop(binds, body, env),
        Ast::Again(args) => gen_again(args, env),
        Ast::Call(name, args) => {
            // 1) a sibling module arm (static by-name call)
            if let Some((arm_axis, arity)) = env.lookup_func(name) {
                if args.len() != arity {
                    return Err(format!(
                        "function '{}' expects {} argument(s), got {}",
                        name,
                        arity,
                        args.len()
                    ));
                }
                let mc = env
                    .module_core
                    .ok_or_else(|| format!("call to '{}' outside a core", name))?;
                let sample = build_sample(args, env)?;
                let edit = cell(num(10), cell(cell(num(3), sample), f_axis(mc)));
                return Ok(cell(num(9), cell(num(arm_axis), edit)));
            }
            // 2) a face holding a first-class gate value: set its sample, invoke arm 2
            if let Some(face_axis) = env.lookup(name) {
                if args.is_empty() {
                    return Err(format!("gate '{}' applied to no arguments", name));
                }
                let sample = build_sample(args, env)?;
                let edit = cell(num(10), cell(cell(num(6), sample), f_axis(face_axis)));
                return Ok(cell(num(9), cell(num(2), edit)));
            }
            Err(format!("unknown function or gate '{}'", name))
        }
        Ast::Fast(name, body) => Ok(crate::loom::f_jet(name, gen(body, env)?)),
        Ast::Gate(params, body) => {
            let arity = params.len();
            if arity == 0 {
                return Err("a gate must take at least one parameter".into());
            }
            // gate value = [battery [sample context]]; context captures the current
            // subject (closure). Inside the body: sample at axis 6, context at axis 7.
            let mut faces: Vec<(String, u128)> =
                env.faces.iter().map(|(n, a)| (n.clone(), peg(7, *a))).collect();
            for (j, p) in params.iter().enumerate() {
                faces.push((p.clone(), tuple_elem_axis(6, j, arity)));
            }
            let genv = Env {
                faces,
                loops: None,
                funcs: env.funcs.clone(),
                module_core: env.module_core.map(|a| peg(7, a)),
            };
            let body_f = gen(body, &genv)?;
            // [ [1 body] [ [1 0] [0 1] ] ]  => [battery [bunt context]]
            let payload = cell(cell(num(1), num(0)), f_axis(1));
            Ok(cell(cell(num(1), body_f), payload))
        }
    }
}

/// Build a call sample: a single arg is passed bare; multiple args form a right-nested tuple.
fn build_sample(args: &[Ast], env: &Env) -> Result<N, String> {
    let mut it = args.iter().rev();
    let mut sample = gen(it.next().unwrap(), env)?;
    for a in it {
        sample = cell(gen(a, env)?, sample);
    }
    Ok(sample)
}

fn gen_loop(binds: &[(String, Ast)], body: &Ast, env: &Env) -> Result<N, String> {
    let n = binds.len();
    if n == 0 {
        return Err("loop needs at least one variable".into());
    }
    // initial sample = [v0 v1 ... v(n-1)] compiled in the *current* env
    let mut sample = {
        let mut it = binds.iter().rev();
        let (_, last) = it.next().unwrap();
        let mut acc = gen(last, env)?;
        for (_, e) in it {
            acc = cell(gen(e, env)?, acc);
        }
        acc
    };
    // BODY runs against the core = [BODY [sample s]] where s is the old subject.
    // sample sits at payload-head axis 6, old subject s at axis 7.
    let mut body_env = Env {
        faces: env.faces.iter().map(|(nm, a)| (nm.clone(), peg(7, *a))).collect(),
        loops: None,
        funcs: env.funcs.clone(),
        module_core: env.module_core.map(|a| peg(7, a)),
    };
    let mut vars = Vec::with_capacity(n);
    for (k, (name, _)) in binds.iter().enumerate() {
        let axis = tuple_elem_axis(6, k, n);
        body_env.faces.push((name.clone(), axis));
        vars.push((name.clone(), axis));
    }
    body_env.loops = Some(LoopCtx { core_axis: 1, vars });
    let body_f = gen(body, &body_env)?;

    // loopForm = [8 sample [8 [1 BODY] [9 2 [0 1]]]]
    let call = cell(num(9), cell(num(2), f_axis(1)));
    let push_battery = cell(num(8), cell(f_quote(body_f), call));
    // guard: `sample` was moved into use above; rebuild not needed
    let _ = &mut sample;
    Ok(cell(num(8), cell(sample, push_battery)))
}

fn gen_again(args: &[Ast], env: &Env) -> Result<N, String> {
    let lc = env
        .loops
        .as_ref()
        .ok_or_else(|| "again used outside a loop".to_string())?;
    if args.len() != lc.vars.len() {
        return Err(format!(
            "again expects {} argument(s), got {}",
            lc.vars.len(),
            args.len()
        ));
    }
    // EDIT_WHOLE: edit each loop var axis in the whole subject [0 1] with its new value.
    let mut base = f_axis(1);
    for (i, (_, axis)) in lc.vars.iter().enumerate() {
        let fv = gen(&args[i], env)?;
        // [10 [axis fv] base]
        base = cell(num(10), cell(cell(num(*axis), fv), base));
    }
    // produce the edited core, then invoke its battery (arm 2): [9 2 [7 EDIT [0 core_axis]]]
    let edited_core = cell(num(7), cell(base, f_axis(lc.core_axis)));
    Ok(cell(num(9), cell(num(2), edited_core)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knot::{cell, num};
    use crate::loom::tar;

    #[test]
    fn runtime_library_can_be_added_and_imported() {
        // a library that does not exist at compile time, registered at run time
        register_runtime_lib(
            "greet",
            "import std\ncore greet\n  twice = fn [x] -> (add x x)\n  bump = fn [x] -> (add (twice x) 1)\nend",
        );
        let n = run_with_libs("(bump 20)", &["std", "greet"]).unwrap();
        assert_eq!(n.as_atom().unwrap().to_u128().unwrap(), 41);
        // a runtime lib may import another runtime lib
        register_runtime_lib(
            "greet2",
            "import greet\ncore greet2\n  quad = fn [x] -> (twice (twice x))\nend",
        );
        let m = run_with_libs("(quad 5)", &["std", "greet", "greet2"]).unwrap();
        assert_eq!(m.as_atom().unwrap().to_u128().unwrap(), 20);
        assert!(runtime_lib_names().contains(&"greet".to_string()));
    }

    #[test]
    fn vec_library_worked_example() {
        // the example from docs/adding-libraries.md: vdot of two lists
        let n = run_with_libs("(vdot [1 [2 [3 0]]] [4 [5 [6 0]]])", &["std", "vec"]).unwrap();
        assert_eq!(n.as_atom().unwrap().to_u128().unwrap(), 32);
        let s = run_with_libs("(vsum [3 [4 [5 0]]])", &["std", "vec"]).unwrap();
        assert_eq!(s.as_atom().unwrap().to_u128().unwrap(), 12);
    }

    fn run(src: &str, subject: N, faces: &[(&str, u128)]) -> N {
        let f = compile(src, faces).expect("compile");
        tar(&subject, &f).expect("eval")
    }

    fn call_arm(core: &N, axis: u128, args: N) -> N {
        use crate::atom::Atom;
        let core2 = crate::loom::edit(&Atom::from_u128(3), &args, core).expect("edit sample");
        let armf = crate::loom::slot(&Atom::from_u128(axis), &core2).expect("slot arm");
        tar(&core2, &armf).expect("run arm")
    }
    fn axis_of(axes: &[(String, u128)], name: &str) -> u128 {
        axes.iter().find(|(m, _)| m == name).unwrap().1
    }

    #[test]
    fn module_functions_and_calls() {
        // A small standard library written entirely in Latte.
        let src = "core lib
          add    = fn [a b] -> loop with [acc = a, i = 0] :
                     if (i == b) then acc else again(+(acc), +(i)) end
          double = fn [x] -> (add x x)
          sum3   = fn [a b c] -> (add a (add b c))
        end";
        let (core, axes) = compile_module(src).expect("compile module");
        assert_eq!(call_arm(&core, axis_of(&axes, "double"), num(21)), num(42));
        let args = cell(num(1), cell(num(2), num(3)));
        assert_eq!(call_arm(&core, axis_of(&axes, "sum3"), args), num(6));
    }

    #[test]
    fn stdlib_arithmetic_and_lists() {
        let user = "core u
           calc = fn [_] -> (mul (sub 100 58) 1)
           cmp  = fn [_] -> (lt 3 5)
           summ = fn [_] -> let g = fn [a b] -> (add a b) in (foldl g 0 [ 10 [ 20 [ 12 0 ] ] ])
        end";
        let (core, axes) = link(&[STD_LAT, user]).unwrap();
        assert_eq!(call_arm(&core, axis_of(&axes, "calc"), num(0)), num(42));
        assert_eq!(call_arm(&core, axis_of(&axes, "cmp"), num(0)), num(0));
        assert_eq!(call_arm(&core, axis_of(&axes, "summ"), num(0)), num(42));
    }

    #[test]
    fn eval_uses_std() {
        assert_eq!(eval_with_std("(mul 6 7)").unwrap(), num(42));
        assert_eq!(eval_with_std("(sub 50 8)").unwrap(), num(42));
        assert_eq!(eval_with_std("(div 84 2)").unwrap(), num(42));
        assert_eq!(eval_with_std("(mod 142 100)").unwrap(), num(42));
        assert_eq!(eval_with_std("(lt 5 3)").unwrap(), num(1));
        assert_eq!(eval_with_std("(len [ 1 [ 2 [ 3 0 ] ] ])").unwrap(), num(3));
    }

    #[test]
    fn import_directive_links_std() {
        let src = "import std\ncore u\n  calc = fn [_] -> (add 40 2)\nend";
        let (core, axes) = compile_module(src).unwrap();
        assert_eq!(call_arm(&core, axis_of(&axes, "calc"), num(0)), num(42));
    }

    #[test]
    fn user_module_can_shadow_std() {
        let user = "core u
           add  = fn [a b] -> 0
           test = fn [_] -> (add 40 2)
        end";
        let (core, axes) = link(&[STD_LAT, user]).unwrap();
        assert_eq!(call_arm(&core, axis_of(&axes, "test"), num(0)), num(0));
    }

    #[test]
    fn higher_order_map_and_fold() {
        // map and foldl are Latte library functions that take a GATE VALUE.
        let src = "core std
          add     = fn [a b] -> loop with [acc = a, i = 0] :
                      if (i == b) then acc else again(+(acc), +(i)) end
          reverse = fn [lst] -> loop with [cur = lst, acc = 0] :
                      if (cur == 0) then acc else again((tail cur), [ (head cur) acc ]) end
          map     = fn [f lst] -> loop with [cur = lst, acc = 0] :
                      if (cur == 0) then (reverse acc)
                      else again((tail cur), [ (f (head cur)) acc ]) end
          foldl   = fn [f z lst] -> loop with [cur = lst, acc = z] :
                      if (cur == 0) then acc else again((tail cur), (f acc (head cur))) end
          test_map = fn [lst] -> let g = fn [x] -> +(x) in (map g lst)
          test_sum = fn [lst] -> let g = fn [a b] -> (add a b) in (foldl g 0 lst)
        end";
        let (core, axes) = compile_module(src).expect("compile module");
        let lst = cell(num(1), cell(num(2), cell(num(3), num(0))));
        let mapped = call_arm(&core, axis_of(&axes, "test_map"), lst.clone());
        assert_eq!(mapped, cell(num(2), cell(num(3), cell(num(4), num(0)))));
        let lst2 = cell(num(10), cell(num(20), cell(num(30), num(0))));
        let summed = call_arm(&core, axis_of(&axes, "test_sum"), lst2);
        assert_eq!(summed, num(60));
    }

    #[test]
    fn closure_captures_environment() {
        let src = "core t
          add  = fn [a b] -> loop with [acc = a, i = 0] :
                   if (i == b) then acc else again(+(acc), +(i)) end
          make = fn [k] -> let g = fn [x] -> (add x k) in (g 5)
        end";
        let (core, axes) = compile_module(src).expect("compile");
        assert_eq!(call_arm(&core, axis_of(&axes, "make"), num(100)), num(105));
    }

    #[test]
    fn call_inside_loop_inside_let() {
        // Stresses module-core rebasing through both a let (peg 3) and a loop (peg 7):
        // triangle(n) = 0+1+...+(n-1).
        let src = "core
          add      = fn [a b] -> loop with [acc = a, i = 0] :
                       if (i == b) then acc else again(+(acc), +(i)) end
          triangle = fn [n] -> let z = 0 in
                       loop with [acc = z, i = 0] :
                         if (i == n) then acc else again((add acc i), +(i)) end
        end";
        let (core, axes) = compile_module(src).expect("compile module");
        assert_eq!(call_arm(&core, axis_of(&axes, "triangle"), num(5)), num(10));
        assert_eq!(call_arm(&core, axis_of(&axes, "triangle"), num(10)), num(45));
    }

    #[test]
    fn literals_and_cells() {
        assert_eq!(run("42", num(0), &[]), num(42));
        assert_eq!(run("[1 2 3]", num(0), &[]), cell(num(1), cell(num(2), num(3))));
        assert_eq!(run("%incr", num(0), &[]), crate::knot::cord("incr"));
    }

    #[test]
    fn faces_and_proj() {
        // subject = [10 20], a@2 b@3
        let s = cell(num(10), num(20));
        assert_eq!(run("a", s.clone(), &[("a", 2), ("b", 3)]), num(10));
        assert_eq!(run("b", s.clone(), &[("a", 2), ("b", 3)]), num(20));
        // head/tail of a literal cell
        assert_eq!(run("head [7 8]", num(0), &[]), num(7));
        assert_eq!(run("tail [7 8]", num(0), &[]), num(8));
    }

    #[test]
    fn lets_and_if_and_eq() {
        assert_eq!(run("let x = 5 in +(x)", num(0), &[]), num(6));
        assert_eq!(run("if (1 == 1) then 100 else 200", num(0), &[]), num(100));
        assert_eq!(run("if (1 == 2) then 100 else 200", num(0), &[]), num(200));
        // nested let keeps outer faces reachable
        assert_eq!(run("let x = 3 in let y = 4 in [x y]", num(0), &[]),
                   cell(num(3), num(4)));
    }

    #[test]
    fn case_switch() {
        let src = "case t of %a -> 1 ; %b -> 2 ; _ -> 0 end";
        assert_eq!(run(src, crate::knot::cord("a"), &[("t", 1)]), num(1));
        assert_eq!(run(src, crate::knot::cord("b"), &[("t", 1)]), num(2));
        assert_eq!(run(src, crate::knot::cord("zz"), &[("t", 1)]), num(0));
    }

    #[test]
    fn errors_carry_source_position() {
        // missing `in` after a let binding, on line 3
        let src = "let x = 5\n\nlet y = 6 then";
        let e = compile(src, &[]).unwrap_err();
        assert!(e.contains("line 3"), "error lacked line info: {}", e);
        assert!(e.contains("'in'"), "error lacked expectation: {}", e);
        // unbound face names are reported by name
        let e2 = compile("nope", &[]).unwrap_err();
        assert!(e2.contains("nope"), "error lacked face name: {}", e2);
    }

    #[test]
    fn loop_add() {
        // add(a,b) by counting i up to b while incrementing acc
        let src = "loop with [acc = a, i = 0] : \
                   if (i == b) then acc else again(+(acc), +(i)) end";
        let s = cell(num(5), num(3)); // a@2 b@3
        assert_eq!(run(src, s, &[("a", 2), ("b", 3)]), num(8));
        let s2 = cell(num(0), num(1000));
        assert_eq!(run(src, s2, &[("a", 2), ("b", 3)]), num(1000));
    }

    #[test]
    fn loop_inside_let() {        // ensure peg-rebasing of loop ctx through an inner let is correct
        let src = "let base = a in \
                   loop with [acc = base, i = 0] : \
                   if (i == b) then acc else again(+(acc), +(i)) end";
        let s = cell(num(10), num(5));
        assert_eq!(run(src, s, &[("a", 2), ("b", 3)]), num(15));
    }
}

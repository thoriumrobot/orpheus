//! fmt — the Latte source formatter.
//!
//! A real formatter, not a whitespace trimmer: the source is lexed (comments
//! included), the author's LINE BREAKS are kept, and everything else is made
//! canonical —
//!
//!   * indentation is computed from the structure (a depth machine over the
//!     token stream: brackets, `case`/`loop`…`end`, the module frame, arm
//!     continuations, and hanging operators like a trailing `->`/`in`/`else`);
//!   * spacing between tokens follows fixed rules (tight brackets, spaced
//!     `=`/`==`/`->`, `, ` after commas, ` ; ` between case arms);
//!   * comments survive verbatim: own-line comments sit at the local indent,
//!     trailing comments stay on their line.
//!
//! SAFETY: formatting is provably meaning-preserving. The formatted text is
//! re-parsed and every arm's AST must be EQUAL to the original's (module name,
//! imports and arm order too); the comment count must match; otherwise the
//! original source is returned untouched. A formatter must never break — or
//! silently change — the program.

use crate::latte;

const UNIT: usize = 2; // one indentation step
const ARM_INDENT: usize = 2; // arms sit two spaces inside `core … end`

// ---------------------------------------------------------------------------
// lexing (formatter-private: keeps comments and the verbatim slice of every
// token, so number radix, escapes, and unicode all round-trip exactly)
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq)]
enum T {
    Word(String),    // identifier / keyword
    Num(String),     // verbatim literal (keeps 0x…/0b…/_ separators)
    Tag(String),     // %name (verbatim, with %)
    Str(String),     // "…" verbatim INCLUDING the quotes
    Open(char),      // ( [
    Close(char),     // ) ]
    Comma,
    Semi,
    Colon,
    Arrow,           // ->
    Assign,          // =
    Eq,              // ==
    Plus,            // + (always +(…))
    Comment(String), // :: … (text after ::, trimmed right)
}

#[derive(Debug, Clone)]
struct Item {
    tok: T,
    line: usize, // 0-based source line
}

fn lex(src: &str) -> Option<Vec<Item>> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut line = 0usize;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == ':' && i + 1 < b.len() && b[i + 1] == b':' {
            let s = i + 2;
            let mut j = s;
            while j < b.len() && b[j] != b'\n' {
                j += 1;
            }
            out.push(Item { tok: T::Comment(src[s..j].trim().to_string()), line });
            i = j;
            continue;
        }
        let start = i;
        match c {
            '"' => {
                let mut j = i + 1;
                loop {
                    if j >= b.len() || b[j] == b'\n' {
                        return None; // unterminated: not formattable
                    }
                    match b[j] {
                        b'"' => {
                            j += 1;
                            break;
                        }
                        b'\\' if j + 1 < b.len() => j += 2,
                        _ => j += 1,
                    }
                }
                out.push(Item { tok: T::Str(src[start..j].to_string()), line });
                i = j;
            }
            '(' | '[' => {
                out.push(Item { tok: T::Open(c), line });
                i += 1;
            }
            ')' | ']' => {
                out.push(Item { tok: T::Close(c), line });
                i += 1;
            }
            ',' => {
                out.push(Item { tok: T::Comma, line });
                i += 1;
            }
            ';' => {
                out.push(Item { tok: T::Semi, line });
                i += 1;
            }
            ':' => {
                out.push(Item { tok: T::Colon, line });
                i += 1;
            }
            '+' => {
                out.push(Item { tok: T::Plus, line });
                i += 1;
            }
            '-' => {
                if i + 1 < b.len() && b[i + 1] == b'>' {
                    out.push(Item { tok: T::Arrow, line });
                    i += 2;
                } else {
                    return None;
                }
            }
            '=' => {
                if i + 1 < b.len() && b[i + 1] == b'=' {
                    out.push(Item { tok: T::Eq, line });
                    i += 2;
                } else {
                    out.push(Item { tok: T::Assign, line });
                    i += 1;
                }
            }
            '%' => {
                let s = i + 1;
                let mut j = s;
                while j < b.len()
                    && (is_ident(b[j]) || b[j] == b'.' || b[j] == b'$' || b[j] == b':' || b[j] >= 0x80)
                {
                    j += 1;
                }
                if j == s {
                    return None;
                }
                out.push(Item { tok: T::Tag(src[start..j].to_string()), line });
                i = j;
            }
            _ if c.is_ascii_digit() => {
                let mut j = i;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                out.push(Item { tok: T::Num(src[start..j].to_string()), line });
                i = j;
            }
            _ if is_ident_start(b[i]) => {
                let mut j = i;
                while j < b.len() && is_ident(b[j]) {
                    j += 1;
                }
                out.push(Item { tok: T::Word(src[start..j].to_string()), line });
                i = j;
            }
            _ => return None,
        }
    }
    Some(out)
}
fn is_ident_start(b: u8) -> bool {
    (b as char).is_ascii_alphabetic() || b == b'_'
}
fn is_ident(b: u8) -> bool {
    (b as char).is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    Core,
    Case,
    Loop,
    Brack, // ( or [
}

/// Is this code line an arm head (`name = …` at module depth)?
fn line_is_arm_head(toks: &[&T]) -> bool {
    matches!(
        (toks.first(), toks.get(1)),
        (Some(T::Word(_)), Some(T::Assign))
    )
}

/// Render one source line's tokens with canonical spacing.
fn render_line(toks: &[&T]) -> String {
    let mut s = String::new();
    for (k, t) in toks.iter().enumerate() {
        let prev = if k > 0 { Some(toks[k - 1]) } else { None };
        let space = match (prev, *t) {
            (None, _) => false,
            // never before closers / separators
            (_, T::Close(_)) | (_, T::Comma) | (_, T::Semi) => false,
            // never after an opener or the Inc `+`
            (Some(T::Open(_)), _) | (Some(T::Plus), _) => false,
            // `again(` is one token-pair in the grammar: keep it tight
            (Some(T::Word(w)), T::Open('(')) if w == "again" => false,
            // everything else gets one space
            _ => true,
        };
        if space {
            s.push(' ');
        }
        match t {
            T::Word(w) => s.push_str(w),
            T::Num(n) => s.push_str(n),
            T::Tag(g) => s.push_str(g),
            T::Str(q) => s.push_str(q),
            T::Open(c) => s.push(*c),
            T::Close(c) => s.push(*c),
            T::Comma => s.push(','),
            T::Semi => s.push(';'),
            T::Colon => s.push(':'),
            T::Arrow => s.push_str("->"),
            T::Assign => s.push('='),
            T::Eq => s.push_str("=="),
            T::Plus => s.push('+'),
            T::Comment(c) => {
                s.push_str("::");
                if !c.is_empty() {
                    s.push(' ');
                    s.push_str(c);
                }
            }
        }
    }
    s
}

/// Format Latte source: canonical indentation and spacing, author line breaks
/// and comments preserved. Returns None when the source can't be safely
/// formatted (the caller falls back to the original).
///
/// Indentation = structural frames (core / case / loop / brackets) plus HANG
/// levels: a line ending in `->`, `=`, `in`, `then`, `else` or `of` opens a
/// hanging indent that persists for every following line at that depth or
/// deeper, and closes when the structure pops back out. This reproduces the
/// canonical Latte shape:
///
///   values = fn [l recipes iters] ->
///     loop with [v = l, k = 0] :
///       if (k == iters) then v
///       else again((step l recipes v), +(k))
///     end
pub fn format(src: &str) -> Option<String> {
    let items = lex(src)?;
    if items.is_empty() {
        return Some(if src.trim().is_empty() { String::new() } else { src.to_string() });
    }

    // group into source lines (a line = the items that share a source row)
    let last_line = items.iter().map(|i| i.line).max().unwrap_or(0);
    let mut rows: Vec<Vec<&Item>> = vec![Vec::new(); last_line + 1];
    for it in &items {
        rows[it.line].push(it);
    }

    let has_core = items.iter().any(|i| matches!(&i.tok, T::Word(w) if w == "core"));

    #[derive(Clone)]
    enum Line {
        Blank,
        CommentOnly(String),          // rendered comment, indent decided in pass 2
        Code(usize, String),          // (indent columns, rendered text incl. trailing comment)
    }

    let mut stack: Vec<Frame> = Vec::new();
    let mut hangs: Vec<usize> = Vec::new(); // hang levels: depths with an extra unit
    let mut in_arm = false;
    let mut lines: Vec<Line> = Vec::new();
    let mut blank_run = 0usize;

    for row in &rows {
        if row.is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !lines.is_empty() {
                lines.push(Line::Blank);
            }
            continue;
        }
        blank_run = 0;

        let code: Vec<&T> = row.iter().map(|i| &i.tok).filter(|t| !matches!(t, T::Comment(_))).collect();
        let comment: Option<&T> = row.iter().map(|i| &i.tok).find(|t| matches!(t, T::Comment(_)));

        if code.is_empty() {
            lines.push(Line::CommentOnly(render_line(&[comment.unwrap()])));
            continue;
        }

        // leading closers / `end`s un-indent the line they start
        let mut depth_for_line = stack.len();
        {
            let mut k = 0usize;
            while k < code.len() {
                match code[k] {
                    T::Close(_) => depth_for_line = depth_for_line.saturating_sub(1),
                    T::Word(w) if w == "end" => depth_for_line = depth_for_line.saturating_sub(1),
                    _ => break,
                }
                k += 1;
            }
        }
        hangs.retain(|&h| depth_for_line >= h); // structure popped out: hangs close

        let first = code[0];
        let module_kw = stack.is_empty() && matches!(first, T::Word(w) if w == "import" || w == "core");
        let is_head = stack.as_slice() == [Frame::Core] && line_is_arm_head(&code);
        let module_end = has_core && depth_for_line == 0 && matches!(first, T::Word(w) if w == "end");

        let indent_cols = if module_kw || module_end {
            0
        } else if is_head {
            in_arm = true;
            hangs.clear();
            ARM_INDENT
        } else if has_core {
            // inside a module: ARM_INDENT for the Core frame, one UNIT per
            // extra structural frame, hang level, and the arm continuation
            let frames_beyond_core = depth_for_line.saturating_sub(1);
            let arm = if in_arm { 1 } else { 0 };
            ARM_INDENT + UNIT * (frames_beyond_core + hangs.len() + arm)
        } else {
            UNIT * (depth_for_line + hangs.len())
        };
        if module_end {
            in_arm = false;
        }

        let mut toks: Vec<&T> = code.clone();
        if let Some(c) = comment {
            toks.push(c);
        }
        lines.push(Line::Code(indent_cols, render_line(&toks)));

        // advance the frame stack over this line's tokens
        for t in &code {
            match t {
                T::Open(_) => stack.push(Frame::Brack),
                T::Close(_) => {
                    if matches!(stack.last(), Some(Frame::Brack)) {
                        stack.pop();
                    }
                }
                T::Word(w) => match w.as_str() {
                    "core" => stack.push(Frame::Core),
                    "case" => stack.push(Frame::Case),
                    "loop" => stack.push(Frame::Loop),
                    "end" => match stack.last() {
                        Some(Frame::Case) | Some(Frame::Loop) => {
                            stack.pop();
                        }
                        Some(Frame::Core) => {
                            stack.pop();
                            in_arm = false;
                        }
                        _ => {}
                    },
                    _ => {}
                },
                _ => {}
            }
        }
        hangs.retain(|&h| stack.len() >= h);

        // a line ending in a hanging operator opens a hang at the current depth
        // (`of`/`:` are not hangers — the Case/Loop frames carry their
        // bodies; `in` is not a hanger — Latte let-chains are FLAT, the body
        // sits level with its lets; an arm head's `->` is carried by the arm
        // continuation)
        let hanger = matches!(code.last(), Some(T::Arrow) | Some(T::Assign))
            || matches!(code.last(), Some(T::Word(w)) if w == "then" || w == "else");
        if hanger && !is_head && hangs.last() != Some(&stack.len()) {
            hangs.push(stack.len());
        }
    }

    // pass 2: a comment line sits at the indent of the next code line
    // (or the previous one at the file tail)
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        match line {
            Line::Blank => out.push(String::new()),
            Line::Code(ind, text) => out.push(format!("{}{}", " ".repeat(*ind), text)),
            Line::CommentOnly(text) => {
                let near = lines[i + 1..]
                    .iter()
                    .find_map(|l| match l {
                        Line::Code(ind, _) => Some(*ind),
                        _ => None,
                    })
                    .or_else(|| {
                        lines[..i].iter().rev().find_map(|l| match l {
                            Line::Code(ind, _) => Some(*ind),
                            _ => None,
                        })
                    })
                    .unwrap_or(0);
                out.push(format!("{}{}", " ".repeat(near), text));
            }
        }
    }

    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut text = out.join("\n");
    text.push('\n');
    Some(text)
}

// ---------------------------------------------------------------------------
// the public entry: format with the meaning-preservation proof
// ---------------------------------------------------------------------------
pub fn format_checked(src: &str) -> String {
    let formatted = match format(src) {
        Some(f) => f,
        None => return src.to_string(),
    };
    if formatted == src {
        return formatted;
    }
    if !equivalent(src, &formatted) {
        return src.to_string(); // never change meaning; never break the program
    }
    formatted
}

/// Provable equivalence: same module shape and IDENTICAL per-arm ASTs (or, for
/// a bare expression, an identical AST), plus the same number of comments.
fn equivalent(a: &str, b: &str) -> bool {
    let comments = |s: &str| s.lines().filter(|l| l.contains("::")).count();
    if comments(a) != comments(b) {
        return false;
    }
    match (latte::parse_program(a), latte::parse_program(b)) {
        (Ok(pa), Ok(pb)) => pa == pb,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_canonically() {
        let src = "core t\n  f=fn [n]->if (n==0) then 1 else (mul n (f (dec n)))\nend\n";
        let f = format_checked(src);
        assert!(f.contains("  f = fn [n] -> if (n == 0) then 1 else (mul n (f (dec n)))"), "{}", f);
    }

    #[test]
    fn preserves_meaning_on_every_shipped_library() {
        for name in latte::builtin_lib_names() {
            let src = latte::library_source(&name).unwrap();
            let f = format_checked(&src);
            assert!(equivalent(&src, &f), "{} changed meaning", name);
            // idempotent: formatting a formatted source is a fixed point
            assert_eq!(f, format_checked(&f), "{} not idempotent", name);
        }
    }

    #[test]
    fn comments_survive() {
        let src = "core t\n  :: the answer\n  a = fn [_] -> 42  :: trailing\nend\n";
        let f = format_checked(src);
        assert!(f.contains(":: the answer"), "{}", f);
        assert!(f.contains(":: trailing"), "{}", f);
    }

    #[test]
    fn strings_and_radix_round_trip() {
        let src = "core t\n  s = fn [_] -> (cat \"a \\\"b\\\"\" %x)\n  h = fn [_] -> 0xFF\nend\n";
        let f = format_checked(src);
        assert!(f.contains("\"a \\\"b\\\"\""), "{}", f);
        assert!(f.contains("0xFF"), "{}", f);
    }

    #[test]
    fn broken_source_returned_unchanged() {
        let src = "core t\n  f = fn [n] -> (mul n\nend\n";
        assert_eq!(format_checked(src), src);
    }
}

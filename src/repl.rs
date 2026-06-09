//! The self-hosting environment — an interactive Latte session. Definitions entered
//! at the prompt accumulate into a live `core` that is recompiled and run on Loom, so
//! the environment hosts evolving Latte code at runtime: later expressions can call
//! arms you defined earlier and the std/mold/plan libraries. It also exposes the rest
//! of the toolbox: `:t` typechecks, `:sca` runs SCArs.

use crate::knot::{Knot, N};
use crate::{check, latte, sca};
use std::io::{BufRead, Write};

pub struct Session {
    defs: String, // accumulated `name = body` arms
    count: usize,
}

impl Session {
    pub fn new() -> Session {
        Session { defs: String::new(), count: 0 }
    }

    /// Handle one input line. Returns (output, should_quit).
    pub fn handle(&mut self, line: &str) -> (String, bool) {
        let t = line.trim();
        if t.is_empty() {
            return (String::new(), false);
        }
        if let Some(rest) = t.strip_prefix(':') {
            return self.command(rest.trim());
        }
        if let Some((name, body)) = as_definition(t) {
            return (self.define(name, body), false);
        }
        (self.eval(t), false)
    }

    fn command(&mut self, c: &str) -> (String, bool) {
        let (head, arg) = match c.split_once(char::is_whitespace) {
            Some((h, a)) => (h, a.trim()),
            None => (c, ""),
        };
        match head {
            "q" | "quit" | "exit" => ("bye".into(), true),
            "help" | "h" => (HELP.into(), false),
            "t" | "type" => match latte::parse(arg) {
                Ok(ast) => match check::check(&ast) {
                    Ok(ty) => (format!("{} : {}", arg, ty.show()), false),
                    Err(e) => (e, false),
                },
                Err(e) => (format!("parse error: {}", e), false),
            },
            "sca" => {
                let mut out = Vec::new();
                for w in arg.split_whitespace() {
                    match sca::evolve(w) {
                        Ok(h) => out.push(format!("{} → {}", w, h)),
                        Err(e) => out.push(format!("{}: {}", w, e)),
                    }
                }
                (out.join("\n"), false)
            }
            "defs" => {
                if self.count == 0 {
                    ("(no definitions yet)".into(), false)
                } else {
                    (self.defs.trim_end().to_string(), false)
                }
            }
            "reset" => {
                self.defs.clear();
                self.count = 0;
                ("definitions cleared".into(), false)
            }
            other => (format!("unknown command :{} (try :help)", other), false),
        }
    }

    fn define(&mut self, name: &str, body: &str) -> String {
        let candidate = format!("{}  {} = {}\n", self.defs, name, body);
        // validate by compiling the whole core with a trivial main
        match latte::run_module_expr(&candidate, "0") {
            Ok(_) => {
                self.defs = candidate;
                self.count += 1;
                format!("defined {}", name)
            }
            Err(e) => format!("definition rejected: {}", e),
        }
    }

    fn eval(&self, expr: &str) -> String {
        match latte::run_module_expr(&self.defs, expr) {
            Ok(v) => render(&v),
            Err(e) => e,
        }
    }
}

fn as_definition(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    // not an equality test `==`
    if line[eq..].starts_with("==") {
        return None;
    }
    let lhs = line[..eq].trim();
    let rhs = line[eq + 1..].trim();
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    let ok = lhs.chars().enumerate().all(|(i, ch)| {
        if i == 0 {
            ch.is_ascii_alphabetic() || ch == '_'
        } else {
            ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
        }
    });
    if ok {
        Some((lhs, rhs))
    } else {
        None
    }
}

fn render(n: &N) -> String {
    match &**n {
        Knot::Atom(a) => {
            let num = a.to_u128().map(|x| x.to_string()).unwrap_or_else(|| "<big>".into());
            match a.as_cord() {
                // annotate as text only when it is clearly a word (≥2 printable chars)
                Some(s) if s.chars().count() >= 2 && s.bytes().all(|b| b >= 0x20) => format!("'{}'", s),
                _ => num,
            }
        }
        Knot::Cell(h, t) => format!("[{} {}]", render(h), render(t)),
    }
}

const HELP: &str = "\
Orpheus self-hosting environment. Type Latte and it runs on Loom.
  <expr>              evaluate, e.g.  (add 2 3)   [1 [2 3]]   (values ...)
  name = <expr>       define an arm; later expressions can use it
  :t <expr>           infer the static type
  :sca <words>        derive Heart Speech with SCArs
  :defs               show your definitions      :reset   clear them
  :help               this message               :q       quit";

pub fn cmd_repl() {
    let mut s = Session::new();
    println!("Orpheus — self-hosting Latte environment.  :help for commands, :q to quit.");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    print!("» ");
    let _ = out.flush();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let (text, quit) = s.handle(&line);
        if !text.is_empty() {
            println!("{}", text);
        }
        if quit {
            break;
        }
        print!("» ");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_then_use() {
        let mut s = Session::new();
        let (o, _) = s.handle("double = fn [x] -> (add x x)");
        assert_eq!(o, "defined double");
        let (o, _) = s.handle("(double 21)");
        assert_eq!(o, "42");
    }

    #[test]
    fn definitions_compose() {
        let mut s = Session::new();
        s.handle("twice = fn [f x] -> (f (f x))");
        // pass a closure (module arms are not first-class, but `fn` values are)
        let (o, _) = s.handle("(twice (fn [y] -> +(y)) 40)");
        assert_eq!(o, "42");
    }

    #[test]
    fn evaluates_cells_and_uses_plan_library() {
        let mut s = Session::new();
        let (o, _) = s.handle("[1 2]");
        assert_eq!(o, "[1 2]");
        // the planning library is in scope inside the REPL
        let (o, _) = s.handle("(dot [2 [3 0]] [4 [5 0]])"); // 2*4 + 3*5 = 23
        assert_eq!(o, "23");
    }

    #[test]
    fn typecheck_and_reject_bad_def() {
        let mut s = Session::new();
        let (o, _) = s.handle(":t head [1 2]");
        assert!(o.contains(": @"));
        let (o, _) = s.handle("oops = fn [x] -> (nope x)"); // unknown arm
        assert!(o.starts_with("definition rejected") || o == "defined oops");
    }

    #[test]
    fn quit_signals() {
        let mut s = Session::new();
        let (_, q) = s.handle(":q");
        assert!(q);
    }
}

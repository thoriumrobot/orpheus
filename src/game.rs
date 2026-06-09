//! game — a tool for boardgames where the players are *machines*: independent Orpheus cores,
//! each holding its own compiled program (rules + a move chooser), queried turn by turn. Chess
//! is the first game. The match driver is game-agnostic: any Latte game exposing `initial`,
//! `legal`, `incheck`, `choose`, and `apply` can be played. Two modes:
//!   * machine vs machine (default): both sides play automatically;
//!   * human vs machine (`--human white|black`): you type moves, the machine answers using the
//!     ML-learned evaluator (see lib/chessml.lat).

use crate::knot::{cell, num, Knot, N};
use crate::latte;
use std::io::Write;

// ---- noun helpers ----------------------------------------------------------
fn head(n: &N) -> Option<N> {
    if let Knot::Cell(h, _) = &**n { Some(h.clone()) } else { None }
}
fn tail(n: &N) -> Option<N> {
    if let Knot::Cell(_, t) = &**n { Some(t.clone()) } else { None }
}
fn u(n: &N) -> u128 {
    n.as_atom().and_then(|a| a.to_u128()).unwrap_or(0)
}
fn list_u(n: &N) -> Vec<u128> {
    let mut v = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        v.push(u(h));
        cur = t.clone();
    }
    v
}
// walk a list of moves [from [to 0]] into (from,to) pairs
fn move_list(n: &N) -> Vec<(u128, u128)> {
    let mut out = Vec::new();
    let mut cur = n.clone();
    while let Knot::Cell(h, t) = &*cur {
        if let (Some(f), Some(rest)) = (head(h), tail(h)) {
            if let Some(to) = head(&rest) {
                out.push((u(&f), u(&to)));
            }
        }
        cur = t.clone();
    }
    out
}

// ---- a machine: a compiled program instance --------------------------------
struct Machine {
    core: N,
    axes: Vec<(String, u128)>,
}
impl Machine {
    fn new(libs: &[&str]) -> Result<Machine, String> {
        let (core, axes) = latte::compile_library_program(libs)?;
        Ok(Machine { core, axes })
    }
    fn call(&self, arm: &str, args: N) -> Result<N, String> {
        latte::call_arm(&self.core, &self.axes, arm, args)
    }
}

// ---- chess rendering -------------------------------------------------------
fn glyph(p: u128) -> char {
    match p {
        1 => 'P', 2 => 'N', 3 => 'B', 4 => 'R', 5 => 'Q', 6 => 'K',
        7 => 'p', 8 => 'n', 9 => 'b', 10 => 'r', 11 => 'q', 12 => 'k',
        _ => '.',
    }
}
fn square(i: u128) -> String {
    format!("{}{}", (b'a' + (i % 8) as u8) as char, 8 - i / 8)
}
fn parse_square(s: &str) -> Option<u128> {
    let b = s.as_bytes();
    if b.len() != 2 { return None; }
    let c = b[0].wrapping_sub(b'a');
    let r = b[1].wrapping_sub(b'1');
    if c > 7 || r > 7 { return None; }
    Some((7 - r as u128) * 8 + c as u128)
}
fn render(board: &[u128]) -> String {
    let mut s = String::new();
    for r in 0..8u128 {
        s.push_str(&format!("  {} ", 8 - r));
        for c in 0..8u128 {
            s.push(glyph(*board.get((r * 8 + c) as usize).unwrap_or(&0)));
            s.push(' ');
        }
        s.push('\n');
    }
    s.push_str("    a b c d e f g h\n");
    s
}
fn material(board: &[u128]) -> (i64, i64) {
    let val = |t: u128| match t { 1 => 1, 2 => 3, 3 => 3, 4 => 5, 5 => 9, _ => 0 };
    let (mut w, mut b) = (0i64, 0i64);
    for &p in board {
        if p == 0 { continue; }
        if p < 7 { w += val(p); } else { b += val(p - 6); }
    }
    (w, b)
}
fn side_name(side: u128) -> &'static str { if side == 1 { "White" } else { "Black" } }
fn other(side: u128) -> u128 { if side == 1 { 2 } else { 1 } }

enum Control { Greedy, Ml, Human }

pub fn cmd_game(args: &[String]) {
    let mut game = "chess".to_string();
    let mut maxply = 200usize;
    let mut show_every = 0usize;
    let mut human: Option<u128> = None; // side the human plays
    let mut ml_iters = 400u128;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--max" => { i += 1; if i < args.len() { maxply = args[i].parse().unwrap_or(200); } }
            "--show" => { i += 1; if i < args.len() { show_every = args[i].parse().unwrap_or(0); } }
            "--iters" => { i += 1; if i < args.len() { ml_iters = args[i].parse().unwrap_or(400); } }
            "--human" => {
                i += 1;
                if i < args.len() {
                    human = Some(if args[i].starts_with('b') { 2 } else { 1 });
                }
            }
            other => game = other.to_string(),
        }
        i += 1;
    }
    if game != "chess" {
        eprintln!("game: only 'chess' is implemented (try: latte game chess)");
        return;
    }

    // rules referee + greedy player (chess), and the ML player (chess + chessml)
    let rules = match Machine::new(&["std", "chess"]) { Ok(m) => m, Err(e) => { eprintln!("game: {}", e); return; } };
    let ml = match Machine::new(&["std", "chess", "num", "chessml"]) { Ok(m) => m, Err(e) => { eprintln!("game: {}", e); return; } };

    // train the ML evaluator (in Latte) once; reuse the learned weights all game
    let weights = match ml.call("trained", num(ml_iters)) {
        Ok(w) => w,
        Err(e) => { eprintln!("game: training failed: {}", e); return; }
    };

    let control = |side: u128| -> Control {
        match human {
            Some(h) if h == side => Control::Human,
            Some(_) => Control::Ml,         // the machine opponent uses the learned model
            None => Control::Greedy,        // machine vs machine
        }
    };
    let pname = |side: u128| -> String {
        match human {
            Some(h) if h == side => "You".to_string(),
            Some(_) => "Minerva (ML)".to_string(),
            None => if side == 1 { "Talos".to_string() } else { "Daedalus".to_string() },
        }
    };

    let mut state = match rules.call("initial", num(0)) { Ok(s) => s, Err(e) => { eprintln!("game: {}", e); return; } };

    println!("Orpheus boardgame tool — chess");
    if human.is_some() {
        println!("You are {}. The machine plays the learned model (piece values trained in Latte, {} iters).",
            side_name(human.unwrap()), ml_iters);
        println!("Enter moves like  e2e4 . Type  quit  to resign.\n");
    } else {
        println!("players: White = {}, Black = {}\n", pname(1), pname(2));
    }
    print!("{}", render(&list_u(&head(&state).unwrap())));
    println!();

    let stdin = std::io::stdin();
    let mut ply = 0usize;
    let result;
    loop {
        let board = list_u(&head(&state).unwrap());
        let side = u(&head(&tail(&state).unwrap()).unwrap());

        if ply >= maxply {
            let (w, b) = material(&board);
            result = match w.cmp(&b) {
                std::cmp::Ordering::Greater => format!("move cap — White ahead {}–{} on material", w, b),
                std::cmp::Ordering::Less => format!("move cap — Black ahead {}–{} on material", b, w),
                std::cmp::Ordering::Equal => format!("move cap — level {}–{}, draw", w, b),
            };
            break;
        }

        // obtain this side's move
        let mv: Option<N> = match control(side) {
            Control::Greedy => {
                let m = rules.call("choose", state.clone()).unwrap_or_else(|_| num(0));
                if m.as_atom().is_some() { None } else { Some(m) }
            }
            Control::Ml => {
                let m = ml.call("choose_ml", cell(state.clone(), weights.clone())).unwrap_or_else(|_| num(0));
                if m.as_atom().is_some() { None } else { Some(m) }
            }
            Control::Human => {
                let legal = move_list(&rules.call("legal", state.clone()).unwrap_or_else(|_| num(0)));
                if legal.is_empty() {
                    None
                } else {
                    loop {
                        print!("your move ({}): ", side_name(side));
                        let _ = std::io::stdout().flush();
                        let mut line = String::new();
                        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                            println!("(end of input)");
                            return;
                        }
                        let t = line.trim();
                        if t == "quit" || t == "resign" {
                            println!("\nYou resigned. {} wins.", pname(other(side)));
                            return;
                        }
                        if t.len() >= 4 {
                            if let (Some(f), Some(to)) = (parse_square(&t[0..2]), parse_square(&t[2..4])) {
                                if legal.iter().any(|&(lf, lt)| lf == f && lt == to) {
                                    break Some(cell(num(f), cell(num(to), num(0))));
                                }
                            }
                        }
                        println!("  illegal move. legal examples: {}",
                            legal.iter().take(6).map(|&(f, t)| format!("{}{}", square(f), square(t))).collect::<Vec<_>>().join(" "));
                    }
                }
            }
        };

        let mv = match mv {
            Some(m) => m,
            None => {
                let chk = u(&rules.call("incheck", state.clone()).unwrap_or_else(|_| num(1)));
                result = if chk == 0 {
                    format!("checkmate — {} ({}) wins in {} plies", pname(other(side)), side_name(other(side)), ply)
                } else {
                    format!("stalemate — draw after {} plies", ply)
                };
                break;
            }
        };

        let from = u(&head(&mv).unwrap());
        let to = u(&head(&tail(&mv).unwrap()).unwrap());
        let captured = board.get(to as usize).copied().unwrap_or(0);
        let cap = if captured != 0 { format!(" x{}", glyph(captured)) } else { String::new() };
        println!("{:>3}. {:<13} {}{}{}", ply + 1, pname(side) + if side == 1 { " (W)" } else { " (B)" }, square(from), square(to), cap);

        let args = cell(head(&state).unwrap(), cell(num(side), mv));
        state = match rules.call("apply", args) { Ok(s) => s, Err(e) => { eprintln!("apply failed: {}", e); return; } };
        ply += 1;
        let show = show_every != 0 && ply % show_every == 0;
        if show || human.is_some() {
            println!();
            print!("{}", render(&list_u(&head(&state).unwrap())));
            println!();
        }
    }

    println!();
    print!("{}", render(&list_u(&head(&state).unwrap())));
    println!("\n{}", result);
}

#[cfg(test)]
mod tests {
    use crate::latte;
    fn val(expr: &str) -> u128 {
        latte::run_with_libs(expr, &["std", "chess"]).unwrap().as_atom().unwrap().to_u128().unwrap()
    }
    fn board(side: u128, pieces: &[(usize, u128)]) -> String {
        let mut sq = [0u128; 64];
        for &(i, p) in pieces { sq[i] = p; }
        let mut s = String::from("[ [ ");
        for v in sq { s.push_str(&v.to_string()); s.push(' '); }
        s.push_str(&format!("0 ] [ {} 0 ] ]", side));
        s
    }
    #[test]
    fn chess_opening_is_legal() {
        assert_eq!(val("(len (legal (initial 0)))"), 20);
        assert_eq!(val("(status (initial 0))"), 0);
    }
    #[test]
    fn chess_detects_checkmate() {
        let m = board(2, &[(7, 12), (14, 5), (22, 6)]);
        assert_eq!(val(&format!("(len (legal {}))", m)), 0);
        assert_eq!(val(&format!("(status {})", m)), 1);
    }
    #[test]
    fn chess_detects_stalemate() {
        let s = board(2, &[(7, 12), (13, 6), (22, 5)]);
        assert_eq!(val(&format!("(len (legal {}))", s)), 0);
        assert_eq!(val(&format!("(status {})", s)), 2);
    }
    #[test]
    fn chess_chooser_finds_mate_in_one() {
        let p = board(1, &[(7, 12), (8, 5), (22, 6)]);
        assert_eq!(val(&format!("(nth (choose {}) 0)", p)), 8);
        assert_eq!(val(&format!("(nth (choose {}) 1)", p)), 14);
        assert_eq!(val(&format!("(status (apply (nth {} 0) 1 (choose {})))", p, p)), 1);
    }
    // the ML evaluator learns sensible piece values (Q > R > minor > pawn)
    #[test]
    fn chessml_learns_piece_ordering() {
        let w = |k: usize| -> u128 {
            latte::run_with_libs(&format!("(nth (trained 300) {})", k), &["std", "chess", "num", "chessml"])
                .unwrap();
            // weights are signed [sign mag]; read the magnitude
            latte::run_with_libs(&format!("(nmag (nth (trained 300) {}))", k), &["std", "chess", "num", "chessml"])
                .unwrap().as_atom().unwrap().to_u128().unwrap()
        };
        let (p, n, b, r, q) = (w(0), w(1), w(2), w(3), w(4));
        assert!(q > r && r > n && n >= p, "expected Q>R>N>=P, got P{} N{} B{} R{} Q{}", p, n, b, r, q);
        assert!(b > p, "bishop should outweigh pawn");
    }
}

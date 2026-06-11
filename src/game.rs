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
use std::sync::OnceLock;

// ---- a cached opponent for the GUI -----------------------------------------
// The board UI asks the server to read the position and play moves. Compiling the chess
// program (and training the evaluator) is done once and reused; the interpreter path is
// fast and unbounded, unlike the node's fuel-bounded peek, so it is the source of truth
// for legality, status, and the model's replies.
fn rules_engine() -> Option<&'static Machine> {
    static R: OnceLock<Option<Machine>> = OnceLock::new();
    R.get_or_init(|| Machine::new(&["std", "chess"]).ok()).as_ref()
}

struct Ai {
    ml: Machine,
    weights: N,
}
fn ai() -> Option<&'static Ai> {
    static A: OnceLock<Option<Ai>> = OnceLock::new();
    A.get_or_init(|| {
        let ml = Machine::new(&["std", "chess", "num", "chessml"]).ok()?;
        let weights = ml.call("trained", num(250)).ok()?;
        Some(Ai { ml, weights })
    })
    .as_ref()
}

/// Legal moves of `state` (`[board [side 0]]`) as (from,to) pairs, via the unbounded engine.
pub fn legal_moves(state: &N) -> Vec<(u128, u128)> {
    rules_engine()
        .and_then(|r| r.call("legal", state.clone()).ok())
        .map(|n| move_list(&n))
        .unwrap_or_default()
}

/// Game status of `state`: 0 ongoing, 1 checkmate (side to move is mated), 2 stalemate.
pub fn status_of(state: &N) -> u128 {
    rules_engine()
        .and_then(|r| r.call("status", state.clone()).ok())
        .map(|n| u(&n))
        .unwrap_or(0)
}

/// The board squares (64 piece codes) and side to move of `state`.
pub fn board_side(state: &N) -> (Vec<u128>, u128) {
    let board = head(state).map(|b| list_u(&b)).unwrap_or_default();
    let side = tail(state).and_then(|t| head(&t)).map(|s| u(&s)).unwrap_or(1);
    (board, side)
}

/// Choose a move for the side to move in `state`. With `ml`, use the Latte-learned evaluator
/// (Minerva); otherwise the fast greedy chooser. `None` = no move (game over). Used by
/// the GUI's `/api/chess` to play "against the model".
pub fn ai_move(state: &N, ml: bool) -> Option<N> {
    if ml {
        // "play the learned model": the Latte ML evaluator at 2-ply (kept as the
        // documented machine-learning experiment, not the strongest opponent)
        let a = ai()?;
        let mv = a
            .ml
            .call("choose_ab", cell(state.clone(), cell(a.weights.clone(), num(2))))
            .ok()?;
        return if mv.as_atom().is_some() { None } else { Some(mv) };
    }
    // THE ENGINE: iterative-deepening alpha-beta + quiescence + piece-square
    // evaluation (src/game.rs above) — the real opponent. Its move is verified
    // against the Latte rules' legal list, so the two implementations check
    // each other; on any disagreement we fall back to the Latte chooser.
    let (board, side) = board_side(state);
    let legal = legal_moves(state);
    if let Some((f, t)) = engine_move(&board, side, 5) {
        if legal.iter().any(|&(lf, lt)| lf == f && lt == t) {
            return Some(cell(num(f), cell(num(t), num(0))));
        }
    }
    let mv = rules_engine()?.call("choose", state.clone()).ok()?;
    if mv.as_atom().is_some() {
        None
    } else {
        Some(mv)
    }
}

// ============================================================================
// THE ENGINE. A real chess engine: iterative-deepening alpha-beta with
// quiescence search, MVV-LVA move ordering, and a material + piece-square
// evaluation — replacing the 1-2 ply greedy/ML choosers as the default
// opponent. It plays the SAME rules as lib/chess.lat (single/double pawn
// pushes, diagonal captures, auto-queen promotion, no castling or en passant),
// and every move it returns is verified against the Latte `legal` list, so the
// two implementations police each other. The Latte ML evaluator remains
// available as the "play the learned model" mode.
// ============================================================================

type Board = [i8; 64]; // 0 empty; White 1..6 = P N B R Q K; Black 7..12

fn b_of(list: &[u128]) -> Board {
    let mut b = [0i8; 64];
    for (i, &p) in list.iter().take(64).enumerate() {
        b[i] = p as i8;
    }
    b
}
fn color(p: i8) -> i8 {
    if p == 0 { 0 } else if p < 7 { 1 } else { 2 }
}
fn kind(p: i8) -> i8 {
    if p == 0 { 0 } else if p < 7 { p } else { p - 6 }
}
const VAL: [i32; 7] = [0, 100, 320, 330, 500, 900, 20000];

// piece-square tables (White's view; mirrored for Black) — classic midgame sets
const PST_P: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 50, 50, 50, 50, 50, 50, 50, 50, 10, 10, 20, 30, 30, 20, 10, 10,
    5, 5, 10, 25, 25, 10, 5, 5, 0, 0, 0, 20, 20, 0, 0, 0, 5, -5, -10, 0, 0, -10, -5, 5,
    5, 10, 10, -20, -20, 10, 10, 5, 0, 0, 0, 0, 0, 0, 0, 0,
];
const PST_N: [i32; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, -40, -20, 0, 0, 0, 0, -20, -40,
    -30, 0, 10, 15, 15, 10, 0, -30, -30, 5, 15, 20, 20, 15, 5, -30,
    -30, 0, 15, 20, 20, 15, 0, -30, -30, 5, 10, 15, 15, 10, 5, -30,
    -40, -20, 0, 5, 5, 0, -20, -40, -50, -40, -30, -30, -30, -30, -40, -50,
];
const PST_B: [i32; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10,
    -10, 0, 5, 10, 10, 5, 0, -10, -10, 5, 5, 10, 10, 5, 5, -10,
    -10, 0, 10, 10, 10, 10, 0, -10, -10, 10, 10, 10, 10, 10, 10, -10,
    -10, 5, 0, 0, 0, 0, 5, -10, -20, -10, -10, -10, -10, -10, -10, -20,
];
const PST_R: [i32; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 5, 10, 10, 10, 10, 10, 10, 5, -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5,
    -5, 0, 0, 0, 0, 0, 0, -5, 0, 0, 0, 5, 5, 0, 0, 0,
];
const PST_Q: [i32; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10,
    -10, 0, 5, 5, 5, 5, 0, -10, -5, 0, 5, 5, 5, 5, 0, -5, 0, 0, 5, 5, 5, 5, 0, -5,
    -10, 5, 5, 5, 5, 5, 0, -10, -10, 0, 5, 0, 0, 0, 0, -10, -20, -10, -10, -5, -5, -10, -10, -20,
];
const PST_K: [i32; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30,
    -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30,
    -20, -30, -30, -40, -40, -30, -30, -20, -10, -20, -20, -20, -20, -20, -20, -10,
    20, 20, 0, 0, 0, 0, 20, 20, 20, 30, 10, 0, 0, 10, 30, 20,
];

fn pst(k: i8, sq: usize, white: bool) -> i32 {
    let i = if white { sq } else { 63 - sq }; // mirror ranks AND files (tables are symmetric enough)
    match k {
        1 => PST_P[i],
        2 => PST_N[i],
        3 => PST_B[i],
        4 => PST_R[i],
        5 => PST_Q[i],
        6 => PST_K[i],
        _ => 0,
    }
}

/// Static evaluation from WHITE's view (centipawns).
fn eval(b: &Board) -> i32 {
    let mut e = 0;
    for (sq, &p) in b.iter().enumerate() {
        if p == 0 {
            continue;
        }
        let k = kind(p);
        let v = VAL[k as usize] + pst(k, sq, color(p) == 1);
        e += if color(p) == 1 { v } else { -v };
    }
    e
}

/// Is `sq` attacked by `by` (1 white / 2 black)? Mirrors chess.lat geometry.
fn attacked(b: &Board, sq: usize, by: i8) -> bool {
    let (r, c) = ((sq / 8) as i32, (sq % 8) as i32);
    let at = |rr: i32, cc: i32| -> i8 {
        if (0..8).contains(&rr) && (0..8).contains(&cc) { b[(rr * 8 + cc) as usize] } else { -1 }
    };
    // pawns: white pawns attack toward row-1 (up), so a square is attacked by white
    // pawns sitting at row+1; by black pawns sitting at row-1
    let pr = if by == 1 { r + 1 } else { r - 1 };
    for dc in [-1i32, 1] {
        let p = at(pr, c + dc);
        if p > 0 && color(p) == by && kind(p) == 1 {
            return true;
        }
    }
    // knights
    for (dr, dc) in [(-2, -1), (-2, 1), (-1, -2), (-1, 2), (1, -2), (1, 2), (2, -1), (2, 1)] {
        let p = at(r + dr, c + dc);
        if p > 0 && color(p) == by && kind(p) == 2 {
            return true;
        }
    }
    // king
    for dr in -1..=1i32 {
        for dc in -1..=1i32 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let p = at(r + dr, c + dc);
            if p > 0 && color(p) == by && kind(p) == 6 {
                return true;
            }
        }
    }
    // sliders
    for (dr, dc, diag) in [
        (-1i32, 0i32, false), (1, 0, false), (0, -1, false), (0, 1, false),
        (-1, -1, true), (-1, 1, true), (1, -1, true), (1, 1, true),
    ] {
        let (mut rr, mut cc) = (r + dr, c + dc);
        while (0..8).contains(&rr) && (0..8).contains(&cc) {
            let p = b[(rr * 8 + cc) as usize];
            if p != 0 {
                if color(p) == by {
                    let k = kind(p);
                    if k == 5 || (diag && k == 3) || (!diag && k == 4) {
                        return true;
                    }
                }
                break;
            }
            rr += dr;
            cc += dc;
        }
    }
    false
}

fn king_sq(b: &Board, side: i8) -> usize {
    let kp = if side == 1 { 6 } else { 12 };
    b.iter().position(|&p| p == kp).unwrap_or(64.min(63))
}
fn in_check(b: &Board, side: i8) -> bool {
    attacked(b, king_sq(b, side), if side == 1 { 2 } else { 1 })
}

/// Apply (from,to) with auto-queen promotion; returns the new board.
fn make(b: &Board, from: usize, to: usize) -> Board {
    let mut nb = *b;
    let mut p = nb[from];
    if kind(p) == 1 {
        let tr = to / 8;
        if (color(p) == 1 && tr == 0) || (color(p) == 2 && tr == 7) {
            p = if color(p) == 1 { 5 } else { 11 };
        }
    }
    nb[to] = p;
    nb[from] = 0;
    nb
}

/// Pseudo-legal moves for `side` (chess.lat rules), captures first (MVV-LVA).
fn gen_moves(b: &Board, side: i8) -> Vec<(usize, usize)> {
    let mut caps: Vec<(i32, usize, usize)> = Vec::new();
    let mut quiet: Vec<(usize, usize)> = Vec::new();
    let mut push = |b: &Board, from: usize, to: usize| {
        let victim = b[to];
        if victim != 0 {
            caps.push((VAL[kind(victim) as usize] * 10 - VAL[kind(b[from]) as usize], from, to));
        } else {
            quiet.push((from, to));
        }
    };
    for from in 0..64usize {
        let p = b[from];
        if p == 0 || color(p) != side {
            continue;
        }
        let (r, c) = ((from / 8) as i32, (from % 8) as i32);
        let k = kind(p);
        let ok = |rr: i32, cc: i32| (0..8).contains(&rr) && (0..8).contains(&cc);
        match k {
            1 => {
                let dir: i32 = if side == 1 { -1 } else { 1 };
                let start = if side == 1 { 6 } else { 1 };
                if ok(r + dir, c) && b[((r + dir) * 8 + c) as usize] == 0 {
                    push(b, from, ((r + dir) * 8 + c) as usize);
                    if r == start && b[((r + 2 * dir) * 8 + c) as usize] == 0 {
                        push(b, from, ((r + 2 * dir) * 8 + c) as usize);
                    }
                }
                for dc in [-1i32, 1] {
                    if ok(r + dir, c + dc) {
                        let t = ((r + dir) * 8 + c + dc) as usize;
                        if b[t] != 0 && color(b[t]) != side {
                            push(b, from, t);
                        }
                    }
                }
            }
            2 | 6 => {
                let deltas: &[(i32, i32)] = if k == 2 {
                    &[(-2, -1), (-2, 1), (-1, -2), (-1, 2), (1, -2), (1, 2), (2, -1), (2, 1)]
                } else {
                    &[(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]
                };
                for &(dr, dc) in deltas {
                    if ok(r + dr, c + dc) {
                        let t = ((r + dr) * 8 + c + dc) as usize;
                        if b[t] == 0 || color(b[t]) != side {
                            push(b, from, t);
                        }
                    }
                }
            }
            3 | 4 | 5 => {
                let dirs: &[(i32, i32)] = match k {
                    3 => &[(-1, -1), (-1, 1), (1, -1), (1, 1)],
                    4 => &[(-1, 0), (1, 0), (0, -1), (0, 1)],
                    _ => &[(-1, -1), (-1, 1), (1, -1), (1, 1), (-1, 0), (1, 0), (0, -1), (0, 1)],
                };
                for &(dr, dc) in dirs {
                    let (mut rr, mut cc) = (r + dr, c + dc);
                    while ok(rr, cc) {
                        let t = (rr * 8 + cc) as usize;
                        if b[t] == 0 {
                            push(b, from, t);
                        } else {
                            if color(b[t]) != side {
                                push(b, from, t);
                            }
                            break;
                        }
                        rr += dr;
                        cc += dc;
                    }
                }
            }
            _ => {}
        }
    }
    caps.sort_by(|a, b2| b2.0.cmp(&a.0));
    let mut out: Vec<(usize, usize)> = caps.into_iter().map(|(_, f, t)| (f, t)).collect();
    out.extend(quiet);
    out
}

/// Quiescence: stand pat, then captures only (prevents horizon blunders).
fn qsearch(b: &Board, side: i8, mut alpha: i32, beta: i32, nodes: &mut u64) -> i32 {
    *nodes += 1;
    let stand = if side == 1 { eval(b) } else { -eval(b) };
    if stand >= beta {
        return beta;
    }
    if stand > alpha {
        alpha = stand;
    }
    for (f, t) in gen_moves(b, side) {
        if b[t] == 0 {
            break; // captures are ordered first
        }
        let nb = make(b, f, t);
        if in_check(&nb, side) {
            continue;
        }
        let score = -qsearch(&nb, 3 - side, -beta, -alpha, nodes);
        if score >= beta {
            return beta;
        }
        if score > alpha {
            alpha = score;
        }
    }
    alpha
}

fn alphabeta(b: &Board, side: i8, depth: u32, mut alpha: i32, beta: i32, nodes: &mut u64) -> i32 {
    if depth == 0 {
        return qsearch(b, side, alpha, beta, nodes);
    }
    *nodes += 1;
    let mut any = false;
    for (f, t) in gen_moves(b, side) {
        let nb = make(b, f, t);
        if in_check(&nb, side) {
            continue; // own king left in check: illegal
        }
        any = true;
        let score = -alphabeta(&nb, 3 - side, depth - 1, -beta, -alpha, nodes);
        if score >= beta {
            return beta;
        }
        if score > alpha {
            alpha = score;
        }
    }
    if !any {
        // mate or stalemate
        return if in_check(b, side) { -30000 + (5 - depth as i32) } else { 0 };
    }
    alpha
}

/// The engine's move for `side`: iterative deepening to `max_depth`, stopping
/// early after ~1.2s. Returns (from, to) or None when no legal move exists.
pub fn engine_move(board_list: &[u128], side: u128, max_depth: u32) -> Option<(u128, u128)> {
    let b = b_of(board_list);
    let side = side as i8;
    let t0 = std::time::Instant::now();
    let mut best: Option<(usize, usize)> = None;
    let mut nodes = 0u64;
    for depth in 1..=max_depth {
        let mut alpha = -40000;
        let beta = 40000;
        let mut local: Option<(usize, usize)> = None;
        // try the previous iteration's best move first (cheap PV ordering)
        let mut moves = gen_moves(&b, side);
        if let Some(pv) = best {
            if let Some(pos) = moves.iter().position(|&m| m == pv) {
                moves.swap(0, pos);
            }
        }
        for (f, t) in moves {
            let nb = make(&b, f, t);
            if in_check(&nb, side) {
                continue;
            }
            let score = -alphabeta(&nb, 3 - side, depth - 1, -beta, -alpha, &mut nodes);
            if score > alpha {
                alpha = score;
                local = Some((f, t));
            }
        }
        if local.is_some() {
            best = local;
        }
        if t0.elapsed().as_millis() > 1200 {
            break;
        }
    }
    best.map(|(f, t)| (f as u128, t as u128))
}

// ============================================================================
// XIANGQI. The same architecture, server-local: rules in lib/xiangqi.lat on an
// unbounded Machine; the opponent is the TRAINED MODEL of lib/xiangqiml.lat
// (a linear evaluator whose piece values are learned by gradient descent in
// Latte) searching 2 plies of alpha-beta. State lives in a process-wide slot.
// ============================================================================

fn xq_rules() -> Option<&'static Machine> {
    static R: OnceLock<Option<Machine>> = OnceLock::new();
    R.get_or_init(|| Machine::new(&["std", "xiangqi"]).ok()).as_ref()
}

struct XqAi {
    ml: Machine,
    weights: N,
}
fn xq_ai() -> Option<&'static XqAi> {
    static A: OnceLock<Option<XqAi>> = OnceLock::new();
    A.get_or_init(|| {
        let ml = Machine::new(&["std", "xiangqi", "num", "xiangqiml"]).ok()?;
        let weights = ml.call("xtrained", num(250)).ok()?;
        Some(XqAi { ml, weights })
    })
    .as_ref()
}

fn xq_slot() -> &'static std::sync::Mutex<Option<N>> {
    static S: OnceLock<std::sync::Mutex<Option<N>>> = OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(None))
}
fn xq_current() -> Option<N> {
    let mut g = xq_slot().lock().unwrap();
    if g.is_none() {
        *g = xq_rules()?.call("xqinitial", num(0)).ok();
    }
    g.clone()
}

/// The learned piece values, for display: [soldier horse elephant chariot cannon advisor] ×1000.
pub fn xq_learned_values() -> Vec<i128> {
    let a = match xq_ai() {
        Some(a) => a,
        None => return Vec::new(),
    };
    // weights are signed nums [sign mag]
    let mut out = Vec::new();
    let mut cur = a.weights.clone();
    while let crate::knot::Knot::Cell(h, t) = &*cur {
        if let crate::knot::Knot::Cell(sg, mag) = &**h {
            let sgn = sg.as_atom().and_then(|x| x.to_u128()).unwrap_or(0);
            let m = mag.as_atom().and_then(|x| x.to_u128()).unwrap_or(0) as i128;
            out.push(if sgn == 0 { m } else { -m });
        }
        cur = t.clone();
    }
    out
}

/// Handle a /api/xiangqi verb; returns the JSON state afterwards.
pub fn xq_command(verb: &str, f: Option<u128>, t: Option<u128>) -> String {
    match verb {
        "new" => {
            *xq_slot().lock().unwrap() = xq_rules().and_then(|r| r.call("xqinitial", num(0)).ok());
        }
        "move" => {
            if let (Some(f), Some(t), Some(st), Some(r)) = (f, t, xq_current(), xq_rules()) {
                // only apply moves the Latte rules call legal
                let legal = r
                    .call("xqlegal", st.clone())
                    .map(|n| move_list(&n))
                    .unwrap_or_default();
                if legal.iter().any(|&(lf, lt)| lf == f && lt == t) {
                    let (board, side) = xq_board_side(&st);
                    let bn = vec_to_list(&board);
                    let mv = cell(num(f), cell(num(t), num(0)));
                    if let Ok(ns) = r.call("xqapply", cell(bn, cell(num(side), mv))) {
                        *xq_slot().lock().unwrap() = Some(ns);
                    }
                }
            }
        }
        "ai" => {
            if let (Some(st), Some(r)) = (xq_current(), xq_rules()) {
                if let Some((f, t2)) = xq_model_move(&st) {
                    let (board, side) = xq_board_side(&st);
                    let bn = vec_to_list(&board);
                    let mvn = cell(num(f), cell(num(t2), num(0)));
                    if let Ok(ns) = r.call("xqapply", cell(bn, cell(num(side), mvn))) {
                        *xq_slot().lock().unwrap() = Some(ns);
                    }
                }
            }
        }
        _ => {}
    }
    xq_json()
}

/// The trained model's move. The MODEL is the Latte-learned evaluator of
/// lib/xiangqiml.lat (piece values fit by gradient descent on Loom, once per
/// process); the SEARCH runs natively here at 4 plies, because a 90-square
/// alpha-beta on the Loom interpreter exceeds any reasonable fuel budget and
/// Anvil compiles per-expression (a fresh board literal would mean a fresh
/// native build every move). Rules mirror lib/xiangqi.lat exactly, and the
/// returned move is verified against the Latte `xqlegal` list — the two
/// implementations police each other, as with chess.
fn xq_model_move(state: &N) -> Option<(u128, u128)> {
    let (board, side) = xq_board_side(state);
    let legal = xq_rules()?
        .call("xqlegal", state.clone())
        .map(|n| move_list(&n))
        .unwrap_or_default();
    if legal.is_empty() {
        return None;
    }
    let w = xq_learned_values(); // the TRAINED weights (×1000), from Latte
    let weights: [i32; 6] = if w.len() == 6 {
        [w[0] as i32, w[1] as i32, w[2] as i32, w[3] as i32, w[4] as i32, w[5] as i32]
    } else {
        [1000, 4000, 2000, 9000, 4500, 2000]
    };
    let b = xqb_of(&board);
    if let Some((f, t)) = xq_search(&b, side as i8, 4, &weights) {
        if legal.iter().any(|&(lf, lt)| lf == f as u128 && lt == t as u128) {
            return Some((f as u128, t as u128));
        }
    }
    // fall back to the first Latte-legal move (never returns an illegal one)
    legal.first().copied()
}

// ---- the native xiangqi search (rules mirror lib/xiangqi.lat) ---------------
type XqBoard = [i8; 90];
fn xqb_of(list: &[u128]) -> XqBoard {
    let mut b = [0i8; 90];
    for (i, &p) in list.iter().take(90).enumerate() {
        b[i] = p as i8;
    }
    b
}
fn xqcolor(p: i8) -> i8 {
    if p == 0 { 0 } else if p < 8 { 1 } else { 2 }
}
fn xqkind(p: i8) -> i8 {
    if p == 0 { 0 } else if p < 8 { p } else { p - 7 }
}
/// model evaluation from White's view: the LEARNED piece values (general = huge)
fn xqeval(b: &XqBoard, w: &[i32; 6]) -> i32 {
    let mut e = 0i32;
    for &p in b.iter() {
        if p == 0 {
            continue;
        }
        let k = xqkind(p);
        let v = if k == 7 { 1_000_000 } else { w[(k - 1) as usize] };
        e += if xqcolor(p) == 1 { v } else { -v };
    }
    e
}
fn xq_onboard(r: i32, c: i32) -> bool {
    (0..10).contains(&r) && (0..9).contains(&c)
}
fn xq_inpalace(side: i8, r: i32, c: i32) -> bool {
    (3..=5).contains(&c) && if side == 1 { (7..=9).contains(&r) } else { (0..=2).contains(&r) }
}
fn xq_ownside(side: i8, r: i32) -> bool {
    if side == 1 { r >= 5 } else { r <= 4 }
}
fn xq_moves(b: &XqBoard, side: i8) -> Vec<(usize, usize)> {
    let mut caps: Vec<(i32, usize, usize)> = Vec::new();
    let mut quiet: Vec<(usize, usize)> = Vec::new();
    let at = |r: i32, c: i32| -> i8 { if xq_onboard(r, c) { b[(r * 9 + c) as usize] } else { -1 } };
    let mut push = |from: usize, r: i32, c: i32| {
        if !xq_onboard(r, c) {
            return;
        }
        let to = (r * 9 + c) as usize;
        let v = b[to];
        if v != 0 && xqcolor(v) == side {
            return;
        }
        if v != 0 {
            caps.push((xqkind(v) as i32 * 10 - xqkind(b[from]) as i32, from, to));
        } else {
            quiet.push((from, to));
        }
    };
    for from in 0..90usize {
        let p = b[from];
        if p == 0 || xqcolor(p) != side {
            continue;
        }
        let (r, c) = ((from / 9) as i32, (from % 9) as i32);
        match xqkind(p) {
            1 => {
                // soldier: forward; sideways after crossing the river
                let dir: i32 = if side == 1 { -1 } else { 1 };
                push(from, r + dir, c);
                let crossed = if side == 1 { r <= 4 } else { r >= 5 };
                if crossed {
                    push(from, r, c - 1);
                    push(from, r, c + 1);
                }
            }
            2 => {
                // horse with leg blocks
                for (lr, lc, mr, mc) in [
                    (-1i32, 0i32, -2i32, -1i32), (-1, 0, -2, 1), (1, 0, 2, -1), (1, 0, 2, 1),
                    (0, -1, -1, -2), (0, -1, 1, -2), (0, 1, -1, 2), (0, 1, 1, 2),
                ] {
                    if at(r + lr, c + lc) == 0 {
                        push(from, r + mr, c + mc);
                    }
                }
            }
            3 => {
                // elephant: 2-diagonal, eye clear, own side of the river
                for (dr, dc) in [(-2i32, -2i32), (-2, 2), (2, -2), (2, 2)] {
                    if at(r + dr / 2, c + dc / 2) == 0 && xq_onboard(r + dr, c + dc) && xq_ownside(side, r + dr) {
                        push(from, r + dr, c + dc);
                    }
                }
            }
            4 => {
                // chariot
                for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (mut rr, mut cc) = (r + dr, c + dc);
                    while xq_onboard(rr, cc) {
                        push(from, rr, cc);
                        if b[(rr * 9 + cc) as usize] != 0 {
                            break;
                        }
                        rr += dr;
                        cc += dc;
                    }
                }
            }
            5 => {
                // cannon: slide while empty; capture over exactly one screen
                for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (mut rr, mut cc) = (r + dr, c + dc);
                    let mut jumped = false;
                    while xq_onboard(rr, cc) {
                        let v = b[(rr * 9 + cc) as usize];
                        if !jumped {
                            if v == 0 {
                                push(from, rr, cc);
                            } else {
                                jumped = true;
                            }
                        } else if v != 0 {
                            if xqcolor(v) != side {
                                push(from, rr, cc);
                            }
                            break;
                        }
                        rr += dr;
                        cc += dc;
                    }
                }
            }
            6 => {
                // advisor: 1 diagonal within the palace
                for (dr, dc) in [(-1i32, -1i32), (-1, 1), (1, -1), (1, 1)] {
                    if xq_inpalace(side, r + dr, c + dc) {
                        push(from, r + dr, c + dc);
                    }
                }
            }
            7 => {
                // general: 1 orthogonal within the palace
                for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    if xq_inpalace(side, r + dr, c + dc) {
                        push(from, r + dr, c + dc);
                    }
                }
            }
            _ => {}
        }
    }
    caps.sort_by(|a, b2| b2.0.cmp(&a.0));
    let mut out: Vec<(usize, usize)> = caps.into_iter().map(|(_, f, t)| (f, t)).collect();
    out.extend(quiet);
    out
}
fn xq_make(b: &XqBoard, f: usize, t: usize) -> XqBoard {
    let mut nb = *b;
    nb[t] = nb[f];
    nb[f] = 0;
    nb
}
fn xq_general(b: &XqBoard, side: i8) -> Option<usize> {
    let g = if side == 1 { 7 } else { 14 };
    b.iter().position(|&p| p == g)
}
fn xq_facing(b: &XqBoard) -> bool {
    if let (Some(wg), Some(bg)) = (xq_general(b, 1), xq_general(b, 2)) {
        if wg % 9 == bg % 9 {
            let c = wg % 9;
            let (lo, hi) = (bg / 9, wg / 9);
            return ((lo + 1)..hi).all(|r| b[r * 9 + c] == 0);
        }
    }
    false
}
fn xq_ab(b: &XqBoard, side: i8, depth: u32, mut alpha: i32, beta: i32, w: &[i32; 6]) -> i32 {
    // captured-general / flying-general terminals (pseudo-move search)
    if xq_general(b, 1).is_none() {
        return if side == 1 { -2_000_000 } else { 2_000_000 };
    }
    if xq_general(b, 2).is_none() {
        return if side == 1 { 2_000_000 } else { -2_000_000 };
    }
    if depth == 0 {
        let e = xqeval(b, w);
        return if side == 1 { e } else { -e };
    }
    let mut any = false;
    for (f, t) in xq_moves(b, side) {
        let nb = xq_make(b, f, t);
        if xq_facing(&nb) {
            continue; // never create a flying-general position
        }
        any = true;
        let v = -xq_ab(&nb, 3 - side, depth - 1, -beta, -alpha, w);
        if v >= beta {
            return beta;
        }
        if v > alpha {
            alpha = v;
        }
    }
    if !any {
        return -1_500_000; // stuck loses in xiangqi
    }
    alpha
}
fn xq_search(b: &XqBoard, side: i8, depth: u32, w: &[i32; 6]) -> Option<(usize, usize)> {
    let mut best = None;
    let mut alpha = -3_000_000;
    let beta = 3_000_000;
    for (f, t) in xq_moves(b, side) {
        let nb = xq_make(b, f, t);
        if xq_facing(&nb) {
            continue;
        }
        let v = -xq_ab(&nb, 3 - side, depth - 1, -beta, -alpha, w);
        if v > alpha {
            alpha = v;
            best = Some((f, t));
        }
    }
    best
}

fn xq_board_side(state: &N) -> (Vec<u128>, u128) {
    let board = head(state).map(|b| list_u(&b)).unwrap_or_default();
    let side = tail(state).and_then(|t| head(&t)).map(|s| u(&s)).unwrap_or(1);
    (board, side)
}

fn vec_to_list(v: &[u128]) -> N {
    let mut acc = num(0);
    for x in v.iter().rev() {
        acc = cell(num(*x), acc);
    }
    acc
}

pub fn xq_json() -> String {
    let st = match xq_current() {
        Some(s) => s,
        None => return "{\"error\":\"xiangqi engine unavailable\"}".into(),
    };
    let (board, side) = xq_board_side(&st);
    let r = xq_rules();
    let status = r
        .and_then(|r| r.call("xqstatus", st.clone()).ok())
        .map(|n| u(&n))
        .unwrap_or(0);
    let legal: Vec<(u128, u128)> = r
        .and_then(|r| r.call("xqlegal", st.clone()).ok())
        .map(|n| move_list(&n))
        .unwrap_or_default();
    let vals = xq_learned_values();
    let board_s = board.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    let legal_s = legal.iter().map(|(f, t)| format!("[{},{}]", f, t)).collect::<Vec<_>>().join(",");
    let vals_s = vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    format!(
        "{{\"board\":[{}],\"side\":{},\"status\":{},\"legal\":[{}],\"learned\":[{}]}}",
        board_s, side, status, legal_s, vals_s
    )
}

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
    fn chess_ab_search_finds_mate_and_legal_moves() {
        // run with the learned evaluator + search
        let valml = |expr: &str| {
            latte::run_with_libs(expr, &["std", "chess", "num", "chessml"])
                .unwrap_or_else(|e| panic!("EXPR <{}> ERR <{}>", expr, e))
                .as_atom()
                .unwrap()
                .to_u128()
                .unwrap()
        };
        // mate in one: bK a8, wK c7, wQ b1, White to move → Qb7#. A 2-ply alpha-beta search
        // (cheap here — only three pieces) must return a real move (a cell) whose resulting
        // position is checkmate. (Deeper searches from dense positions use the compiled engine,
        // which is far faster than this reference interpreter.)
        let s = board(1, &[(0, 12), (10, 6), (57, 5)]);
        assert_eq!(valml(&format!("(iscell (choose_ab {0} (trained 20) 2))", s)), 0);
        let mate = format!("(status (apply (nth {0} 0) 1 (choose_ab {0} (trained 20) 2)))", s);
        assert_eq!(valml(&mate), 1, "alpha-beta search should find the mate in one");
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

    // ---- the GUI-facing engine (used by /api/chess) ------------------------
    use super::{ai_move, board_side, legal_moves, status_of};

    #[test]
    fn gui_engine_reads_opening_and_moves() {
        let st = latte::run_with_libs("(initial 0)", &["std", "chess"]).unwrap();
        let (board, side) = board_side(&st);
        assert_eq!(board.len(), 64);
        assert_eq!(side, 1);
        assert_eq!(status_of(&st), 0);
        let legal = legal_moves(&st);
        assert_eq!(legal.len(), 20);
        // the model picks a legal move for the side to move
        let mv = ai_move(&st, false).expect("greedy move from the opening");
        let from = super::u(&super::head(&mv).unwrap());
        let to = super::u(&super::head(&super::tail(&mv).unwrap()).unwrap());
        assert!(legal.iter().any(|&(f, t)| f == from && t == to), "ai move {}-{} must be legal", from, to);
    }

    #[test]
    fn chessgame_app_validates_and_applies() {
        // The networked board's rules are the Latte Mocha app (lib/chessgame.lat).
        crate::latte::register_runtime_lib("chessgame", crate::mocha::CHESSGAME_LAT);
        let libs = &["std", "chess", "chessgame"];
        // an empty node (state 0) is the opening: 20 legal moves, ongoing
        assert_eq!(
            latte::run_with_libs("(len (peek [%legal 0] 0))", libs).unwrap().as_atom().unwrap().to_u128().unwrap(),
            20
        );
        // e2e4 (52->36) applies: side flips to 2 and the pawn lands on 36
        let side = latte::run_with_libs("(nth (tail (poke [%move [52 [36 0]]] 0)) 1)", libs)
            .unwrap().as_atom().unwrap().to_u128().unwrap();
        assert_eq!(side, 2);
        let at36 = latte::run_with_libs("(nth (nth (tail (poke [%move [52 [36 0]]] 0)) 0) 36)", libs)
            .unwrap().as_atom().unwrap().to_u128().unwrap();
        assert_eq!(at36, 1);
        // an illegal move (52->20) is a no-op: still White to move
        let side2 = latte::run_with_libs("(nth (tail (poke [%move [52 [20 0]]] 0)) 1)", libs)
            .unwrap().as_atom().unwrap().to_u128().unwrap();
        assert_eq!(side2, 1);
    }

    #[test]
    fn engine_finds_mate_in_one() {
        // back-rank: White Ra1, Kg1 vs Black Kg8 (pawns f7 g7 h7) — Ra1-a8#
        let mut b = vec![0u128; 64];
        b[6] = 12; // black king g8
        b[13] = 7; b[14] = 7; b[15] = 7; // f7 g7 h7 black pawns
        b[56] = 4; // white rook a1
        b[62] = 6; // white king g1
        let (f, t) = super::engine_move(&b, 1, 4).expect("a move");
        assert_eq!((f, t), (56, 0), "expected Ra1-a8 mate, got {} -> {}", f, t);
    }

    #[test]
    fn engine_takes_the_hanging_queen() {
        // White Qd1 can take an undefended Black queen on d8
        let mut b = vec![0u128; 64];
        b[3] = 11; // black queen d8
        b[7] = 12; // black king h8 (NOT defending d8 — the queen truly hangs)
        b[59] = 5; // white queen d1
        b[60] = 6; // white king e1
        b[8] = 7; b[9] = 7; // a7 b7 pawns so it isn't immediately mate-hunting
        let (f, t) = super::engine_move(&b, 1, 4).expect("a move");
        assert_eq!((f, t), (59, 3), "expected Qxd8, got {} -> {}", f, t);
    }

    #[test]
    fn engine_moves_are_always_latte_legal() {
        // play the engine against itself from the start; every move must be in
        // the Latte rules' legal list (the two rule implementations agree)
        let mut state = super::rules_engine().unwrap().call("initial", crate::knot::num(0)).unwrap();
        for _ply in 0..12 {
            let legal = super::legal_moves(&state);
            if legal.is_empty() {
                break;
            }
            let (board, side) = super::board_side(&state);
            let (f, t) = super::engine_move(&board, side, 3).expect("engine move");
            assert!(
                legal.iter().any(|&(lf, lt)| lf == f && lt == t),
                "engine move {}->{} not in the Latte legal list",
                f,
                t
            );
            state = super::rules_engine()
                .unwrap()
                .call("apply", crate::knot::cell(crate::knot::num(f), crate::knot::cell(crate::knot::num(t), state.clone())))
                .unwrap_or(state);
        }
    }

    #[test]
    fn xq_trained_model_plays_a_legal_opening_move() {
        let st = super::xq_rules().expect("rules machine").call("xqinitial", crate::knot::num(0)).unwrap();
        let (f, t) = super::xq_model_move(&st).expect("a model move (Anvil or Loom fallback)");
        let legal = super::xq_rules().unwrap().call("xqlegal", st).map(|n| super::move_list(&n)).unwrap();
        assert!(legal.iter().any(|&(lf, lt)| lf == f && lt == t), "model move {}->{} not legal", f, t);
    }

    #[test]
    fn xq_self_play_stays_latte_legal() {
        // the native searcher (driven by the Latte-trained weights) must always
        // produce moves the Latte rules call legal, for both sides
        let r = super::xq_rules().unwrap();
        let mut st = r.call("xqinitial", crate::knot::num(0)).unwrap();
        for _ply in 0..8 {
            let legal = r.call("xqlegal", st.clone()).map(|n| super::move_list(&n)).unwrap();
            if legal.is_empty() {
                break;
            }
            let (f, t) = super::xq_model_move(&st).expect("model move");
            assert!(legal.iter().any(|&(lf, lt)| lf == f && lt == t), "{}->{} not legal", f, t);
            let (board, side) = super::xq_board_side(&st);
            let bn = super::vec_to_list(&board);
            let mv = crate::knot::cell(crate::knot::num(f), crate::knot::cell(crate::knot::num(t), crate::knot::num(0)));
            st = r.call("xqapply", crate::knot::cell(bn, crate::knot::cell(crate::knot::num(side), mv))).unwrap();
        }
    }
}

//! Game Oracle interface and wire protocol executor (`dregg_automatafl_rules` compatible).
//!
//! Routes canonical wire queries (`stock`, `goals`, `sense`, `step`, `mid`, `turn`, `adjudicate`,
//! `dist`, `legal`, `targets`, `livetargets`, `clash`, `round`) and generalized `GameConfig`
//! variants (`preset`, `sensecfg`, `stepcfg`, `midcfg`, `landedcfg`, `roundcfg`) directly to the
//! formally verified Lean 4 core (`automatafl_lean::rules_wire`).

use std::sync::OnceLock;

use crate::protocol::{
    ATT, AUTO, AutomatonSense, Board, BoardPreset, ConfluenceMode, Coord, Decision, GameConfig,
    Move, REP, Raycast, RoundOutcome, Sightline, Standing, TieBreakMode, VAC, coords_from_wire,
    coords_wire, goals_wire, moves_from_wire, moves_wire, pids_from_wire, pids_wire,
};

type FfiWireFn = fn(&str) -> Result<String, String>;

static LEAN_FFI_HANDLER: OnceLock<FfiWireFn> = OnceLock::new();

/// Register the compiled Lean 4 FFI wire entry (`dregg_automatafl_rules`).
pub fn register_lean_ffi(handler: FfiWireFn) {
    let _ = LEAN_FFI_HANDLER.set(handler);
}

/// Dispatch a wire string to the linked Lean FFI handler if registered, or execute the
/// compiled Lean 4 C FFI (`automatafl_lean::rules_wire`) by default.
pub fn call_wire(wire: &str) -> Result<String, String> {
    if let Some(f) = LEAN_FFI_HANDLER.get() {
        return f(wire);
    }
    Ok(automatafl_lean::rules_wire(wire))
}

/// Canonical `AutomataflFFI.rulesWire` evaluation delegated directly to `automatafl-lean`.
pub fn eval_wire_canonical(wire: &str) -> String {
    automatafl_lean::rules_wire(wire)
}

/// Ask the oracle and return the payload tokens with the leading `"1"` stripped.
pub fn ask(wire: &str) -> Result<Vec<String>, String> {
    let out = call_wire(wire)?;
    let mut toks = out.split_whitespace();
    match toks.next() {
        Some("1") => Ok(toks.map(str::to_owned).collect()),
        Some("0") => Err(format!(
            "dregg_automatafl_rules REFUSED the wire (fail-closed): {wire:?}"
        )),
        other => Err(format!(
            "dregg_automatafl_rules returned a malformed reply {other:?} for {wire:?}"
        )),
    }
}

fn board_wire(b: &Board) -> Result<String, String> {
    b.validate()?;
    Ok(b.to_wire())
}

fn refs(toks: &[String]) -> Vec<&str> {
    toks.iter().map(String::as_str).collect()
}

fn board_of(toks: &[String], what: &str) -> Result<Board, String> {
    let r = refs(toks);
    let (b, rest) = Board::from_wire(&r)?;
    if !rest.is_empty() {
        return Err(format!("{what}: {} trailing tokens", rest.len()));
    }
    Ok(b)
}

fn encode_opt_dir(dir: Option<Coord>) -> &'static str {
    match dir {
        None => "0",
        Some((dx, _)) if dx > 0 => "1",
        Some((dx, _)) if dx < 0 => "2",
        Some((_, dy)) if dy > 0 => "3",
        Some((_, dy)) if dy < 0 => "4",
        _ => "0",
    }
}

/// The stock 11×11 two-player opening board (`AutomataflRules.stockTwoPlayer`).
pub fn stock_two_player() -> Result<Board, String> {
    board_of(&ask("stock")?, "stock")
}

/// Opening board for a `BoardPreset` and `GameConfig`.
pub fn stock_board_for_config(cfg: &GameConfig) -> Result<Board, String> {
    let col_rule = matches!(cfg.tie_break, TieBreakMode::Column);
    let mut b = match cfg.preset {
        BoardPreset::Classic11x11 => stock_two_player()?,
        BoardPreset::Tactical9x9 => board_of(&ask("preset 1")?, "preset 1")?,
        BoardPreset::Blitz7x7 => board_of(&ask("preset 0")?, "preset 0")?,
        BoardPreset::Mini5x5 => build_mini_5x5(col_rule),
    };
    b.col_rule = col_rule;
    Ok(b)
}

fn mk_board(size: usize, placed: &[(Coord, u8)], auto: Coord, col_rule: bool) -> Board {
    let mut cells = vec![VAC; size * size];
    for &(c, p) in placed {
        if c.0 >= 0 && (c.0 as usize) < size && c.1 >= 0 && (c.1 as usize) < size {
            cells[(c.1 as usize) * size + (c.0 as usize)] = p;
        }
    }
    cells[(auto.1 as usize) * size + (auto.0 as usize)] = AUTO;
    Board {
        n: size,
        cells,
        auto,
        col_rule,
    }
}

fn build_mini_5x5(col_rule: bool) -> Board {
    let placed = [
        ((0, 1), ATT),
        ((1, 0), REP),
        ((4, 3), ATT),
        ((3, 4), REP),
    ];
    mk_board(5, &placed, (2, 2), col_rule)
}

/// Stock two-player goal assignment (`AutomataflRules.stockGoals2`):
/// Seat 0 owns `(0, 0)` and `(size - 1, 0)`; Seat 1 owns `(0, size - 1)` and `(size - 1, size - 1)`.
pub fn stock_goals2(size: usize) -> Result<Vec<(Coord, u32)>, String> {
    let toks = ask(&format!("goals {size}"))?;
    let r = refs(&toks);
    let (k, mut rest) = match r.split_first() {
        Some((kt, rest)) => (
            kt.parse::<usize>()
                .map_err(|e| format!("goal count {kt:?}: {e}"))?,
            rest,
        ),
        None => return Err("goals reply is empty".into()),
    };
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        if rest.len() < 3 {
            return Err("goals reply is truncated".into());
        }
        let x: i32 = rest[0].parse().map_err(|e| format!("goal x: {e}"))?;
        let y: i32 = rest[1].parse().map_err(|e| format!("goal y: {e}"))?;
        let seat: u32 = rest[2].parse().map_err(|e| format!("goal seat: {e}"))?;
        out.push(((x, y), seat));
        rest = &rest[3..];
    }
    Ok(out)
}

fn parse_sense_tokens(toks: &[String]) -> Result<AutomatonSense, String> {
    if toks.len() != 18 {
        return Err(format!("sense reply has {} tokens, want 18", toks.len()));
    }
    let n = |i: usize| -> Result<usize, String> {
        toks[i]
            .parse::<usize>()
            .map_err(|e| format!("sense field {i} ({:?}): {e}", toks[i]))
    };
    let z = |i: usize| -> Result<i32, String> {
        toks[i]
            .parse::<i32>()
            .map_err(|e| format!("sense field {i} ({:?}): {e}", toks[i]))
    };
    let ray = |i: usize| -> Result<Raycast, String> {
        Ok(Raycast {
            what: n(i)? as u8,
            dist: n(i + 1)?,
        })
    };
    let dec = |i: usize| -> Result<Decision, String> {
        Ok(Decision {
            variant: n(i)? as u8,
            pos: n(i + 1)? != 0,
            att_dist: n(i + 2)?,
            rep_dist: n(i + 3)?,
        })
    };
    Ok(AutomatonSense {
        rays: [ray(0)?, ray(2)?, ray(4)?, ray(6)?],
        x_dec: dec(8)?,
        y_dec: dec(12)?,
        offset: (z(16)?, z(17)?),
    })
}

/// The automaton's whole decision on `b` (`sense TIE BOARD`).
pub fn sense(b: &Board) -> Result<AutomatonSense, String> {
    sense_with_tie(b, b.tie_token())
}

pub fn sense_with_tie(b: &Board, tie_tok: &str) -> Result<AutomatonSense, String> {
    let effective_tok = match tie_tok {
        "0" | "1" | "2" | "3" | "4" => tie_tok,
        _ => b.tie_token(),
    };
    let toks = ask(&format!("sense {} {}", effective_tok, board_wire(b)?))?;
    parse_sense_tokens(&toks)
}

pub fn sense_with_config(
    b: &Board,
    tb: TieBreakMode,
    last_auto_step: Option<Coord>,
) -> Result<AutomatonSense, String> {
    let toks = ask(&format!(
        "sensecfg {} {} {}",
        tb.wire_token(),
        encode_opt_dir(last_auto_step),
        board_wire(b)?
    ))?;
    parse_sense_tokens(&toks)
}

/// The automaton step (`step TIE BOARD`).
pub fn automaton_step(b: &Board) -> Result<Board, String> {
    board_of(
        &ask(&format!("step {} {}", b.tie_token(), board_wire(b)?))?,
        "step",
    )
}

/// The move-resolved intermediate board (`mid BOARD COORDS MOVES`).
pub fn resolve_mid(b: &Board, marks: &[Coord], ms: &[Move]) -> Result<Board, String> {
    board_of(
        &ask(&format!(
            "mid {} {} {}",
            board_wire(b)?,
            coords_wire(marks),
            moves_wire(ms)
        ))?,
        "mid",
    )
}

/// Config-aware move resolution (`midcfg GAME_CONFIG BOARD COORDS MOVES`).
pub fn resolve_moves_cfg(cfg: &GameConfig, b: &Board, ms: &[Move]) -> Board {
    let Ok(bw) = board_wire(b) else {
        return b.clone();
    };
    ask(&format!(
        "midcfg {} {} 0 {}",
        cfg.to_wire(),
        bw,
        moves_wire(ms)
    ))
    .and_then(|toks| board_of(&toks, "midcfg"))
    .unwrap_or_else(|_| b.clone())
}

/// Step the automaton `cfg.automata_count.max(1)` times on `b` via Lean's `round_with_config`,
/// returning `(stepped_board, winner_opt, next_step_dir)`.
pub fn step_automata_cfg(
    cfg: &GameConfig,
    goals: &[(Coord, u32)],
    b: &Board,
    last_step: Option<Coord>,
) -> (Board, Option<u32>, Option<Coord>) {
    let mut cfg1 = cfg.clone();
    cfg1.automata_count = 1;
    let mut cur_b = b.clone();
    let mut cur_d = last_step;
    let k = cfg.automata_count.max(1);
    for _ in 0..k {
        let Ok((RoundOutcome::Resolved { board: nxt_b, win }, _)) =
            round_with_config(&cfg1, cur_d, &cur_b, goals, &[], &[], &[0], &[])
        else {
            break;
        };
        let dx = (nxt_b.auto.0 - cur_b.auto.0).signum();
        let dy = (nxt_b.auto.1 - cur_b.auto.1).signum();
        if dx != 0 || dy != 0 {
            cur_d = Some((dx, dy));
        }
        cur_b = nxt_b;
        if win.is_some() {
            return (cur_b, win, cur_d);
        }
    }
    (cur_b, None, cur_d)
}

/// One clean round (`turn TIE BOARD GOALS COORDS MOVES`).
pub fn turn(
    b: &Board,
    marks: &[Coord],
    ms: &[Move],
    goals: &[(Coord, u32)],
) -> Result<(Board, Option<u32>), String> {
    let toks = ask(&format!(
        "turn {} {} {} {} {}",
        b.tie_token(),
        board_wire(b)?,
        goals_wire(goals),
        coords_wire(marks),
        moves_wire(ms)
    ))?;
    let (win_tok, rest) = toks
        .split_first()
        .ok_or_else(|| "turn reply is empty".to_string())?;
    let win: i64 = win_tok
        .parse()
        .map_err(|e| format!("turn win token {win_tok:?}: {e}"))?;
    let board = board_of(rest, "turn")?;
    Ok((board, if win < 0 { None } else { Some(win as u32) }))
}

/// Capped-match terminal rule (`adjudicate BOARD GOALS`).
pub fn adjudicate_capped(b: &Board, goals: &[(Coord, u32)]) -> Result<Option<u32>, String> {
    let toks = ask(&format!(
        "adjudicate {} {}",
        board_wire(b)?,
        goals_wire(goals)
    ))?;
    let win_tok = toks
        .first()
        .ok_or_else(|| "adjudicate reply is empty".to_string())?;
    let win: i64 = win_tok
        .parse()
        .map_err(|e| format!("adjudicate win token {win_tok:?}: {e}"))?;
    Ok(if win < 0 { None } else { Some(win as u32) })
}

/// Goal distances and capped verdict (`dist BOARD GOALS`).
pub fn standing(b: &Board, goals: &[(Coord, u32)]) -> Result<Standing, String> {
    let toks = ask(&format!("dist {} {}", board_wire(b)?, goals_wire(goals)))?;
    if toks.len() != 3 {
        return Err(format!("dist reply has {} tokens, want 3", toks.len()));
    }
    let signed = |i: usize| -> Result<i64, String> {
        toks[i]
            .parse::<i64>()
            .map_err(|e| format!("dist field {i} ({:?}): {e}", toks[i]))
    };
    let opt = |v: i64| if v < 0 { None } else { Some(v as u32) };
    Ok(Standing {
        dist: [opt(signed(0)?), opt(signed(1)?)],
        verdict: opt(signed(2)?),
    })
}

/// Move legality (`legal BOARD COORDS who fX fY tX tY`).
pub fn move_legal(b: &Board, marks: &[Coord], m: &Move) -> Result<bool, String> {
    let toks = ask(&format!(
        "legal {} {} {} {} {}",
        board_wire(b)?,
        coords_wire(marks),
        m.who,
        Board::wire_coord(m.frm),
        Board::wire_coord(m.to)
    ))?;
    match toks.first().map(String::as_str) {
        Some("1") => Ok(true),
        Some("0") => Ok(false),
        other => Err(format!("legal reply {other:?} is not a bit")),
    }
}

/// Proposable destinations of `frm` (`targets BOARD COORDS who fX fY`).
pub fn legal_targets(
    b: &Board,
    marks: &[Coord],
    who: u32,
    frm: Coord,
) -> Result<Vec<Coord>, String> {
    let toks = ask(&format!(
        "targets {} {} {} {}",
        board_wire(b)?,
        coords_wire(marks),
        who,
        Board::wire_coord(frm)
    ))?;
    let r = refs(&toks);
    let (cs, rest) = coords_from_wire(&r)?;
    if !rest.is_empty() {
        return Err(format!("targets reply: {} trailing tokens", rest.len()));
    }
    Ok(cs)
}

/// Executable destinations of `frm` (`livetargets BOARD COORDS MOVES who fX fY`).
pub fn executable_targets(
    b: &Board,
    marks: &[Coord],
    ms: &[Move],
    who: u32,
    frm: Coord,
) -> Result<Vec<Coord>, String> {
    let toks = ask(&format!(
        "livetargets {} {} {} {} {}",
        board_wire(b)?,
        coords_wire(marks),
        moves_wire(ms),
        who,
        Board::wire_coord(frm)
    ))?;
    let r = refs(&toks);
    let (cs, rest) = coords_from_wire(&r)?;
    if !rest.is_empty() {
        return Err(format!("livetargets reply: {} trailing tokens", rest.len()));
    }
    Ok(cs)
}

/// Conflicted coordinates of a round (`clash BOARD COORDS MOVES`).
pub fn clash(b: &Board, marks: &[Coord], ms: &[Move]) -> Result<Vec<Coord>, String> {
    let toks = ask(&format!(
        "clash {} {} {}",
        board_wire(b)?,
        coords_wire(marks),
        moves_wire(ms)
    ))?;
    let r = refs(&toks);
    let (cs, rest) = coords_from_wire(&r)?;
    if !rest.is_empty() {
        return Err(format!("clash reply: {} trailing tokens", rest.len()));
    }
    Ok(cs)
}

/// One canonical round (`round TIE BOARD GOALS COORDS MOVES PIDS MOVES`).
pub fn round(
    b: &Board,
    goals: &[(Coord, u32)],
    marks: &[Coord],
    locked: &[Move],
    waiting: &[u32],
    subs: &[Move],
) -> Result<RoundOutcome, String> {
    let toks = ask(&format!(
        "round {} {} {} {} {} {} {}",
        b.tie_token(),
        board_wire(b)?,
        goals_wire(goals),
        coords_wire(marks),
        moves_wire(locked),
        pids_wire(waiting),
        moves_wire(subs)
    ))?;
    let (tag, rest) = toks
        .split_first()
        .ok_or_else(|| "round reply is empty".to_string())?;
    match tag.as_str() {
        "A" => {
            let r = refs(rest);
            let (marks, r1) = coords_from_wire(&r)?;
            let (locked, r2) = moves_from_wire(r1)?;
            let (waiting, r3) = pids_from_wire(r2)?;
            if !r3.is_empty() {
                return Err(format!("round A reply: {} trailing tokens", r3.len()));
            }
            Ok(RoundOutcome::Again {
                marks,
                locked,
                waiting,
            })
        }
        "R" => {
            let (win_tok, tail) = rest
                .split_first()
                .ok_or_else(|| "round R reply has no win token".to_string())?;
            let win: i64 = win_tok
                .parse()
                .map_err(|e| format!("round win token {win_tok:?}: {e}"))?;
            Ok(RoundOutcome::Resolved {
                board: board_of(tail, "round R")?,
                win: if win < 0 { None } else { Some(win as u32) },
            })
        }
        other => Err(format!("round reply tag {other:?} is neither A nor R")),
    }
}

/// Config-aware round execution supporting R2 variants (`TieBreakMode`, `ConfluenceMode`,
/// `two_cycle_swaps`, `railgun_fling`, `automata_count`) while delegating canonical rounds to `round`.
pub fn round_with_config(
    cfg: &GameConfig,
    last_auto_step: Option<Coord>,
    b: &Board,
    goals: &[(Coord, u32)],
    marks: &[Coord],
    locked: &[Move],
    waiting: &[u32],
    subs: &[Move],
) -> Result<(RoundOutcome, Board), String> {
    let is_canonical = matches!(
        cfg.tie_break,
        TieBreakMode::Column | TieBreakMode::Row | TieBreakMode::Freeze
    ) && cfg.confluence == ConfluenceMode::Conflict
        && !cfg.two_cycle_swaps
        && !cfg.railgun_fling
        && cfg.automata_count <= 1;

    if is_canonical {
        let mut b_cfg = b.clone();
        b_cfg.col_rule = matches!(cfg.tie_break, TieBreakMode::Column);
        let tie_tok = cfg.tie_break.wire_token();
        let toks = ask(&format!(
            "round {} {} {} {} {} {} {}",
            tie_tok,
            board_wire(&b_cfg)?,
            goals_wire(goals),
            coords_wire(marks),
            moves_wire(locked),
            pids_wire(waiting),
            moves_wire(subs)
        ))?;
        let (tag, rest) = toks
            .split_first()
            .ok_or_else(|| "round reply is empty".to_string())?;
        match tag.as_str() {
            "A" => {
                let r = refs(rest);
                let (marks_out, r1) = coords_from_wire(&r)?;
                let (locked_out, r2) = moves_from_wire(r1)?;
                let (waiting_out, r3) = pids_from_wire(r2)?;
                if !r3.is_empty() {
                    return Err(format!("round A reply: {} trailing tokens", r3.len()));
                }
                return Ok((
                    RoundOutcome::Again {
                        marks: marks_out,
                        locked: locked_out,
                        waiting: waiting_out,
                    },
                    b.clone(),
                ));
            }
            "R" => {
                let (win_tok, tail) = rest
                    .split_first()
                    .ok_or_else(|| "round R reply has no win token".to_string())?;
                let win: i64 = win_tok
                    .parse()
                    .map_err(|e| format!("round win token {win_tok:?}: {e}"))?;
                let mut all_moves = locked.to_vec();
                all_moves.extend(subs.iter().copied().filter(|m| waiting.contains(&m.who)));
                let mid = resolve_mid(&b_cfg, marks, &all_moves)?;
                let resolved_board = board_of(tail, "round R")?;
                return Ok((
                    RoundOutcome::Resolved {
                        board: resolved_board,
                        win: if win < 0 { None } else { Some(win as u32) },
                    },
                    mid,
                ));
            }
            other => return Err(format!("round reply tag {other:?} is neither A nor R")),
        }
    }

    // Generalized R2 variant round evaluation via `roundcfg` / `midcfg` wire protocol
    let mut b_cfg = b.clone();
    b_cfg.col_rule = matches!(cfg.tie_break, TieBreakMode::Column);
    let toks = ask(&format!(
        "roundcfg {} {} {} {} {} {} {} 0 {}",
        cfg.to_wire(),
        encode_opt_dir(last_auto_step),
        board_wire(&b_cfg)?,
        goals_wire(goals),
        coords_wire(marks),
        moves_wire(locked),
        pids_wire(waiting),
        moves_wire(subs)
    ))?;
    let (tag, rest) = toks
        .split_first()
        .ok_or_else(|| "roundcfg reply is empty".to_string())?;
    match tag.as_str() {
        "A" => {
            let r = refs(rest);
            let (marks_out, r1) = coords_from_wire(&r)?;
            let (locked_out, r2) = moves_from_wire(r1)?;
            let (waiting_out, _r3) = pids_from_wire(r2)?;
            Ok((
                RoundOutcome::Again {
                    marks: marks_out,
                    locked: locked_out,
                    waiting: waiting_out,
                },
                b.clone(),
            ))
        }
        "R" => {
            let (win_tok, tail) = rest
                .split_first()
                .ok_or_else(|| "roundcfg R reply has no win token".to_string())?;
            let win: i64 = win_tok
                .parse()
                .map_err(|e| format!("roundcfg win token {win_tok:?}: {e}"))?;
            let r = refs(tail);
            let (resolved_board, _after_board) = Board::from_wire(&r)?;
            let mut all_moves = locked.to_vec();
            all_moves.extend(subs.iter().copied().filter(|m| waiting.contains(&m.who)));
            let mid_toks = ask(&format!(
                "midcfg {} {} {} {}",
                cfg.to_wire(),
                board_wire(&b_cfg)?,
                coords_wire(marks),
                moves_wire(&all_moves)
            ))?;
            let mid = board_of(&mid_toks, "midcfg")?;
            Ok((
                RoundOutcome::Resolved {
                    board: resolved_board,
                    win: if win < 0 { None } else { Some(win as u32) },
                },
                mid,
            ))
        }
        other => Err(format!("roundcfg reply tag {other:?} is neither A nor R")),
    }
}

/// Compute the landing squares of all movers that moved on `b` in this round under `cfg`,
/// delegating directly to Lean's `landedcfg` (`Variants.lean::landedCoordsCfg`).
pub fn compute_landed_squares(
    cfg: &GameConfig,
    b: &Board,
    marks: &[Coord],
    all_moves: &[Move],
) -> Vec<Coord> {
    let Ok(bw) = board_wire(b) else {
        return Vec::new();
    };
    let Ok(toks) = ask(&format!(
        "landedcfg {} {} {} {}",
        cfg.to_wire(),
        bw,
        coords_wire(marks),
        moves_wire(all_moves)
    )) else {
        return Vec::new();
    };
    let r = refs(&toks);
    coords_from_wire(&r).map(|(cs, _)| cs).unwrap_or_default()
}

pub fn landed_coords_cfg(cfg: &GameConfig, b: &Board, ms: &[Move]) -> Vec<Coord> {
    compute_landed_squares(cfg, b, &[], ms)
}

/// Compute the four sightlines of the automaton on `b` via Lean's `sense`.
pub fn sightlines_of(b: &Board) -> Option<[Sightline; 4]> {
    let s = sense(b).ok()?;
    let spec: [(&'static str, Coord, usize); 4] = [
        ("left", (-1, 0), 1),
        ("right", (1, 0), 0),
        ("up", (0, -1), 3),
        ("down", (0, 1), 2),
    ];
    Some(spec.map(|(dir, step, i)| {
        let r = s.rays[i];
        Sightline {
            dir: dir.to_string(),
            step,
            dist: r.dist,
            what: r.what,
            end: (
                b.auto.0 + step.0 * r.dist as i32,
                b.auto.1 + step.1 * r.dist as i32,
            ),
        }
    }))
}

/// Union of the four sightline segments on `b` (the squares that can change the automaton's decision).
pub fn sightline_squares(b: &Board, lines: &[Sightline; 4]) -> Vec<Coord> {
    let mut out = Vec::new();
    for l in lines {
        for k in 1..=l.dist {
            let c = (
                b.auto.0 + l.step.0 * k as i32,
                b.auto.1 + l.step.1 * k as i32,
            );
            if b.in_bounds(c) && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// Convenience helper returning the active sightline squares on `b`.
pub fn sightlines(b: &Board) -> Vec<Coord> {
    sightlines_of(b)
        .map(|lines| sightline_squares(b, &lines))
        .unwrap_or_default()
}

fn squares_word(d: usize) -> String {
    if d == 1 {
        "1 square".to_string()
    } else {
        format!("{d} squares")
    }
}

fn dir_word(off: Coord) -> &'static str {
    match off {
        (dx, _) if dx > 0 => "right",
        (dx, _) if dx < 0 => "left",
        (_, dy) if dy > 0 => "down",
        (_, dy) if dy < 0 => "up",
        _ => "nowhere",
    }
}

fn decision_why(d: &Decision, axis_x: bool) -> String {
    let (dir, back) = match (axis_x, d.pos) {
        (true, true) => ("right", "left"),
        (true, false) => ("left", "right"),
        (false, true) => ("down", "up"),
        (false, false) => ("up", "down"),
    };
    match d.variant {
        3 => format!(
            "an attractor {} to its {dir} and a repulsor {} to its {back} (both send it {dir})",
            squares_word(d.att_dist),
            squares_word(d.rep_dist),
        ),
        2 => format!(
            "shoved {dir}, away from a repulsor {} to its {back}",
            squares_word(d.rep_dist.max(1)),
        ),
        1 => format!(
            "pulled {dir}, toward an attractor {} to its {dir}",
            squares_word(d.att_dist.max(1)),
        ),
        _ => "nothing pulls it on this axis".to_string(),
    }
}

/// Human-readable forecast of where the automaton would step on `b` and why.
pub fn sense_reading(b: &Board) -> (Coord, String) {
    let tb = if b.col_rule {
        TieBreakMode::Column
    } else {
        TieBreakMode::Row
    };
    let mut cfg = GameConfig::default();
    cfg.tie_break = tb;
    sense_reading_with_config(b, &cfg, None)
}

/// Config-aware forecast of where the automaton would step on `b` and why,
/// delegating raycasting, axis decisions, tie-breaks (`sensecfg`), and railgun fling (`stepcfg`) to Lean.
pub fn sense_reading_with_config(
    b: &Board,
    cfg: &GameConfig,
    last_auto_step: Option<Coord>,
) -> (Coord, String) {
    let Ok(s) = sense_with_config(b, cfg.tie_break, last_auto_step) else {
        return (b.auto, "nothing pulls it on this axis".to_string());
    };
    let off = s.offset;
    let (ox, oy) = off;
    let first_target = (b.auto.0 + ox, b.auto.1 + oy);
    let moves =
        (ox != 0 || oy != 0) && b.in_bounds(first_target) && b.cell_at(first_target) == VAC;
    if !moves {
        let why = if ox == 0 && oy == 0 {
            "the two axes pull equally hard, so it holds".to_string()
        } else if !b.in_bounds(first_target) {
            format!(
                "it leans {} but that is off the board, so it holds",
                dir_word(off)
            )
        } else {
            format!(
                "it leans {} but a piece is standing there, so it holds",
                dir_word(off)
            )
        };
        return (b.auto, why);
    }
    let after = board_wire(b)
        .and_then(|bw| {
            ask(&format!(
                "stepcfg {} {} {}",
                cfg.to_wire(),
                encode_opt_dir(last_auto_step),
                bw
            ))
        })
        .and_then(|toks| board_of(&toks, "stepcfg"))
        .unwrap_or_else(|_| b.clone());
    let axis_x = ox != 0;
    let dec = if axis_x { s.x_dec } else { s.y_dec };
    (after.auto, decision_why(&dec, axis_x))
}

/// Counterfactual forecast of what `seat`'s sealed move `mv` alone (plus public locked moves) would do.
pub fn own_move_forecast(
    b: &Board,
    marks: &[Coord],
    locked: &[Move],
    goals: &[(Coord, u32)],
    seat_idx: u32,
    mv: &Move,
) -> Option<String> {
    let mut cfg = GameConfig::default();
    cfg.tie_break = if b.col_rule {
        TieBreakMode::Column
    } else {
        TieBreakMode::Row
    };
    own_move_forecast_with_config(&cfg, None, b, marks, locked, goals, seat_idx, mv)
}

/// Config-aware counterfactual forecast of what `seat`'s sealed move `mv` alone would do.
pub fn own_move_forecast_with_config(
    cfg: &GameConfig,
    last_auto_step: Option<Coord>,
    b: &Board,
    marks: &[Coord],
    locked: &[Move],
    goals: &[(Coord, u32)],
    seat_idx: u32,
    mv: &Move,
) -> Option<String> {
    let waiting = vec![mv.who];
    let subs = vec![*mv];
    let (outcome, _) = round_with_config(
        cfg,
        last_auto_step,
        b,
        goals,
        marks,
        locked,
        &waiting,
        &subs,
    )
    .ok()?;
    let RoundOutcome::Resolved { board: after, win } = outcome else {
        return None;
    };
    let mine = standing(b, goals).ok().and_then(|s| s.dist[seat_idx as usize]);
    let then = standing(&after, goals)
        .ok()
        .and_then(|s| s.dist[seat_idx as usize]);
    let closer = match (mine, then) {
        (Some(a), Some(b)) if b < a => format!(", {} NEARER your corner", a - b),
        (Some(a), Some(b)) if b > a => format!(", {} further from your corner", b - a),
        _ => ", no nearer your corner".to_string(),
    };
    Some(match win {
        Some(w) if w == seat_idx => {
            "⚑ IF YOURS IS THE ONLY MOVE THAT LANDS, THIS WINS THE MATCH: the automaton steps \
             into your own corner. The other seat is sealing right now and can take that away."
                .to_string()
        }
        Some(w) => format!(
            "⚑ CAREFUL: if yours is the only move that lands, this hands the match to SEAT {}; \
             it walks the automaton into THEIR corner.",
            if w == 0 { "A" } else { "B" }
        ),
        None if after.auto == b.auto => {
            "Your move alone would leave the automaton where it is. (The other seat is moving too, \
             so the round can still shift it.)"
                .to_string()
        }
        None => format!(
            "Your move alone would send the automaton to ({},{}){closer}. The other seat is \
             sealing at the same time, so the real answer can differ; that gap is the game.",
            after.auto.0, after.auto.1
        ),
    })
}

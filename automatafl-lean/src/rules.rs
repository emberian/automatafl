//! **THE GAME ORACLE — every answer comes from the proven Lean 4 core.**
//!
//! Each function marshals a wire, calls `@[export] dregg_automatafl_rules`
//! (`Dregg2.Games.AutomataflFFI.rulesFFI`), and decodes the reply. Nothing in this module
//! decides any game rule in Rust (zero-twin architecture).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::board::{
    AutomatonSense, Board, Coord, Decision, Move, Raycast, coords_from_wire, coords_wire,
    goals_wire, moves_from_wire, moves_wire, pids_from_wire, pids_wire,
};
use crate::config::{
    BoardPreset, CooldownEntry, Dir, GameConfig, cooldowns_from_wire, cooldowns_wire,
};
use crate::ffi_rules_wire;

/// Whether the linked Lean core is available. Always `true` in `automatafl-lean` because
/// `build.rs` compiles the Lean C core directly into this crate on both native and `wasm32`.
pub fn available() -> bool {
    true
}

/// Raw wire query against `@[export] dregg_automatafl_rules`.
pub fn rules_wire(wire: &str) -> String {
    ffi_rules_wire(wire)
}

/// Ask the oracle. Strips the leading `"1"` success tag; returns `Err` on `"0"` (fail-closed).
fn ask(wire: &str) -> Result<Vec<String>, String> {
    let out = ffi_rules_wire(wire);
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

/// **The stock two-player opening** (`AutomataflRules.stockTwoPlayer`): the 11×11 Classic board.
pub fn stock_two_player() -> Result<Board, String> {
    board_of(&ask("stock")?, "stock")
}

/// **Preset opening board** (`preset 7`, `preset 9`, `preset 11`).
pub fn preset_board(preset: BoardPreset) -> Result<Board, String> {
    board_of(
        &ask(&format!("preset {}", preset.to_wire_token()))?,
        "preset",
    )
}

/// **The stock two-player goal assignment** (`AutomataflRules.stockGoals2`): seat `0` owns the two
/// `y = 0` corners, seat `1` owns the two `y = size - 1` corners.
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

/// **The automaton's whole decision** on `b` (`Board.raycast` × 4, `evaluateAxis` × 2, `chooseOffsetCfg`).
pub fn sense(b: &Board) -> Result<AutomatonSense, String> {
    let toks = ask(&format!("sense {} {}", b.tie_token(), board_wire(b)?))?;
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

/// **The automaton step** (`AutomataflRules.automatonStepCfg`).
pub fn automaton_step(b: &Board) -> Result<Board, String> {
    board_of(
        &ask(&format!("step {} {}", b.tie_token(), board_wire(b)?))?,
        "step",
    )
}

/// **Variant automaton step** (`stepcfg` verb) supporting `Momentum` tie-break and `railgun_fling`.
pub fn automaton_step_cfg(
    cfg: &GameConfig,
    b: &Board,
    last_dir: Option<Dir>,
) -> Result<Board, String> {
    board_of(
        &ask(&format!(
            "stepcfg {} {} {}",
            cfg.to_wire(),
            Dir::opt_to_wire(last_dir),
            board_wire(b)?
        ))?,
        "stepcfg",
    )
}

/// **The move-resolved intermediate board** (`AutomataflRules.resolveMoves` over legal submissions).
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

/// **Variant move-resolved intermediate board** (`midcfg` verb) supporting `BunchBefore`,
/// `Annihilate`, and `two_cycle_swaps`.
pub fn resolve_mid_cfg(
    cfg: &GameConfig,
    b: &Board,
    marks: &[Coord],
    ms: &[Move],
) -> Result<Board, String> {
    board_of(
        &ask(&format!(
            "midcfg {} {} {} {}",
            cfg.to_wire(),
            board_wire(b)?,
            coords_wire(marks),
            moves_wire(ms)
        ))?,
        "midcfg",
    )
}

/// **One clean round, whole**: resolve, step the automaton, check the win on entry (`winOnEntry`).
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

/// **Capped match terminal adjudication** (`AutomataflRules.adjudicateCapped`).
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

/// The board half of [`turn`] with no markers and no goals.
pub fn apply_turn(b: &Board, ms: &[Move]) -> Result<Board, String> {
    Ok(turn(b, &[], ms, &[])?.0)
}

/// Each seat's nearest-goal Manhattan distance plus the `adjudicateCapped` verdict (`dist` verb).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Standing {
    /// Each seat's Manhattan distance to its nearest goal corner (`None` if unseated).
    pub dist: [Option<u32>; 2],
    /// Who would win if the turn cap expired on this board (`adjudicateCapped`).
    pub verdict: Option<u32>,
}

/// Query [`Standing`] (`goalDistance` for seats 0 and 1 + `adjudicateCapped`).
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

/// **Move legality** (`AutomataflRules.moveLegalB`).
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

/// **Every proposable destination of `frm`** for seat `who` (`AutomataflFFI.targetsOf`).
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

/// **Every destination of `frm` that would actually execute** (`AutomataflFFI.liveTargetsOf`).
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

/// **The round's conflicted coordinates** (`clash` verb). Empty ⟺ the round resolves.
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

/// **Is this round clean?** Every submission is legal AND `clash` is empty.
pub fn round_is_clean(b: &Board, ms: &[Move]) -> Result<bool, String> {
    for m in ms {
        if !move_legal(b, &[], m)? {
            return Ok(false);
        }
    }
    Ok(clash(b, &[], ms)?.is_empty())
}

/// Canonical round outcome (`AutomataflRules.RoundOutcome`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RoundOutcome {
    /// Conflicted round: contested coordinates are marked, offending moves dropped, seats re-enter.
    Again {
        marks: Vec<Coord>,
        locked: Vec<Move>,
        waiting: Vec<u32>,
    },
    /// Clean round: moves resolved, automaton stepped, win checked on entry.
    Resolved {
        board: Board,
        win: Option<u32>,
    },
}

/// **One canonical round** (`AutomataflRules.roundStep`).
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

/// Variant round outcome (`AutomataflRules.VariantRoundOutcome`) including cooldowns and `last_dir`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum VariantRoundOutcome {
    /// Conflicted round requiring re-entry.
    Again {
        marks: Vec<Coord>,
        locked: Vec<Move>,
        waiting: Vec<u32>,
        cooldowns: Vec<CooldownEntry>,
        last_dir: Option<Dir>,
    },
    /// Clean round resolved under `cfg`.
    Resolved {
        board: Board,
        win: Option<u32>,
        cooldowns: Vec<CooldownEntry>,
        last_dir: Option<Dir>,
    },
}

/// **One configurable variant round** (`roundcfg` verb -> `AutomataflRules.roundStepVariant`).
#[allow(clippy::too_many_arguments)]
pub fn round_cfg(
    cfg: &GameConfig,
    last_dir: Option<Dir>,
    b: &Board,
    goals: &[(Coord, u32)],
    marks: &[Coord],
    locked: &[Move],
    waiting: &[u32],
    cooldowns: &[CooldownEntry],
    subs: &[Move],
) -> Result<VariantRoundOutcome, String> {
    let toks = ask(&format!(
        "roundcfg {} {} {} {} {} {} {} {} {}",
        cfg.to_wire(),
        Dir::opt_to_wire(last_dir),
        board_wire(b)?,
        goals_wire(goals),
        coords_wire(marks),
        moves_wire(locked),
        pids_wire(waiting),
        cooldowns_wire(cooldowns),
        moves_wire(subs)
    ))?;
    let (tag, rest) = toks
        .split_first()
        .ok_or_else(|| "roundcfg reply is empty".to_string())?;
    match tag.as_str() {
        "A" => {
            let r = refs(rest);
            let (marks, r1) = coords_from_wire(&r)?;
            let (locked, r2) = moves_from_wire(r1)?;
            let (waiting, r3) = pids_from_wire(r2)?;
            let (cooldowns, r4) = cooldowns_from_wire(r3)?;
            let ldir_tok = r4
                .first()
                .ok_or_else(|| "roundcfg A reply missing lastDir".to_string())?;
            let last_dir = Dir::opt_from_wire(ldir_tok)?;
            if r4.len() != 1 {
                return Err(format!(
                    "roundcfg A reply: {} trailing tokens",
                    r4.len() - 1
                ));
            }
            Ok(VariantRoundOutcome::Again {
                marks,
                locked,
                waiting,
                cooldowns,
                last_dir,
            })
        }
        "R" => {
            let (win_tok, tail) = rest
                .split_first()
                .ok_or_else(|| "roundcfg R reply has no win token".to_string())?;
            let win: i64 = win_tok
                .parse()
                .map_err(|e| format!("roundcfg win token {win_tok:?}: {e}"))?;
            let r = refs(tail);
            let (board, r1) = Board::from_wire(&r)?;
            let (cooldowns, r2) = cooldowns_from_wire(r1)?;
            let ldir_tok = r2
                .first()
                .ok_or_else(|| "roundcfg R reply missing lastDir".to_string())?;
            let last_dir = Dir::opt_from_wire(ldir_tok)?;
            if r2.len() != 1 {
                return Err(format!(
                    "roundcfg R reply: {} trailing tokens",
                    r2.len() - 1
                ));
            }
            Ok(VariantRoundOutcome::Resolved {
                board,
                win: if win < 0 { None } else { Some(win as u32) },
                cooldowns,
                last_dir,
            })
        }
        other => Err(format!("roundcfg reply tag {other:?} is neither A nor R")),
    }
}

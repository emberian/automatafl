//! Allocation-free position evaluation and tactical candidate move ordering over [`PackedBoard`].

use crate::packed::{
    GameConfig, MAX_TACTICAL_MOVES, MoveList, PackedBoard, PackedGoals, PackedMove,
    PackedRoundOutcome, PackedRoundState,
};

#[inline]
fn min_opponent_goal_distance(board: &PackedBoard, goals: &PackedGoals, seat: u8) -> Option<u32> {
    let auto = board.automaton();
    let mut best: Option<u32> = None;
    for &(gc, owner) in goals.slice() {
        if owner != seat {
            let d = auto.manhattan(gc);
            best = Some(match best {
                Some(prev) if prev <= d => prev,
                _ => d,
            });
        }
    }
    best
}

/// Evaluate a [`PackedBoard`] from the perspective of `seat`, returning a bounded score in `[-1.0, 1.0]`.
///
/// Combines:
/// 1. `goalDistance` differential (`AutomataflRules.adjudicateCapped` metric, generalized to 2–4 seats)
/// 2. One-step autonomous automaton lookahead (`automatonStepCfg` + `winOnEntry`)
/// 3. Goal-corner vacuum clearance (corners start blocked by repulsors and must be cleared to win)
/// 4. Cardinal ray alignment with the automaton
#[inline]
pub fn evaluate_board(
    board: &PackedBoard,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat: u8,
) -> f64 {
    let max_span = (2 * (board.size as u32).saturating_sub(1)).max(1) as f64;
    let fallback_dist = board.size as u32 * 2;

    let my_dist = board.goal_distance(goals, seat).unwrap_or(fallback_dist);
    let opp_dist = min_opponent_goal_distance(board, goals, seat).unwrap_or(fallback_dist);

    // 1. Primary distance differential
    let dist_diff = (opp_dist as f64 - my_dist as f64) / max_span;

    // 2. One-turn autonomous automaton lookahead (`stepAutomataCfg`)
    let (next_board, auto_win) = board.step_automata(cfg, goals);
    if let Some(winner) = auto_win {
        return if winner == seat { 0.95 } else { -0.95 };
    }

    let my_next_dist = next_board
        .goal_distance(goals, seat)
        .unwrap_or(fallback_dist);
    let opp_next_dist =
        min_opponent_goal_distance(&next_board, goals, seat).unwrap_or(fallback_dist);
    let next_diff = (opp_next_dist as f64 - my_next_dist as f64) / max_span;

    // 3. Goal corner clearance: an occupied corner cannot be entered by the automaton.
    let mut corner_bonus = 0.0f64;
    for &(gc, owner) in goals.slice() {
        let is_clear = board.is_vacuum(gc);
        if owner == seat {
            if is_clear {
                corner_bonus += if my_dist <= 4 { 0.08 } else { 0.03 };
            } else if my_dist <= 3 {
                corner_bonus -= 0.05;
            }
        } else if is_clear && opp_dist <= 4 {
            corner_bonus -= 0.06;
        }
    }

    let raw = 0.45 * dist_diff + 0.45 * next_diff + corner_bonus;
    raw.clamp(-0.99, 0.99)
}

/// Evaluate a single candidate move `m` for `seat` using ONLY the public [`PackedRoundState`].
///
/// Computes the Solo transition (`locked ++ [m]`) and rewards moves that:
/// - Win immediately on the automaton step
/// - Pull/push the automaton toward `seat`'s goal row/corner
/// - Place an attractor/repulsor onto the automaton's cardinal sightlines (`x == auto_x` or `y == auto_y`)
/// - Clear `seat`'s own goal corners
#[inline]
pub fn score_candidate_move_public(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat: u8,
    m: PackedMove,
) -> f64 {
    let mut solo_rs = *rs;
    solo_rs.waiting_mask = if m.who < 8 { 1u8 << m.who } else { 1 };
    let mut single_src = crate::packed::BitBoard128::EMPTY;
    if rs.board.in_bounds(m.frm) {
        single_src.set(rs.board.idx(m.frm));
    }
    let cfg_fast;
    let step_cfg = if rs.locked_len == 0 && !rs.board.blocked_by_mask(single_src, m) {
        cfg_fast = GameConfig {
            confluence: crate::packed::ConfluenceMode::Conflict,
            two_cycle_swaps: false,
            cooldown_turns: 0,
            ..*cfg
        };
        &cfg_fast
    } else {
        cfg
    };
    let (after, win) = match solo_rs.round_step(step_cfg, goals, &[m]) {
        PackedRoundOutcome::Again(_) => return -0.9,
        PackedRoundOutcome::Resolved { board, win } => (board, win),
    };
    if let Some(winner) = win {
        return if winner == seat { 1.0 } else { -1.0 };
    }

    let mut score = evaluate_board(&after, goals, cfg, seat);

    // Bonus for interacting with the automaton's sightlines
    let auto = rs.board.automaton();
    if m.to.x == auto.x || m.to.y == auto.y {
        score += 0.025;
    }
    if m.frm.x == auto.x || m.frm.y == auto.y {
        score += 0.015;
    }

    // Bonus for vacating own goal corner
    if goals.owner_of(m.frm) == Some(seat) && !rs.board.is_vacuum(m.frm) {
        score += 0.035;
    }

    score.clamp(-0.98, 0.98)
}

/// Generate and rank up to `max_candidates` tactical moves for `seat` in descending order
/// of solo tactical evaluation, using only public round state and zero heap allocations.
pub fn rank_candidate_moves(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat: u8,
    max_candidates: usize,
    out: &mut MoveList,
) {
    let mut out_scores = [0.0f32; MAX_TACTICAL_MOVES];
    rank_candidate_moves_scored(rs, goals, cfg, seat, max_candidates, out, &mut out_scores);
}

/// Generate and rank up to `max_candidates` tactical moves for `seat` in descending order
/// of solo tactical evaluation, writing both the ranked moves into `out` and their corresponding
/// scores into `out_scores[..out.len()]`.
pub fn rank_candidate_moves_scored(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat: u8,
    max_candidates: usize,
    out: &mut MoveList,
    out_scores: &mut [f32; MAX_TACTICAL_MOVES],
) {
    let mut all_moves = MoveList::new();
    rs.board
        .generate_tactical_moves(rs.marks, rs.locked_slice(), seat, cfg, &mut all_moves);

    let count = all_moves.len();
    if count == 0 {
        out.clear();
        return;
    }

    let mut scores = [0.0f32; MAX_TACTICAL_MOVES];
    let mut indices = [0u16; MAX_TACTICAL_MOVES];
    let slice = all_moves.as_slice();

    // Fast exact sightline invariance optimization: when `locked_len == 0`, `automata_count <= 1`,
    // and `!railgun_fling`, any move `m` that does not touch the automaton's current axes
    // `(auto0.x, auto0.y)`, its 1-step lookahead axes `(auto1.x, auto1.y)`, or a goal corner
    // produces the exact same post-step automaton trajectory and board evaluation as `stepped1`.
    let can_cache_quiet =
        rs.locked_len == 0 && cfg.automata_count <= 1 && !cfg.railgun_fling;
    let auto0 = rs.board.automaton();
    let (auto1, quiet_score_opt) = if can_cache_quiet {
        let (stepped1, win1) = rs.board.step_automata(cfg, goals);
        let a1 = stepped1.automaton();
        let qs = if let Some(w) = win1 {
            if w == seat { 1.0f32 } else { -1.0f32 }
        } else {
            evaluate_board(&stepped1, goals, cfg, seat).clamp(-0.98, 0.98) as f32
        };
        (a1, Some(qs))
    } else {
        (auto0, None)
    };

    for i in 0..count {
        let m = slice[i];
        let touches_sightlines_or_corners = m.frm.x == auto0.x
            || m.frm.y == auto0.y
            || m.to.x == auto0.x
            || m.to.y == auto0.y
            || m.frm.x == auto1.x
            || m.frm.y == auto1.y
            || m.to.x == auto1.x
            || m.to.y == auto1.y
            || goals.owner_of(m.frm).is_some()
            || goals.owner_of(m.to).is_some();
        scores[i] = match (quiet_score_opt, touches_sightlines_or_corners) {
            (Some(qs), false) => qs,
            _ => score_candidate_move_public(rs, goals, cfg, seat, m) as f32,
        };
        indices[i] = i as u16;
    }

    // Partial selection sort for top `k` candidates on stack (avoids sorting the entire tail)
    let k = max_candidates.min(count);
    out.clear();
    for i in 0..k {
        let mut best_j = i;
        let mut best_s = scores[indices[i] as usize];
        for j in (i + 1)..count {
            let s = scores[indices[j] as usize];
            if s > best_s {
                best_s = s;
                best_j = j;
            }
        }
        indices.swap(i, best_j);
        let chosen_idx = indices[i] as usize;
        out.push(slice[chosen_idx]);
        out_scores[i] = scores[chosen_idx];
    }
}

/// Fast zero-FFI tactical move heuristic for MCTS playout sampling.
#[inline]
pub fn fast_rollout_move_score(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    seat: u8,
    m: PackedMove,
) -> f64 {
    let auto = rs.board.automaton();
    let mut s = 0.0f64;
    if m.to.x == auto.x || m.to.y == auto.y {
        s += 0.04;
    }
    if m.frm.x == auto.x || m.frm.y == auto.y {
        s += 0.02;
    }
    if goals.owner_of(m.frm) == Some(seat) && !rs.board.is_vacuum(m.frm) {
        s += 0.06;
    }
    if goals.owner_of(m.to) == Some(seat) {
        s -= 0.05;
    }
    s
}

//! Regret Matching Plus (RM+) Simultaneous-Move Nash Equilibrium Bot over [`PackedBoard`].
//!
//! As established in `AutomataflRules.lean` §6B, Automatafl is a simultaneous-move game where
//! deterministic policies on symmetric positions can lock into mirror-symmetric cycles, whereas
//! game-theoretically optimal play is generally a mixed-strategy Nash equilibrium of the stage
//! matrix game.
//!
//! This module constructs the normal-form payoff matrix over the public [`PackedRoundState`]
//! (never inspecting any secret opponent move) and runs RM+ (Tammelin 2014 / Hart & Mas-Colell
//! 2000) to compute the time-averaged mixed equilibrium strategy in stack memory.

use crate::SplitMix64;
use crate::eval::{evaluate_board, rank_candidate_moves, rank_candidate_moves_scored};
use crate::packed::{
    GameConfig, MAX_PLAYERS, MAX_TACTICAL_MOVES, MoveList, PackedGoals, PackedMove,
    PackedRoundOutcome, PackedRoundState,
};

/// Maximum matrix dimension per seat (fits `24 x 24` matrix on the stack).
pub const MAX_MATRIX_MOVES: usize = 24;

/// Configuration for [`RegretMatchingBot`].
#[derive(Clone, Copy, Debug)]
pub struct RegretConfig {
    /// Number of candidate moves per seat included in the normal-form stage matrix.
    pub candidate_count: usize,
    /// Number of RM+ iterations to run per decision.
    pub iterations: usize,
    /// Minimum probability support threshold when sampling from the equilibrium strategy.
    pub support_threshold: f64,
    /// Whether to include a 1-turn tactical lookahead on each matrix cell.
    pub one_ply_lookahead: bool,
}

impl Default for RegretConfig {
    fn default() -> Self {
        Self {
            candidate_count: 16,
            iterations: 200,
            support_threshold: 0.02,
            one_ply_lookahead: true,
        }
    }
}

/// Solved normal-form stage equilibrium for a simultaneous-move round.
#[derive(Clone, Debug)]
pub struct StageEquilibrium {
    /// Expected game value for Seat 0 (or the primary row seat) in `[-1.0, 1.0]`.
    pub game_value_seat0: f64,
    /// Equilibrium probability distribution `(PackedMove, probability)` for Seat 0 (or row seat).
    pub strategy_seat0: Vec<(PackedMove, f64)>,
    /// Equilibrium probability distribution `(PackedMove, probability)` for Seat 1 (or column seat).
    pub strategy_seat1: Vec<(PackedMove, f64)>,
    /// Whether the stage payoff matrix possesses a pure saddle point (within `1e-5`).
    pub has_pure_saddle: bool,
    /// Number of Seat 0 candidate moves tied (within `1e-4`) for the maximin value.
    pub tied_maximin_count: usize,
}

/// Regret Matching Plus (RM+) simultaneous-move bot.
pub struct RegretMatchingBot {
    pub config: RegretConfig,
    rng: SplitMix64,
}

impl RegretMatchingBot {
    pub fn new(config: RegretConfig, seed: u64) -> Self {
        Self {
            config,
            rng: SplitMix64::new(seed),
        }
    }

    /// Select a move for `seat` by solving the simultaneous-move stage game on `rs` via RM+
    /// and sampling from `seat`'s mixed equilibrium distribution.
    pub fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        if !rs.is_waiting(seat) {
            return None;
        }

        let k_max = self.config.candidate_count.clamp(1, MAX_MATRIX_MOVES);

        // If no opponent is waiting in this clash re-entry round,
        // `seat` faces a single-agent decision over `rs`.
        let Some(opp) = primary_waiting_opponent(rs, seat) else {
            let mut my_moves = MoveList::new();
            rank_candidate_moves(rs, goals, cfg, seat, k_max, &mut my_moves);
            let slice = my_moves.as_slice();
            if slice.is_empty() {
                return None;
            }
            let mut best_m = slice[0];
            let mut best_v = f64::NEG_INFINITY;
            for &m in slice {
                let v = eval_joint_cell_for_seat(
                    rs,
                    goals,
                    cfg,
                    seat,
                    opp_fallback(seat),
                    &[m],
                    self.config.one_ply_lookahead,
                );
                if v > best_v {
                    best_v = v;
                    best_m = m;
                }
            }
            debug_assert_eq!(best_m.who, seat);
            return Some(best_m);
        };

        let eq = solve_stage_equilibrium_for_seats(rs, goals, cfg, &self.config, seat, opp);
        let dist = &eq.strategy_seat0;
        if dist.is_empty() {
            return None;
        }

        // Filter out negligible-weight moves below `support_threshold` and sample
        let mut filtered_sum = 0.0f64;
        for &(_, p) in dist {
            if p >= self.config.support_threshold {
                filtered_sum += p;
            }
        }

        if filtered_sum <= 0.0 {
            let m = dist[0].0;
            debug_assert_eq!(m.who, seat);
            return Some(m);
        }

        let mut r = self.rng.next_f64() * filtered_sum;
        for &(m, p) in dist {
            if p >= self.config.support_threshold {
                if r <= p {
                    debug_assert_eq!(m.who, seat);
                    return Some(m);
                }
                r -= p;
            }
        }

        let fallback = dist[0].0;
        debug_assert_eq!(fallback.who, seat);
        Some(fallback)
    }
}

/// Solve the simultaneous-move stage game between Seat 0 and Seat 1 on `rs` using RM+.
pub fn solve_stage_equilibrium(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    rm_cfg: &RegretConfig,
) -> StageEquilibrium {
    solve_stage_equilibrium_for_seats(rs, goals, cfg, rm_cfg, 0, 1)
}

/// Solve the simultaneous-move stage game between row player `seat_a` and column player `seat_b`
/// on `rs` using RM+ in stack buffers.
pub fn solve_stage_equilibrium_for_seats(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    rm_cfg: &RegretConfig,
    seat_a: u8,
    seat_b: u8,
) -> StageEquilibrium {
    let k_max = rm_cfg.candidate_count.clamp(1, MAX_MATRIX_MOVES);

    let mut moves0 = MoveList::new();
    let mut moves1 = MoveList::new();
    rank_candidate_moves(rs, goals, cfg, seat_a, k_max, &mut moves0);
    rank_candidate_moves(rs, goals, cfg, seat_b, k_max, &mut moves1);

    let k0 = moves0.len().min(MAX_MATRIX_MOVES);
    let k1 = moves1.len().min(MAX_MATRIX_MOVES);

    if k0 == 0 || k1 == 0 {
        return StageEquilibrium {
            game_value_seat0: 0.0,
            strategy_seat0: Vec::new(),
            strategy_seat1: Vec::new(),
            has_pure_saddle: true,
            tied_maximin_count: 0,
        };
    }

    let m0_slice = &moves0.as_slice()[..k0];
    let m1_slice = &moves1.as_slice()[..k1];

    // If there are additional waiting seats in a 3/4-player game, include their top greedy move
    let mut extra_subs = [PackedMove::default(); MAX_PLAYERS];
    let mut extra_len = 0usize;
    for s in 0..(MAX_PLAYERS as u8) {
        if s != seat_a && s != seat_b && rs.is_waiting(s) {
            let mut other_list = MoveList::new();
            rank_candidate_moves(rs, goals, cfg, s, 1, &mut other_list);
            if let Some(&top) = other_list.as_slice().first() {
                extra_subs[extra_len] = top;
                extra_len += 1;
            }
        }
    }

    // Build payoff matrix `payoff[i][j]` = utility for `seat_a` when `seat_a` plays `m0[i]`
    // and `seat_b` plays `m1[j]`.
    let mut payoff = [[0.0f64; MAX_MATRIX_MOVES]; MAX_MATRIX_MOVES];
    for (i, &m0) in m0_slice.iter().enumerate() {
        debug_assert_eq!(m0.who, seat_a);
        for (j, &m1) in m1_slice.iter().enumerate() {
            debug_assert_eq!(m1.who, seat_b);
            let mut subs = [PackedMove::default(); MAX_PLAYERS];
            subs[0] = m0;
            subs[1] = m1;
            for k in 0..extra_len {
                subs[2 + k] = extra_subs[k];
            }
            payoff[i][j] = eval_joint_cell_for_seat(
                rs,
                goals,
                cfg,
                seat_a,
                seat_b,
                &subs[..(2 + extra_len)],
                rm_cfg.one_ply_lookahead,
            );
        }
    }

    // Check pure saddle point (maximin == minimax) and count tied maximin moves
    let mut maximin = f64::NEG_INFINITY;
    let mut row_mins = [0.0f64; MAX_MATRIX_MOVES];
    for i in 0..k0 {
        let mut rmin = f64::INFINITY;
        for j in 0..k1 {
            if payoff[i][j] < rmin {
                rmin = payoff[i][j];
            }
        }
        row_mins[i] = rmin;
        if rmin > maximin {
            maximin = rmin;
        }
    }

    let mut minimax = f64::INFINITY;
    for j in 0..k1 {
        let mut cmax = f64::NEG_INFINITY;
        for i in 0..k0 {
            if payoff[i][j] > cmax {
                cmax = payoff[i][j];
            }
        }
        if cmax < minimax {
            minimax = cmax;
        }
    }

    let has_pure_saddle = (minimax - maximin).abs() <= 1e-5;
    let tied_maximin_count = row_mins[..k0]
        .iter()
        .filter(|&&v| (v - maximin).abs() <= 1e-4)
        .count();

    // Run RM+ (Regret Matching Plus) with linear strategy averaging
    let mut regret0 = [0.0f64; MAX_MATRIX_MOVES];
    let mut regret1 = [0.0f64; MAX_MATRIX_MOVES];
    let mut strat0 = [0.0f64; MAX_MATRIX_MOVES];
    let mut strat1 = [0.0f64; MAX_MATRIX_MOVES];
    let mut sum_strat0 = [0.0f64; MAX_MATRIX_MOVES];
    let mut sum_strat1 = [0.0f64; MAX_MATRIX_MOVES];

    let iters = rm_cfg.iterations.max(1);
    for t in 1..=iters {
        let w = t as f64;

        let pos_sum0: f64 = regret0[..k0].iter().copied().sum();
        if pos_sum0 > 1e-12 {
            for i in 0..k0 {
                strat0[i] = regret0[i] / pos_sum0;
            }
        } else {
            let unif = 1.0 / (k0 as f64);
            for i in 0..k0 {
                strat0[i] = unif;
            }
        }

        let pos_sum1: f64 = regret1[..k1].iter().copied().sum();
        if pos_sum1 > 1e-12 {
            for j in 0..k1 {
                strat1[j] = regret1[j] / pos_sum1;
            }
        } else {
            let unif = 1.0 / (k1 as f64);
            for j in 0..k1 {
                strat1[j] = unif;
            }
        }

        for i in 0..k0 {
            sum_strat0[i] += w * strat0[i];
        }
        for j in 0..k1 {
            sum_strat1[j] += w * strat1[j];
        }

        // Expected utilities for `seat_a` actions against `strat1`
        let mut u0 = [0.0f64; MAX_MATRIX_MOVES];
        let mut ev0 = 0.0f64;
        for i in 0..k0 {
            let mut s = 0.0f64;
            for j in 0..k1 {
                s += strat1[j] * payoff[i][j];
            }
            u0[i] = s;
            ev0 += strat0[i] * s;
        }

        // Expected utilities for `seat_b` actions against `strat0` (zero-sum: `-payoff[i][j]`)
        let mut u1 = [0.0f64; MAX_MATRIX_MOVES];
        for j in 0..k1 {
            let mut s = 0.0f64;
            for i in 0..k0 {
                s -= strat0[i] * payoff[i][j];
            }
            u1[j] = s;
        }
        let ev1 = -ev0;

        for i in 0..k0 {
            regret0[i] = (regret0[i] + u0[i] - ev0).max(0.0);
        }
        for j in 0..k1 {
            regret1[j] = (regret1[j] + u1[j] - ev1).max(0.0);
        }
    }

    let norm0: f64 = sum_strat0[..k0].iter().copied().sum();
    let norm1: f64 = sum_strat1[..k1].iter().copied().sum();

    let mut avg0 = Vec::with_capacity(k0);
    let mut avg1 = Vec::with_capacity(k1);
    for i in 0..k0 {
        let p = if norm0 > 0.0 {
            sum_strat0[i] / norm0
        } else {
            1.0 / (k0 as f64)
        };
        avg0.push((m0_slice[i], p));
    }
    for j in 0..k1 {
        let p = if norm1 > 0.0 {
            sum_strat1[j] / norm1
        } else {
            1.0 / (k1 as f64)
        };
        avg1.push((m1_slice[j], p));
    }

    let mut game_value_seat0 = 0.0f64;
    for (i, &(_, p0)) in avg0.iter().enumerate() {
        for (j, &(_, p1)) in avg1.iter().enumerate() {
            game_value_seat0 += p0 * p1 * payoff[i][j];
        }
    }

    // Sort distributions by descending probability for convenient inspection
    avg0.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
    avg1.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));

    StageEquilibrium {
        game_value_seat0,
        strategy_seat0: avg0,
        strategy_seat1: avg1,
        has_pure_saddle,
        tied_maximin_count,
    }
}

#[inline]
fn primary_waiting_opponent(rs: &PackedRoundState, seat: u8) -> Option<u8> {
    let paired = seat ^ 1;
    if paired < (MAX_PLAYERS as u8) && paired != seat && rs.is_waiting(paired) {
        return Some(paired);
    }
    for s in 0..(MAX_PLAYERS as u8) {
        if s != seat && rs.is_waiting(s) {
            return Some(s);
        }
    }
    None
}

#[inline]
fn opp_fallback(seat: u8) -> u8 {
    (seat ^ 1).min((MAX_PLAYERS as u8) - 1)
}

#[inline]
fn eval_joint_cell_for_seat(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat_a: u8,
    seat_b: u8,
    subs: &[PackedMove],
    one_ply_lookahead: bool,
) -> f64 {
    let active_mask = rs.active_player_mask(goals);
    match rs.round_step(cfg, goals, subs) {
        PackedRoundOutcome::Resolved { board, win } => {
            if let Some(w) = win {
                return if w == seat_a { 1.0 } else { -1.0 };
            }
            let base = evaluate_board(&board, goals, cfg, seat_a);
            if !one_ply_lookahead {
                return base;
            }
            // Fast 1-ply solo response estimate from the post-board
            let next_rs = PackedRoundState::open_with_mask(board, active_mask);
            let mut m0 = MoveList::new();
            let mut s0 = [0.0f32; MAX_TACTICAL_MOVES];
            let mut m1 = MoveList::new();
            let mut s1 = [0.0f32; MAX_TACTICAL_MOVES];
            rank_candidate_moves_scored(&next_rs, goals, cfg, seat_a, 1, &mut m0, &mut s0);
            rank_candidate_moves_scored(&next_rs, goals, cfg, seat_b, 1, &mut m1, &mut s1);
            let best0 = if !m0.is_empty() { s0[0] as f64 } else { base };
            let best1 = if !m1.is_empty() { s1[0] as f64 } else { -base };
            (0.6 * base + 0.2 * best0 - 0.2 * best1).clamp(-0.99, 0.99)
        }
        PackedRoundOutcome::Again(next_rs) => {
            // Evaluate the clash re-entry state accounting for locked non-clashing moves
            let mut base = evaluate_board(&next_rs.board, goals, cfg, seat_a);
            if next_rs.locked_len > 0 {
                let mut locked_only_rs = next_rs;
                locked_only_rs.waiting_mask = 0;
                if let PackedRoundOutcome::Resolved {
                    board: stepped,
                    win,
                } = locked_only_rs.round_step(cfg, goals, &[])
                {
                    if let Some(w) = win {
                        let win_score = if w == seat_a { 0.95 } else { -0.95 };
                        return (0.5 * base + 0.5 * win_score).clamp(-0.99, 0.99);
                    }
                    let locked_eval = evaluate_board(&stepped, goals, cfg, seat_a);
                    base = 0.5 * base + 0.5 * locked_eval;
                }
            }
            base.clamp(-0.99, 0.99)
        }
    }
}


//! Simultaneous-move Maximin / Alpha-Beta Matrix Search over [`PackedBoard`].
//!
//! Models each round as a simultaneous normal-form matrix game over public [`PackedRoundState`],
//! computing security levels ($\max_{a_p} \min_{a_{-p}} V(a_p, a_{-p})$) with alpha-beta row
//! cutoffs and randomized near-tie breaking (avoiding deterministic mirror-symmetry lockstep).

use crate::SplitMix64;
use crate::eval::{evaluate_board, rank_candidate_moves};
use crate::packed::{
    GameConfig, MAX_PLAYERS, MoveList, PackedGoals, PackedMove, PackedRoundOutcome,
    PackedRoundState,
};

#[inline]
fn primary_waiting_opponent(rs: &PackedRoundState, seat: u8) -> Option<u8> {
    let paired = seat ^ 1;
    if paired != seat && rs.is_waiting(paired) {
        return Some(paired);
    }
    for s in 0..MAX_PLAYERS as u8 {
        if s != seat && rs.is_waiting(s) {
            return Some(s);
        }
    }
    None
}

#[inline]
fn collect_other_waiting_top_moves(
    rs: &PackedRoundState,
    goals: &PackedGoals,
    cfg: &GameConfig,
    seat: u8,
    opp_opt: Option<u8>,
    out: &mut [PackedMove; MAX_PLAYERS],
) -> usize {
    let mut len = 0usize;
    let mut tmp = MoveList::new();
    for s in 0..MAX_PLAYERS as u8 {
        if s != seat && Some(s) != opp_opt && rs.is_waiting(s) {
            rank_candidate_moves(rs, goals, cfg, s, 1, &mut tmp);
            if let Some(&m) = tmp.as_slice().first() {
                out[len] = m;
                len += 1;
            }
        }
    }
    len
}

/// Configuration for [`SimultaneousMinimaxBot`].
#[derive(Clone, Copy, Debug)]
pub struct MinimaxConfig {
    /// Search depth in full turns.
    pub max_depth: u8,
    /// Maximum ranked candidate moves considered per seat at the root.
    pub root_branching: usize,
    /// Maximum ranked candidate moves considered per seat at interior nodes.
    pub inner_branching: usize,
    /// Near-tie tolerance for randomized tie-breaking among top maximin moves.
    pub tie_epsilon: f64,
}

impl Default for MinimaxConfig {
    fn default() -> Self {
        Self {
            max_depth: 2,
            root_branching: 16,
            inner_branching: 8,
            tie_epsilon: 1e-4,
        }
    }
}

/// Simultaneous-move Maximin search bot operating directly on [`PackedRoundState`].
pub struct SimultaneousMinimaxBot {
    pub config: MinimaxConfig,
    rng: SplitMix64,
    pub nodes_evaluated: u64,
}

impl SimultaneousMinimaxBot {
    pub fn new(config: MinimaxConfig, seed: u64) -> Self {
        Self {
            config,
            rng: SplitMix64::new(seed),
            nodes_evaluated: 0,
        }
    }

    /// Select a move for `seat` given ONLY the public [`PackedRoundState`].
    ///
    /// Never accesses any uncommitted opponent move and never duplicates moves across players.
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
        self.nodes_evaluated = 0;
        let active_mask = rs.active_player_mask(goals);

        let mut my_moves = MoveList::new();
        rank_candidate_moves(
            rs,
            goals,
            cfg,
            seat,
            self.config.root_branching,
            &mut my_moves,
        );
        if my_moves.is_empty() {
            return None;
        }
        if my_moves.len() == 1 {
            return Some(my_moves.as_slice()[0]);
        }

        let opp_opt = primary_waiting_opponent(rs, seat);
        let mut opp_moves = MoveList::new();
        if let Some(opp) = opp_opt {
            rank_candidate_moves(
                rs,
                goals,
                cfg,
                opp,
                self.config.root_branching,
                &mut opp_moves,
            );
        }

        let mut extra_subs = [PackedMove::default(); MAX_PLAYERS];
        let extra_len =
            collect_other_waiting_top_moves(rs, goals, cfg, seat, opp_opt, &mut extra_subs);

        let mut best_score = f64::NEG_INFINITY;
        let mut scores = [f64::NEG_INFINITY; 64];
        let my_count = my_moves.len().min(64);

        for (i, &my_m) in my_moves.as_slice()[..my_count].iter().enumerate() {
            debug_assert_eq!(my_m.who, seat);

            let score = if opp_opt.is_none() || opp_moves.is_empty() {
                // Only `seat` (plus any fallback seats) is submitting in this re-entry sub-round
                let mut subs = [PackedMove::default(); MAX_PLAYERS];
                subs[0] = my_m;
                subs[1..1 + extra_len].copy_from_slice(&extra_subs[..extra_len]);
                let outcome = rs.round_step(cfg, goals, &subs[..1 + extra_len]);
                self.evaluate_outcome(
                    outcome,
                    goals,
                    cfg,
                    seat,
                    active_mask,
                    self.config.max_depth.saturating_sub(1),
                    2,
                )
            } else {
                let opp = opp_opt.unwrap();
                let mut worst_reply = f64::INFINITY;
                let mut sum_reply = 0.0f64;
                let opp_slice = opp_moves.as_slice();
                let mut pruned = false;

                for &opp_m in opp_slice {
                    debug_assert_eq!(opp_m.who, opp);
                    debug_assert_ne!(my_m.who, opp_m.who);

                    let mut subs = [PackedMove::default(); MAX_PLAYERS];
                    subs[0] = my_m;
                    subs[1] = opp_m;
                    subs[2..2 + extra_len].copy_from_slice(&extra_subs[..extra_len]);
                    let outcome = rs.round_step(cfg, goals, &subs[..2 + extra_len]);
                    let val = self.evaluate_outcome(
                        outcome,
                        goals,
                        cfg,
                        seat,
                        active_mask,
                        self.config.max_depth.saturating_sub(1),
                        2,
                    );
                    if val < worst_reply {
                        worst_reply = val;
                    }
                    sum_reply += val;

                    // Alpha-beta row cutoff: if the worst reply already forces the blended
                    // security score significantly below `best_score`, prune remaining replies.
                    if 0.88 * worst_reply + 0.12 * 1.0 < best_score - self.config.tie_epsilon {
                        pruned = true;
                        break;
                    }
                }

                if pruned {
                    0.88 * worst_reply - 0.12
                } else {
                    let avg_reply = sum_reply / (opp_slice.len() as f64);
                    0.88 * worst_reply + 0.12 * avg_reply
                }
            };

            scores[i] = score;
            if score > best_score {
                best_score = score;
            }
        }

        // Collect all moves within `tie_epsilon` of `best_score` and sample uniformly
        // so symmetric boards do not deterministic-freeze.
        let mut tied = [0usize; 64];
        let mut tied_len = 0usize;
        for i in 0..my_count {
            if scores[i] >= best_score - self.config.tie_epsilon {
                tied[tied_len] = i;
                tied_len += 1;
            }
        }

        let pick_idx = if tied_len <= 1 {
            tied[0]
        } else {
            tied[self.rng.next_usize(tied_len)]
        };
        Some(my_moves.as_slice()[pick_idx])
    }

    fn evaluate_outcome(
        &mut self,
        outcome: PackedRoundOutcome,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
        active_mask: u8,
        depth: u8,
        clash_budget: u8,
    ) -> f64 {
        self.nodes_evaluated += 1;
        match outcome {
            PackedRoundOutcome::Resolved { board, win } => {
                if let Some(w) = win {
                    return if w == seat { 1.0 } else { -1.0 };
                }
                if depth == 0 {
                    return evaluate_board(&board, goals, cfg, seat).clamp(-0.90, 0.90);
                }
                let next_rs = PackedRoundState::open_with_mask(board, active_mask);
                0.92 * self.search_state(&next_rs, goals, cfg, seat, active_mask, depth, 2)
            }
            PackedRoundOutcome::Again(next_rs) => {
                if clash_budget == 0 || depth == 0 {
                    // Slight penalty for unresolved clashes vs clean progress
                    return (evaluate_board(&next_rs.board, goals, cfg, seat) * 0.90)
                        .clamp(-0.90, 0.90);
                }
                0.95 * self.search_state(
                    &next_rs,
                    goals,
                    cfg,
                    seat,
                    active_mask,
                    depth,
                    clash_budget - 1,
                )
            }
        }
    }

    fn search_state(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
        active_mask: u8,
        depth: u8,
        clash_budget: u8,
    ) -> f64 {
        let my_waiting = rs.is_waiting(seat);
        let opp_opt = primary_waiting_opponent(rs, seat);

        let mut my_moves = MoveList::new();
        if my_waiting {
            rank_candidate_moves(
                rs,
                goals,
                cfg,
                seat,
                self.config.inner_branching,
                &mut my_moves,
            );
        }

        let mut opp_moves = MoveList::new();
        if let Some(opp) = opp_opt {
            rank_candidate_moves(
                rs,
                goals,
                cfg,
                opp,
                self.config.inner_branching,
                &mut opp_moves,
            );
        }

        let mut extra_subs = [PackedMove::default(); MAX_PLAYERS];
        let extra_len =
            collect_other_waiting_top_moves(rs, goals, cfg, seat, opp_opt, &mut extra_subs);

        if my_waiting && opp_opt.is_some() {
            if my_moves.is_empty() || opp_moves.is_empty() {
                return evaluate_board(&rs.board, goals, cfg, seat);
            }
            let mut best = f64::NEG_INFINITY;
            for &my_m in my_moves.as_slice() {
                let mut worst = f64::INFINITY;
                for &opp_m in opp_moves.as_slice() {
                    let mut subs = [PackedMove::default(); MAX_PLAYERS];
                    subs[0] = my_m;
                    subs[1] = opp_m;
                    subs[2..2 + extra_len].copy_from_slice(&extra_subs[..extra_len]);
                    let outcome = rs.round_step(cfg, goals, &subs[..2 + extra_len]);
                    let v = self.evaluate_outcome(
                        outcome,
                        goals,
                        cfg,
                        seat,
                        active_mask,
                        depth.saturating_sub(1),
                        clash_budget,
                    );
                    if v < worst {
                        worst = v;
                    }
                    if worst <= best {
                        break;
                    }
                }
                if worst > best {
                    best = worst;
                }
            }
            best
        } else if my_waiting {
            let mut best = f64::NEG_INFINITY;
            for &my_m in my_moves.as_slice() {
                let mut subs = [PackedMove::default(); MAX_PLAYERS];
                subs[0] = my_m;
                subs[1..1 + extra_len].copy_from_slice(&extra_subs[..extra_len]);
                let outcome = rs.round_step(cfg, goals, &subs[..1 + extra_len]);
                let v = self.evaluate_outcome(
                    outcome,
                    goals,
                    cfg,
                    seat,
                    active_mask,
                    depth.saturating_sub(1),
                    clash_budget,
                );
                if v > best {
                    best = v;
                }
            }
            if best == f64::NEG_INFINITY {
                evaluate_board(&rs.board, goals, cfg, seat)
            } else {
                best
            }
        } else if opp_opt.is_some() {
            let mut worst = f64::INFINITY;
            for &opp_m in opp_moves.as_slice() {
                let mut subs = [PackedMove::default(); MAX_PLAYERS];
                subs[0] = opp_m;
                subs[1..1 + extra_len].copy_from_slice(&extra_subs[..extra_len]);
                let outcome = rs.round_step(cfg, goals, &subs[..1 + extra_len]);
                let v = self.evaluate_outcome(
                    outcome,
                    goals,
                    cfg,
                    seat,
                    active_mask,
                    depth.saturating_sub(1),
                    clash_budget,
                );
                if v < worst {
                    worst = v;
                }
            }
            if worst == f64::INFINITY {
                evaluate_board(&rs.board, goals, cfg, seat)
            } else {
                worst
            }
        } else {
            evaluate_board(&rs.board, goals, cfg, seat)
        }
    }
}

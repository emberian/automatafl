//! Decoupled UCT (DUCB) Simultaneous-Move Monte Carlo Tree Search over [`PackedBoard`].
//!
//! At every tree node representing a public [`PackedRoundState`], each waiting player
//! independently maintains and selects from its own action statistics `(visits, value_sum, prior)`
//! without conditioning on the opponent's secret move. Joint action pairs transition through
//! `PackedRoundState::round_step` (supporting both `.resolved` turns and `.again` conflict
//! re-entry sub-rounds), followed by allocation-free playouts on [`PackedBoard`].

use smallvec::SmallVec;

use crate::SplitMix64;
use crate::eval::{evaluate_board, fast_rollout_move_score, rank_candidate_moves_scored};
use crate::packed::{
    GameConfig, MAX_PLAYERS, MAX_TACTICAL_MOVES, MoveList, PackedGoals, PackedMove,
    PackedRoundOutcome, PackedRoundState,
};

/// Configuration for [`DecoupledUctBot`].
#[derive(Clone, Copy, Debug)]
pub struct DuctConfig {
    /// Number of MCTS simulations per move decision.
    pub num_simulations: usize,
    /// Exploration constant (`c_puct` / `c_uct`).
    pub exploration_constant: f64,
    /// Maximum candidate moves expanded per player at each node.
    pub max_actions_per_seat: usize,
    /// Maximum rollout depth (in full turns) during leaf evaluation.
    pub rollout_depth: u8,
    /// Epsilon probability of random tactical exploration during rollouts.
    pub rollout_epsilon: f64,
}

impl Default for DuctConfig {
    fn default() -> Self {
        Self {
            num_simulations: 256,
            exploration_constant: 1.35,
            max_actions_per_seat: 18,
            rollout_depth: 6,
            rollout_epsilon: 0.25,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DuctAction {
    mv: PackedMove,
    visits: u32,
    value_sum: f64,
    prior: f32,
}

struct DuctNode {
    state: PackedRoundState,
    terminal_winner: Option<u8>,
    is_terminal: bool,
    depth_turns: u8,
    total_visits: u32,
    actions: [SmallVec<[DuctAction; 20]>; MAX_PLAYERS],
    children: SmallVec<[([u8; MAX_PLAYERS], u32); 24]>,
}

impl DuctNode {
    fn new(
        state: PackedRoundState,
        terminal_winner: Option<u8>,
        is_terminal: bool,
        depth_turns: u8,
        goals: &PackedGoals,
        cfg: &GameConfig,
        max_actions: usize,
    ) -> Self {
        let mut actions = [
            SmallVec::new(),
            SmallVec::new(),
            SmallVec::new(),
            SmallVec::new(),
        ];
        if !is_terminal {
            for seat in 0..MAX_PLAYERS as u8 {
                if state.is_waiting(seat) {
                    let mut candidates = MoveList::new();
                    let mut cand_scores = [0.0f32; MAX_TACTICAL_MOVES];
                    rank_candidate_moves_scored(
                        &state,
                        goals,
                        cfg,
                        seat,
                        max_actions,
                        &mut candidates,
                        &mut cand_scores,
                    );
                    let slice = candidates.as_slice();
                    if !slice.is_empty() {
                        // Compute softmax-like priors from public solo evaluation
                        let mut raw_scores = [0.0f64; 32];
                        let k = slice.len().min(32);
                        let mut max_s = f64::NEG_INFINITY;
                        for i in 0..k {
                            let s = cand_scores[i] as f64;
                            raw_scores[i] = s;
                            if s > max_s {
                                max_s = s;
                            }
                        }
                        let mut sum_exp = 0.0f64;
                        for s in &mut raw_scores[..k] {
                            *s = ((*s - max_s) * 4.0).exp();
                            sum_exp += *s;
                        }
                        let inv = if sum_exp > 0.0 {
                            1.0 / sum_exp
                        } else {
                            1.0 / (k as f64)
                        };
                        for (i, &m) in slice[..k].iter().enumerate() {
                            actions[seat as usize].push(DuctAction {
                                mv: m,
                                visits: 0,
                                value_sum: 0.0,
                                prior: (raw_scores[i] * inv) as f32,
                            });
                        }
                    }
                }
            }
        }
        Self {
            state,
            terminal_winner,
            is_terminal,
            depth_turns,
            total_visits: 0,
            actions,
            children: SmallVec::new(),
        }
    }

    /// Select an action index for `seat` using ONLY `seat`'s own decoupled statistics.
    #[inline]
    fn select_decoupled_action(&self, seat: u8, c_uct: f64, rng: &mut SplitMix64) -> Option<u8> {
        if (seat as usize) >= MAX_PLAYERS {
            return None;
        }
        let acts = &self.actions[seat as usize];
        if acts.is_empty() {
            return None;
        }
        let sqrt_parent = ((self.total_visits.max(1)) as f64).sqrt();
        let mut best_idx = 0usize;
        let mut best_score = f64::NEG_INFINITY;

        for (i, act) in acts.iter().enumerate() {
            let q = if act.visits == 0 {
                0.0
            } else {
                act.value_sum / (act.visits as f64)
            };
            let u = c_uct * (act.prior as f64) * (sqrt_parent / (1.0 + act.visits as f64));
            // Tiny random jitter to break exact ties symmetrically
            let jitter = rng.next_f64() * 1e-6;
            let score = q + u + jitter;
            if score > best_score {
                best_score = score;
                best_idx = i;
            }
        }
        Some(best_idx as u8)
    }

    #[inline]
    fn find_child(&self, acts: [u8; MAX_PLAYERS]) -> Option<u32> {
        for &(c_acts, child_id) in &self.children {
            if c_acts == acts {
                return Some(child_id);
            }
        }
        None
    }
}

/// Decoupled UCT simultaneous-move search bot.
pub struct DecoupledUctBot {
    pub config: DuctConfig,
    rng: SplitMix64,
    arena: Vec<DuctNode>,
    pub last_root_visits: u32,
}

impl DecoupledUctBot {
    pub fn new(config: DuctConfig, seed: u64) -> Self {
        Self {
            config,
            rng: SplitMix64::new(seed),
            arena: Vec::with_capacity(512),
            last_root_visits: 0,
        }
    }

    /// Choose a move for `seat` from the public [`PackedRoundState`], running `num_simulations`
    /// Decoupled UCT simulations over [`PackedBoard`].
    pub fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        if (seat as usize) >= MAX_PLAYERS || !rs.is_waiting(seat) {
            return None;
        }
        let active_mask = rs.active_player_mask(goals);

        self.arena.clear();
        let root = DuctNode::new(
            *rs,
            None,
            false,
            0,
            goals,
            cfg,
            self.config.max_actions_per_seat,
        );
        if root.actions[seat as usize].is_empty() {
            return None;
        }
        if root.actions[seat as usize].len() == 1 {
            self.last_root_visits = 1;
            return Some(root.actions[seat as usize][0].mv);
        }
        self.arena.push(root);

        for _ in 0..self.config.num_simulations {
            self.run_simulation(goals, cfg, active_mask);
        }

        let root_node = &self.arena[0];
        self.last_root_visits = root_node.total_visits;

        // Select the action for `seat` with the highest empirical value among well-visited
        // actions (`visits >= max_visits / 2`), breaking near-ties by visit count.
        let max_visits = root_node.actions[seat as usize]
            .iter()
            .map(|a| a.visits)
            .max()
            .unwrap_or(0);
        let min_qualifying_visits = (max_visits / 2).max(1);

        let mut best_mv = root_node.actions[seat as usize][0].mv;
        let mut best_score = f64::NEG_INFINITY;
        let mut best_visits = 0u32;

        for act in &root_node.actions[seat as usize] {
            if act.visits < min_qualifying_visits {
                continue;
            }
            let q = act.value_sum / (act.visits as f64);
            let score = q + 0.02 * (act.visits as f64) / (max_visits.max(1) as f64);
            if score > best_score + 1e-6 || ((score - best_score).abs() <= 1e-6 && act.visits > best_visits) {
                best_score = score;
                best_visits = act.visits;
                best_mv = act.mv;
            }
        }

        debug_assert_eq!(best_mv.who, seat);
        Some(best_mv)
    }

    /// Return the root visit distribution `Vec<(PackedMove, u32)>` for `seat` after search.
    pub fn root_visit_distribution(&self, seat: u8) -> Vec<(PackedMove, u32)> {
        if self.arena.is_empty() || (seat as usize) >= MAX_PLAYERS {
            return Vec::new();
        }
        self.arena[0].actions[seat as usize]
            .iter()
            .map(|a| (a.mv, a.visits))
            .collect()
    }

    fn run_simulation(&mut self, goals: &PackedGoals, cfg: &GameConfig, active_mask: u8) {
        const NONE_ACT: u8 = 0xFF;
        let mut path: SmallVec<[(u32, [u8; MAX_PLAYERS]); 16]> = SmallVec::new();
        let mut cur_id = 0u32;

        let leaf_vals = loop {
            let node = &self.arena[cur_id as usize];
            if node.is_terminal {
                let mut vals = Self::terminal_or_eval_values(
                    &node.state.board,
                    node.terminal_winner,
                    goals,
                    cfg,
                    active_mask,
                );
                if node.depth_turns > 1 {
                    for v in &mut vals {
                        *v *= 0.85;
                    }
                }
                break vals;
            }

            let mut acts = [NONE_ACT; MAX_PLAYERS];
            let mut any_act = false;
            for s in 0..(MAX_PLAYERS as u8) {
                if (active_mask & (1u8 << s)) != 0 {
                    if let Some(a) = node.select_decoupled_action(
                        s,
                        self.config.exploration_constant,
                        &mut self.rng,
                    ) {
                        acts[s as usize] = a;
                        any_act = true;
                    }
                }
            }

            if !any_act {
                break Self::terminal_or_eval_values(
                    &node.state.board,
                    None,
                    goals,
                    cfg,
                    active_mask,
                );
            }

            path.push((cur_id, acts));

            if let Some(child_id) = node.find_child(acts) {
                cur_id = child_id;
                continue;
            }

            // Construct the joint move submission with distinct seat IDs
            let mut subs = [PackedMove::default(); MAX_PLAYERS];
            let mut subs_len = 0usize;
            for s in 0..MAX_PLAYERS {
                let a = acts[s];
                if a != NONE_ACT {
                    let mv = node.actions[s][a as usize].mv;
                    debug_assert_eq!(mv.who, s as u8);
                    subs[subs_len] = mv;
                    subs_len += 1;
                }
            }

            let parent_state = node.state;
            let parent_depth = node.depth_turns;
            let outcome = parent_state.round_step(cfg, goals, &subs[..subs_len]);

            let (child_state, winner, is_term, next_depth) = match outcome {
                PackedRoundOutcome::Resolved { board, win } => {
                    let d = parent_depth + 1;
                    let term = win.is_some() || d >= self.config.rollout_depth.max(4);
                    (
                        PackedRoundState::open_with_mask(board, active_mask),
                        win,
                        term,
                        d,
                    )
                }
                PackedRoundOutcome::Again(next_rs) => {
                    let d = parent_depth + 1;
                    let term = d >= self.config.rollout_depth.max(4) + 3;
                    (next_rs, None, term, d)
                }
            };

            let child_id = self.arena.len() as u32;
            let child_node = DuctNode::new(
                child_state,
                winner,
                is_term,
                next_depth,
                goals,
                cfg,
                self.config.max_actions_per_seat,
            );
            self.arena.push(child_node);
            self.arena[cur_id as usize].children.push((acts, child_id));

            let vals = if winner.is_some() {
                Self::terminal_or_eval_values(&child_state.board, winner, goals, cfg, active_mask)
            } else {
                self.rollout_values(
                    child_state,
                    goals,
                    cfg,
                    active_mask,
                    self.config.rollout_depth,
                )
            };
            self.arena[child_id as usize].total_visits += 1;
            break vals;
        };

        // Backpropagate decoupled values per seat
        for &(nid, acts) in path.iter().rev() {
            let node = &mut self.arena[nid as usize];
            node.total_visits += 1;
            for s in 0..MAX_PLAYERS {
                let a = acts[s];
                if a != NONE_ACT {
                    let act = &mut node.actions[s][a as usize];
                    act.visits += 1;
                    act.value_sum += leaf_vals[s];
                }
            }
        }
    }

    #[inline]
    fn terminal_or_eval_values(
        board: &crate::packed::PackedBoard,
        winner: Option<u8>,
        goals: &PackedGoals,
        cfg: &GameConfig,
        active_mask: u8,
    ) -> [f64; MAX_PLAYERS] {
        let mut vals = [0.0f64; MAX_PLAYERS];
        if let Some(w) = winner {
            for s in 0..(MAX_PLAYERS as u8) {
                if (active_mask & (1u8 << s)) != 0 {
                    vals[s as usize] = if s == w { 1.0 } else { -1.0 };
                }
            }
        } else if active_mask == 0b0011 {
            let v0 = evaluate_board(board, goals, cfg, 0);
            vals[0] = v0;
            vals[1] = -v0;
        } else {
            for s in 0..(MAX_PLAYERS as u8) {
                if (active_mask & (1u8 << s)) != 0 {
                    vals[s as usize] = evaluate_board(board, goals, cfg, s);
                }
            }
        }
        vals
    }

    /// Allocation-free simulation playout over [`PackedRoundState`], returning the outcome
    /// from Seat 0's perspective in `[-1.0, 1.0]`.
    pub fn rollout(
        &mut self,
        rs: PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        max_turns: u8,
    ) -> f64 {
        let active_mask = rs.active_player_mask(goals);
        self.rollout_values(rs, goals, cfg, active_mask, max_turns)[0]
    }

    fn rollout_values(
        &mut self,
        mut rs: PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        active_mask: u8,
        max_turns: u8,
    ) -> [f64; MAX_PLAYERS] {
        let mut move_buf = MoveList::new();
        let mut turns_elapsed = 0u8;
        let mut sub_rounds = 0u8;

        while turns_elapsed < max_turns && sub_rounds < max_turns.saturating_mul(2).max(4) {
            sub_rounds += 1;
            let mut subs = [PackedMove::default(); MAX_PLAYERS];
            let mut subs_len = 0usize;

            for s in 0..(MAX_PLAYERS as u8) {
                if (active_mask & (1u8 << s)) != 0 && rs.is_waiting(s) {
                    if let Some(mv) = self.pick_rollout_move(&rs, goals, cfg, s, &mut move_buf) {
                        debug_assert_eq!(mv.who, s);
                        subs[subs_len] = mv;
                        subs_len += 1;
                    }
                }
            }

            if subs_len == 0 {
                break;
            }

            match rs.round_step(cfg, goals, &subs[..subs_len]) {
                PackedRoundOutcome::Resolved { board, win } => {
                    if win.is_some() {
                        return Self::terminal_or_eval_values(&board, win, goals, cfg, active_mask);
                    }
                    rs = PackedRoundState::open_with_mask(board, active_mask);
                    turns_elapsed += 1;
                }
                PackedRoundOutcome::Again(next_rs) => {
                    rs = next_rs;
                }
            }
        }

        // At rollout horizon, blend heuristic board evaluation with `adjudicateCapped`
        if active_mask == 0b0011 {
            let eval = evaluate_board(&rs.board, goals, cfg, 0);
            let cap_bonus = match rs.board.adjudicate_capped(goals) {
                Some(0) => 0.15,
                Some(1) => -0.15,
                _ => 0.0,
            };
            let v0 = (0.85 * eval + cap_bonus).clamp(-0.99, 0.99);
            [v0, -v0, 0.0, 0.0]
        } else {
            Self::terminal_or_eval_values(&rs.board, None, goals, cfg, active_mask)
        }
    }

    #[inline]
    fn pick_rollout_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
        buf: &mut MoveList,
    ) -> Option<PackedMove> {
        rs.board
            .generate_tactical_moves(rs.marks, rs.locked_slice(), seat, cfg, buf);
        let slice = buf.as_slice();
        if slice.is_empty() {
            return None;
        }
        if self.rng.gen_bool(self.config.rollout_epsilon) {
            return Some(slice[self.rng.next_usize(slice.len())]);
        }
        // Sample the best of a small random tournament of size 4 using zero-FFI sightline/corner scoring
        let k = slice.len();
        if k <= 4 {
            let mut best = slice[0];
            let mut best_s = fast_rollout_move_score(rs, goals, seat, best);
            for &m in &slice[1..] {
                let s = fast_rollout_move_score(rs, goals, seat, m);
                if s > best_s {
                    best_s = s;
                    best = m;
                }
            }
            Some(best)
        } else {
            let mut best = slice[self.rng.next_usize(k)];
            let mut best_s = fast_rollout_move_score(rs, goals, seat, best);
            for _ in 0..3 {
                let m = slice[self.rng.next_usize(k)];
                let s = fast_rollout_move_score(rs, goals, seat, m);
                if s > best_s {
                    best_s = s;
                    best = m;
                }
            }
            Some(best)
        }
    }
}

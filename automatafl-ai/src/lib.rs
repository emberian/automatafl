//! `automatafl-ai` — Simultaneous-move AI search engines for Automatafl over allocation-free [`PackedBoard`].
//!
//! ## Engines
//! - [`GreedyHeuristicBot`]: Fast 1-ply public sightline & corner-clearance heuristic bot.
//! - [`SimultaneousMinimaxBot`]: Simultaneous-move Maximin / Alpha-Beta matrix search with randomized near-tie breaking.
//! - [`DecoupledUctBot`]: Decoupled UCT (DUCB) simultaneous-move Monte Carlo Tree Search with allocation-free playouts.
//! - [`RegretMatchingBot`]: Regret Matching Plus (RM+) simultaneous-move Nash equilibrium solver.
//!
//! ## Fog-of-War & Simultaneity Guarantees
//! 1. **Zero Move Leakage**: Every bot operates strictly on `&PackedRoundState`, which holds only
//!    public information (`board`, `marks`, `locked`, `waiting_mask`). No uncommitted opponent move
//!    can ever be passed to or inspected by `choose_move`.
//! 2. **Zero Duplicate Moves**: Every bot emits moves tagged with its own `seat` (`m.who == seat`),
//!    and joint transitions in search/playouts never duplicate a single seat's move for another seat.

pub mod analysis;
pub mod duct;
pub mod eval;
pub mod minimax;
pub mod packed;
pub mod regret;
pub mod wasm_api;

pub use analysis::*;
pub use duct::*;
pub use eval::*;
pub use minimax::*;
pub use packed::*;
pub use regret::*;
pub use wasm_api::*;

use serde::{Deserialize, Serialize};

/// Fast, zero-dependency deterministic SplitMix64 PRNG (`wasm32-unknown-unknown` compatible).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    #[inline(always)]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f64` in `[0.0, 1.0)`.
    #[inline(always)]
    pub fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }

    /// Uniform index in `0..upper` (returns `0` if `upper <= 1`).
    /// Uses `u64` modulo so 32-bit (`wasm32`) and 64-bit native targets produce identical choices.
    #[inline(always)]
    pub fn next_usize(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next_u64() % (upper as u64)) as usize
        }
    }

    /// Bernoulli trial with probability `p` in `[0.0, 1.0]`.
    #[inline(always)]
    pub fn gen_bool(&mut self, p: f64) -> bool {
        if p <= 0.0 {
            false
        } else if p >= 1.0 {
            true
        } else {
            self.next_f64() < p
        }
    }
}

/// Core trait for simultaneous-move Automatafl computer players operating over [`PackedRoundState`].
pub trait SimultaneousBot {
    /// Select a legal move for `seat` given ONLY the public [`PackedRoundState`].
    ///
    /// Returns `None` if `seat` is not currently waiting to submit (`!rs.is_waiting(seat)`)
    /// or if no legal move exists.
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove>;
}

/// Fast 1-ply tactical heuristic bot with randomized near-tie selection.
pub struct GreedyHeuristicBot {
    rng: SplitMix64,
}

impl GreedyHeuristicBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SplitMix64::new(seed),
        }
    }
}

impl SimultaneousBot for GreedyHeuristicBot {
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        if !rs.is_waiting(seat) {
            return None;
        }
        let mut ranked = MoveList::new();
        let mut ranked_scores = [0.0f32; MAX_TACTICAL_MOVES];
        rank_candidate_moves_scored(rs, goals, cfg, seat, 8, &mut ranked, &mut ranked_scores);
        let slice = ranked.as_slice();
        if slice.is_empty() {
            return None;
        }
        let top_score = ranked_scores[0];
        let mut tied_count = 1usize;
        for i in 1..slice.len() {
            if ranked_scores[i] >= top_score - 1e-4 {
                tied_count += 1;
            } else {
                break;
            }
        }
        let idx = self.rng.next_usize(tied_count);
        Some(slice[idx])
    }
}

impl SimultaneousBot for SimultaneousMinimaxBot {
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        SimultaneousMinimaxBot::choose_move(self, rs, goals, cfg, seat)
    }
}

impl SimultaneousBot for DecoupledUctBot {
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        DecoupledUctBot::choose_move(self, rs, goals, cfg, seat)
    }
}

impl SimultaneousBot for RegretMatchingBot {
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        RegretMatchingBot::choose_move(self, rs, goals, cfg, seat)
    }
}

/// Selectable AI algorithm family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BotKind {
    /// Fast 1-ply heuristic evaluation.
    Greedy,
    /// Simultaneous-move Maximin / Alpha-Beta matrix search.
    Minimax,
    /// Decoupled UCT (DUCB) simultaneous-move MCTS.
    #[default]
    DecoupledUct,
    /// Regret Matching Plus (RM+) Nash equilibrium solver.
    RegretMatching,
}

impl BotKind {
    /// Decode a `BotKind` from a compact byte tag (`0 = Greedy`, `1 = Minimax`,
    /// `2 = DecoupledUct`, `3 = RegretMatching`).
    #[inline(always)]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => BotKind::Greedy,
            1 => BotKind::Minimax,
            2 => BotKind::DecoupledUct,
            3 => BotKind::RegretMatching,
            _ => BotKind::DecoupledUct,
        }
    }

    /// Encode this `BotKind` as a compact byte tag (`0..=3`).
    #[inline(always)]
    pub const fn to_u8(self) -> u8 {
        match self {
            BotKind::Greedy => 0,
            BotKind::Minimax => 1,
            BotKind::DecoupledUct => 2,
            BotKind::RegretMatching => 3,
        }
    }
}

/// Unified bot engine dispatching across all four simultaneous-move search families.
pub enum BotEngine {
    Greedy(GreedyHeuristicBot),
    Minimax(SimultaneousMinimaxBot),
    DecoupledUct(DecoupledUctBot),
    RegretMatching(RegretMatchingBot),
}

impl BotEngine {
    /// Create a [`BotEngine`] of the requested [`BotKind`] with default parameters and `seed`.
    pub fn new(kind: BotKind, seed: u64) -> Self {
        match kind {
            BotKind::Greedy => BotEngine::Greedy(GreedyHeuristicBot::new(seed)),
            BotKind::Minimax => {
                BotEngine::Minimax(SimultaneousMinimaxBot::new(MinimaxConfig::default(), seed))
            }
            BotKind::DecoupledUct => {
                BotEngine::DecoupledUct(DecoupledUctBot::new(DuctConfig::default(), seed))
            }
            BotKind::RegretMatching => {
                BotEngine::RegretMatching(RegretMatchingBot::new(RegretConfig::default(), seed))
            }
        }
    }

    pub fn kind(&self) -> BotKind {
        match self {
            BotEngine::Greedy(_) => BotKind::Greedy,
            BotEngine::Minimax(_) => BotKind::Minimax,
            BotEngine::DecoupledUct(_) => BotKind::DecoupledUct,
            BotEngine::RegretMatching(_) => BotKind::RegretMatching,
        }
    }
}

impl SimultaneousBot for BotEngine {
    fn choose_move(
        &mut self,
        rs: &PackedRoundState,
        goals: &PackedGoals,
        cfg: &GameConfig,
        seat: u8,
    ) -> Option<PackedMove> {
        match self {
            BotEngine::Greedy(b) => b.choose_move(rs, goals, cfg, seat),
            BotEngine::Minimax(b) => b.choose_move(rs, goals, cfg, seat),
            BotEngine::DecoupledUct(b) => b.choose_move(rs, goals, cfg, seat),
            BotEngine::RegretMatching(b) => b.choose_move(rs, goals, cfg, seat),
        }
    }
}

/// High-level wire-compatible helper: decode a public round state from wire/coordinates and
/// compute `seat`'s move using `bot`, returning `Ok(Some((from_x, from_y, to_x, to_y)))`.
pub fn choose_move_from_wire(
    bot: &mut dyn SimultaneousBot,
    board_wire: &str,
    marks: &[(i32, i32)],
    locked: &[(u32, (i32, i32), (i32, i32))],
    waiting: &[u32],
    seat: u32,
    cfg: &GameConfig,
) -> Result<Option<(i32, i32, i32, i32)>, String> {
    let board = PackedBoard::from_wire(board_wire)?;
    let mut marks_mask = BitBoard128::EMPTY;
    for &(mx, my) in marks {
        if mx >= 0 && my >= 0 {
            let pc = PackedCoord::new(mx as u8, my as u8);
            if board.in_bounds(pc) {
                marks_mask.set(board.idx(pc));
            }
        }
    }
    let mut locked_arr = [PackedMove::default(); MAX_PLAYERS];
    let mut locked_len = 0usize;
    for &(who, (fx, fy), (tx, ty)) in locked {
        if locked_len < MAX_PLAYERS && fx >= 0 && fy >= 0 && tx >= 0 && ty >= 0 {
            locked_arr[locked_len] = PackedMove::new(
                who as u8,
                PackedCoord::new(fx as u8, fy as u8),
                PackedCoord::new(tx as u8, ty as u8),
            );
            locked_len += 1;
        }
    }
    let mut waiting_mask = 0u8;
    for &w in waiting {
        if w < 8 {
            waiting_mask |= 1u8 << w;
        }
    }
    let rs = PackedRoundState {
        board,
        marks: marks_mask,
        locked: locked_arr,
        locked_len: locked_len as u8,
        waiting_mask,
    };
    let goals = PackedGoals::stock_two_player(board.size);
    Ok(bot
        .choose_move(&rs, &goals, cfg, seat as u8)
        .map(|m| (m.frm.x as i32, m.frm.y as i32, m.to.x as i32, m.to.y as i32)))
}

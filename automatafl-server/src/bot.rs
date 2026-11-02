//! Simultaneous-move AI bot opponent integration for server-hosted and local matches.
//!
//! ## Strict Fog-of-War & Zero Move-Leakage Guarantee
//! [`choose_bot_move`] accepts ONLY public game state (`board`, `goals`, `marks`, `locked`,
//! `bot_seat`, `seed`). The human opponent's secret committed move is structurally unrepresentable
//! in the arguments passed to the bot, making move leakage impossible.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::oracle::{executable_targets, legal_targets, move_legal};
use crate::protocol::{AUTO, Board, Coord, GameConfig, Move, Seat, VAC};

/// Selectable computer opponent strategy / difficulty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BotStrategy {
    /// Random legal executable move (preferring sightlines).
    Novice,
    /// Sightline-aware tactical greedy heuristic.
    #[default]
    Tactical,
    /// Simultaneous-move maximin / minimax over active sightline candidates.
    Minimax,
    /// Decoupled UCT (Monte Carlo Tree Search for simultaneous-move games).
    DecoupledUct,
    /// Regret-matching mixed-strategy equilibrium sampler over the simultaneous payoff matrix.
    RegretMatching,
}

impl BotStrategy {
    pub fn label(self) -> &'static str {
        match self {
            BotStrategy::Novice => "Novice Bot",
            BotStrategy::Tactical => "Tactical Sightline Bot",
            BotStrategy::Minimax => "Simultaneous Minimax",
            BotStrategy::DecoupledUct => "Decoupled UCT (MCTS)",
            BotStrategy::RegretMatching => "Regret Matching (Mixed Nash)",
        }
    }

    pub fn to_ai_kind(self) -> Option<automatafl_ai::BotKind> {
        match self {
            BotStrategy::Novice => None,
            BotStrategy::Tactical => Some(automatafl_ai::BotKind::Greedy),
            BotStrategy::Minimax => Some(automatafl_ai::BotKind::Minimax),
            BotStrategy::DecoupledUct => Some(automatafl_ai::BotKind::DecoupledUct),
            BotStrategy::RegretMatching => Some(automatafl_ai::BotKind::RegretMatching),
        }
    }
}

type ExternalBotFn = fn(
    BotStrategy,
    &Board,
    &[(Coord, u32)],
    &[Coord],
    &[Move],
    Seat,
    u64,
) -> Option<Move>;

static EXTERNAL_BOT_ENGINE: OnceLock<ExternalBotFn> = OnceLock::new();

/// Register an external `automatafl-ai` engine implementation.
pub fn register_ai_engine(engine: ExternalBotFn) {
    let _ = EXTERNAL_BOT_ENGINE.set(engine);
}

/// Deterministic PRNG step (SplitMix64) so bot choices are reproducible from `(seed, turn_no, round_no)`.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Enumerate all legal executable moves for `seat` on `board` given `marks` and `locked`,
/// falling back to proposable legal moves if every executable move is blocked.
pub fn candidate_moves(
    board: &Board,
    marks: &[Coord],
    locked: &[Move],
    seat: Seat,
) -> Vec<Move> {
    candidate_moves_filtered(board, marks, locked, &[], seat)
}

pub fn candidate_moves_filtered(
    board: &Board,
    marks: &[Coord],
    locked: &[Move],
    cooldown_squares: &[Coord],
    seat: Seat,
) -> Vec<Move> {
    let who = seat.pid();
    let mut live_moves = Vec::new();
    let mut fallback_moves = Vec::new();

    for idx in 0..(board.n * board.n) {
        let frm = board.coord_of(idx);
        let p = board.cell_at(frm);
        if p == VAC || p == AUTO || marks.contains(&frm) || cooldown_squares.contains(&frm) {
            continue;
        }
        if let Ok(live) = executable_targets(board, marks, locked, who, frm) {
            for to in live {
                live_moves.push(Move { who, frm, to });
            }
        }
        if live_moves.is_empty() {
            if let Ok(targets) = legal_targets(board, marks, who, frm) {
                for to in targets {
                    fallback_moves.push(Move { who, frm, to });
                }
            }
        }
    }

    if !live_moves.is_empty() {
        live_moves
    } else if !fallback_moves.is_empty() {
        fallback_moves
    } else {
        // When all pieces are marked or on cooldown, any unmarked non-cooldown vacuum rook line is a legal pass move
        for idx in 0..(board.n * board.n) {
            let frm = board.coord_of(idx);
            if board.cell_at(frm) == AUTO
                || frm == board.auto
                || marks.contains(&frm)
                || cooldown_squares.contains(&frm)
            {
                continue;
            }
            if let Ok(targets) = legal_targets(board, marks, who, frm) {
                for to in targets {
                    fallback_moves.push(Move { who, frm, to });
                }
            }
        }
        fallback_moves
    }
}

fn to_packed_board(
    board: &Board,
    last_auto_step: Option<Coord>,
    cooldown_squares: &[Coord],
) -> Option<automatafl_ai::PackedBoard> {
    if board.n > automatafl_ai::MAX_BOARD_SIZE {
        return None;
    }
    let mut pb = automatafl_ai::PackedBoard::from_wire(&board.to_wire()).ok()?;
    if let Some((dx, dy)) = last_auto_step {
        pb.last_dx = (dx as i8).signum();
        pb.last_dy = (dy as i8).signum();
    }
    for &(cx, cy) in cooldown_squares {
        if cx >= 0 && cy >= 0 {
            let pc = automatafl_ai::PackedCoord::new(cx as u8, cy as u8);
            if pb.in_bounds(pc) {
                pb.cooldown_mask.set(pb.idx(pc));
            }
        }
    }
    Some(pb)
}

fn to_packed_goals(size: usize, goals: &[(Coord, u32)]) -> automatafl_ai::PackedGoals {
    let mut pg = automatafl_ai::PackedGoals::empty();
    for &(c, pid) in goals {
        if c.0 >= 0
            && (c.0 as usize) < size
            && c.1 >= 0
            && (c.1 as usize) < size
            && (pg.len as usize) < pg.entries.len()
        {
            pg.entries[pg.len as usize] = (
                automatafl_ai::PackedCoord::new(c.0 as u8, c.1 as u8),
                pid as u8,
            );
            pg.len += 1;
        }
    }
    pg
}

/// Choose a single legal move for `bot_seat` using only public state.
pub fn choose_bot_move(
    strategy: BotStrategy,
    board: &Board,
    goals: &[(Coord, u32)],
    marks: &[Coord],
    locked: &[Move],
    bot_seat: Seat,
    seed: u64,
) -> Option<Move> {
    let mut cfg = GameConfig::default();
    cfg.tie_break = if board.col_rule {
        crate::protocol::TieBreakMode::Column
    } else {
        crate::protocol::TieBreakMode::Row
    };
    choose_bot_move_with_config(
        strategy,
        &cfg,
        None,
        &[],
        board,
        goals,
        marks,
        locked,
        bot_seat,
        seed,
    )
}

/// Config- and cooldown-aware bot move selector delegating to `automatafl_ai::BotEngine`
/// over `PackedBoard` while strictly isolating secret moves.
pub fn choose_bot_move_with_config(
    strategy: BotStrategy,
    cfg: &GameConfig,
    last_auto_step: Option<Coord>,
    cooldown_squares: &[Coord],
    board: &Board,
    goals: &[(Coord, u32)],
    marks: &[Coord],
    locked: &[Move],
    bot_seat: Seat,
    seed: u64,
) -> Option<Move> {
    if let Some(ext) = EXTERNAL_BOT_ENGINE.get() {
        if let Some(mv) = ext(strategy, board, goals, marks, locked, bot_seat, seed) {
            if move_legal(board, marks, &mv).unwrap_or(false)
                && !cooldown_squares.contains(&mv.frm)
            {
                return Some(mv);
            }
        }
    }

    let rng_seed = seed ^ ((bot_seat.pid() as u64).wrapping_mul(0x517C_C1B7_2722_0A95));

    // Primary path: delegate directly to `automatafl_ai::BotEngine` operating on `PackedBoard`
    if let (Some(ai_kind), Some(pb)) = (
        strategy.to_ai_kind(),
        to_packed_board(board, last_auto_step, cooldown_squares),
    ) {
        use automatafl_ai::SimultaneousBot;
        let mut waiting_mask = 1u8 << (bot_seat.pid() & 7);
        let opp_pid = bot_seat.other().pid();
        if !locked.iter().any(|m| m.who == opp_pid) {
            waiting_mask |= 1u8 << (opp_pid & 7);
        }
        let mut rs = automatafl_ai::PackedRoundState {
            board: pb,
            marks: automatafl_ai::BitBoard128::EMPTY,
            locked: [automatafl_ai::PackedMove::default(); automatafl_ai::MAX_PLAYERS],
            locked_len: 0,
            waiting_mask,
        };
        for &(mx, my) in marks {
            if mx >= 0 && my >= 0 {
                let pc = automatafl_ai::PackedCoord::new(mx as u8, my as u8);
                if rs.board.in_bounds(pc) {
                    rs.marks.set(rs.board.idx(pc));
                }
            }
        }
        for m in locked {
            if (rs.locked_len as usize) < rs.locked.len()
                && m.frm.0 >= 0
                && m.frm.1 >= 0
                && m.to.0 >= 0
                && m.to.1 >= 0
            {
                rs.locked[rs.locked_len as usize] = automatafl_ai::PackedMove::new(
                    m.who as u8,
                    automatafl_ai::PackedCoord::new(m.frm.0 as u8, m.frm.1 as u8),
                    automatafl_ai::PackedCoord::new(m.to.0 as u8, m.to.1 as u8),
                );
                rs.locked_len += 1;
            }
        }
        let pg = to_packed_goals(board.n, goals);
        let ai_cfg = cfg.to_ai();
        let mut engine = match ai_kind {
            automatafl_ai::BotKind::Greedy => {
                automatafl_ai::BotEngine::Greedy(automatafl_ai::GreedyHeuristicBot::new(rng_seed))
            }
            automatafl_ai::BotKind::Minimax => automatafl_ai::BotEngine::Minimax(
                automatafl_ai::SimultaneousMinimaxBot::new(
                    automatafl_ai::MinimaxConfig {
                        max_depth: 1,
                        root_branching: 8,
                        inner_branching: 4,
                        tie_epsilon: 1e-4,
                    },
                    rng_seed,
                ),
            ),
            automatafl_ai::BotKind::DecoupledUct => automatafl_ai::BotEngine::DecoupledUct(
                automatafl_ai::DecoupledUctBot::new(
                    automatafl_ai::DuctConfig {
                        num_simulations: 32,
                        exploration_constant: 1.35,
                        max_actions_per_seat: 8,
                        rollout_depth: 3,
                        rollout_epsilon: 0.25,
                    },
                    rng_seed,
                ),
            ),
            automatafl_ai::BotKind::RegretMatching => automatafl_ai::BotEngine::RegretMatching(
                automatafl_ai::RegretMatchingBot::new(
                    automatafl_ai::RegretConfig {
                        candidate_count: 8,
                        iterations: 100,
                        support_threshold: 0.02,
                        one_ply_lookahead: false,
                    },
                    rng_seed,
                ),
            ),
        };
        if let Some(ai_mv) = engine.choose_move(&rs, &pg, &ai_cfg, bot_seat.pid() as u8) {
            let mv = Move {
                who: bot_seat.pid(),
                frm: (ai_mv.frm.x as i32, ai_mv.frm.y as i32),
                to: (ai_mv.to.x as i32, ai_mv.to.y as i32),
            };
            if move_legal(board, marks, &mv).unwrap_or(false)
                && !cooldown_squares.contains(&mv.frm)
            {
                return Some(mv);
            }
        }
    }

    let mut rng = rng_seed;
    let all = candidate_moves_filtered(board, marks, locked, cooldown_squares, bot_seat);
    if all.is_empty() {
        return None;
    }
    let idx = (splitmix64(&mut rng) as usize) % all.len();
    Some(all[idx])
}

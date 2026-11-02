//! Composable `MoveGenerator` adapters backed by `automatafl-ai`'s simultaneous-move search
//! engines (`DecoupledUctBot`, `SimultaneousMinimaxBot`, `RegretMatchingBot`, `GreedyHeuristicBot`).
//!
//! Each waiting seat selects its move independently from the public [`PackedRoundState`],
//! never inspecting uncommitted moves of other waiting seats and never duplicating a move across seats.

use super::MoveGenerator;
use crate::*;
use automatafl_ai::{
    BotEngine, BotKind, ConfluenceMode, GameConfig, PackedBoard, PackedCoord, PackedGoals,
    PackedMove, PackedRoundState, SimultaneousBot, TieBreakMode, MAX_PLAYERS,
};
use smallvec::SmallVec;

fn game_to_packed_round_state(game: &Game) -> PackedRoundState {
    let sz = game.board.size.x;
    let ax = game.board.automaton_location.x;
    let ay = game.board.automaton_location.y;
    let mut board = PackedBoard::empty(sz, ax, ay, game.use_column_rule);
    for y in 0..sz {
        for x in 0..sz {
            let c = Coord { x, y };
            let pc = PackedCoord::new(x, y);
            let idx = board.idx(pc);
            match game.board.particles[c.ix()].what {
                Particle::Repulsor => board.repulsors.set(idx),
                Particle::Attractor => board.attractors.set(idx),
                Particle::Automaton => {
                    board.auto_x = x;
                    board.auto_y = y;
                    board.automata.set(idx);
                }
                Particle::Vacuum => {}
            }
        }
    }
    let mut rs = PackedRoundState::open(board, game.player_count);
    for &c in &game.board.conflict_list {
        let idx = rs.board.idx(PackedCoord::new(c.x, c.y));
        rs.marks.set(idx);
    }
    for m in &game.pending_moves {
        if game.locked_players.contains(&m.who) && (rs.locked_len as usize) < MAX_PLAYERS {
            let idx = rs.locked_len as usize;
            rs.locked[idx] = PackedMove::new(
                m.who.0,
                PackedCoord::new(m.from.x, m.from.y),
                PackedCoord::new(m.to.x, m.to.y),
            );
            rs.locked_len += 1;
            rs.waiting_mask &= !(1u8 << m.who.0);
        }
    }
    rs
}

/// Simultaneous-move AI generator that runs an independent [`BotEngine`] per player seat
/// over the public [`PackedRoundState`].
pub struct SimultaneousAiGen {
    bots: Vec<BotEngine>,
    kind: BotKind,
}

impl SimultaneousAiGen {
    /// Create a simultaneous-move AI generator for up to `max_seats` players using `kind`.
    pub fn new(kind: BotKind, seed: u64) -> Self {
        let mut bots = Vec::with_capacity(4);
        for s in 0..4u64 {
            // Distinct deterministic seed per seat so symmetric seats sample independently
            let seat_seed = seed.wrapping_add(s.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            bots.push(BotEngine::new(kind, seat_seed));
        }
        Self { bots, kind }
    }

    pub fn kind(&self) -> BotKind {
        self.kind
    }
}

impl MoveGenerator for SimultaneousAiGen {
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]> {
        let mut out = SmallVec::new();
        let rs = game_to_packed_round_state(game);

        let mut goals = PackedGoals::empty();
        for &(gc, pid) in &game.goals {
            if (goals.len as usize) < goals.entries.len() {
                goals.entries[goals.len as usize] =
                    (automatafl_ai::PackedCoord::new(gc.x, gc.y), pid.0);
                goals.len += 1;
            }
        }
        if goals.len == 0 {
            if game.player_count == 4 {
                goals = PackedGoals::stock_four_player(rs.board.size);
            } else {
                goals = PackedGoals::stock_two_player(rs.board.size);
            }
        }

        let confluence = match game.merge_resolution_mode {
            MergeResolutionMode::DetectAndConflict => ConfluenceMode::Conflict,
            MergeResolutionMode::BunchBeforeMerge | MergeResolutionMode::BunchedStacking => {
                ConfluenceMode::BunchBefore
            }
            MergeResolutionMode::Annihilate => ConfluenceMode::Annihilate,
        };

        let cfg = GameConfig {
            tie_break: if game.use_column_rule {
                TieBreakMode::Column
            } else {
                TieBreakMode::Freeze
            },
            confluence,
            ..GameConfig::default()
        };

        for seat in 0..game.player_count {
            let pid = Pid(seat);
            if game.locked_players.contains(&pid) {
                continue;
            }
            while self.bots.len() <= seat as usize {
                let s = self.bots.len() as u64;
                self.bots
                    .push(BotEngine::new(self.kind, 0xCAFE_BABE_u64.wrapping_add(s)));
            }
            if let Some(pm) = self.bots[seat as usize].choose_move(&rs, &goals, &cfg, seat) {
                out.push(Move {
                    who: pid,
                    from: Coord {
                        x: pm.frm.x,
                        y: pm.frm.y,
                    },
                    to: Coord {
                        x: pm.to.x,
                        y: pm.to.y,
                    },
                });
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_ai_gen_plays_valid_rounds_all_bot_kinds() {
        for kind in [
            BotKind::Greedy,
            BotKind::Minimax,
            BotKind::RegretMatching,
            BotKind::DecoupledUct,
        ] {
            let mut game = Game::new_default_modes(Board::stock_testing(), 2, true);
            let mut generator = SimultaneousAiGen::new(kind, 42);
            for _step in 0..8 {
                let moves = generator.generate_moves(&game);
                assert!(
                    !moves.is_empty(),
                    "Expected simultaneous AI ({kind:?}) to produce moves for waiting players"
                );
                // Check no duplicate player seats in generated simultaneous move batch
                for i in 0..moves.len() {
                    for j in (i + 1)..moves.len() {
                        assert_ne!(
                            moves[i].who, moves[j].who,
                            "Duplicate move for player {:?}",
                            moves[i].who
                        );
                    }
                }
                for m in moves {
                    let fb = game.propose_move(m);
                    assert!(
                        matches!(
                            fb,
                            ProposeFeedback::Accepted | ProposeFeedback::AcceptedAndReady
                        ),
                        "AI generated move should be accepted, got {fb:?}"
                    );
                }
                let round_fb = game.try_complete_round();
                assert!(
                    matches!(
                        round_fb,
                        CompleteRoundFeedback::CompletedMoves(_)
                            | CompleteRoundFeedback::Conflict(_)
                    ),
                    "Expected round completion or conflict, got {round_fb:?}"
                );
                if game.winner.is_some() || game.round == RoundState::GameOver {
                    break;
                }
            }
        }
    }

    #[test]
    fn simultaneous_ai_gen_four_player_all_bot_kinds_no_duplicate_or_panic() {
        for kind in [
            BotKind::Greedy,
            BotKind::Minimax,
            BotKind::RegretMatching,
            BotKind::DecoupledUct,
        ] {
            let mut game = Game::new_default_modes(Board::stock_testing(), 4, true);
            let mut generator = SimultaneousAiGen::new(kind, 99);
            for _step in 0..4 {
                let moves = generator.generate_moves(&game);
                assert!(!moves.is_empty());
                for i in 0..moves.len() {
                    for j in (i + 1)..moves.len() {
                        assert_ne!(
                            moves[i].who, moves[j].who,
                            "4P {kind:?} emitted duplicate seat {:?}",
                            moves[i].who
                        );
                    }
                }
                for m in moves {
                    let fb = game.propose_move(m);
                    assert!(
                        matches!(
                            fb,
                            ProposeFeedback::Accepted | ProposeFeedback::AcceptedAndReady
                        ),
                        "4P AI generated move should be accepted, got {fb:?}"
                    );
                }
                let round_fb = game.try_complete_round();
                assert!(
                    matches!(
                        round_fb,
                        CompleteRoundFeedback::CompletedMoves(_)
                            | CompleteRoundFeedback::Conflict(_)
                    ),
                    "Expected 4P round completion or conflict, got {round_fb:?}"
                );
                if game.winner.is_some() || game.round == RoundState::GameOver {
                    break;
                }
            }
        }
    }
}



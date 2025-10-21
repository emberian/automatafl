use automatafl_fastlogic::{BoardBits, Game, Pid};
use burn::tensor::backend::Backend;
use std::sync::Arc;

use crate::mcts::{MCTSConfig, MCTSSearch, MoveSelector};
use crate::network::AutomataflNet;

/// Evaluation result
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
}

impl EvaluationResult {
    /// Calculate win rate (wins / total_games)
    pub fn win_rate(&self) -> f32 {
        let total = (self.wins + self.losses + self.draws) as f32;
        if total == 0.0 {
            0.0
        } else {
            self.wins as f32 / total
        }
    }

    /// Check if new model should be accepted (win rate >= threshold)
    pub fn should_accept(&self, threshold: f32) -> bool {
        self.win_rate() >= threshold
    }
}

/// Model evaluator for comparing two models
pub struct Evaluator;

impl Evaluator {
    /// Evaluate model_new against model_old
    ///
    /// Plays `num_games` games with deterministic MCTS (temperature=0).
    /// Returns win/loss/draw statistics from model_new's perspective.
    pub fn evaluate<B: Backend>(
        model_new: Arc<AutomataflNet<B>>,
        model_old: Arc<AutomataflNet<B>>,
        device: B::Device,
        mcts_config: MCTSConfig,
        num_games: usize,
    ) -> EvaluationResult
    where
        B::Device: Clone,
    {
        let mut wins = 0;
        let mut losses = 0;
        let mut draws = 0;

        for game_idx in 0..num_games {
            // Alternate who plays first
            let new_plays_first = game_idx % 2 == 0;

            let outcome = Self::play_game(
                model_new.clone(),
                model_old.clone(),
                device.clone(),
                mcts_config.clone(),
                new_plays_first,
            );

            match outcome {
                GameOutcome::NewWins => wins += 1,
                GameOutcome::OldWins => losses += 1,
                GameOutcome::Draw => draws += 1,
            }
        }

        EvaluationResult {
            wins,
            losses,
            draws,
        }
    }

    /// Play a single game between two models
    fn play_game<B: Backend>(
        model_new: Arc<AutomataflNet<B>>,
        model_old: Arc<AutomataflNet<B>>,
        device: B::Device,
        mcts_config: MCTSConfig,
        new_plays_first: bool,
    ) -> GameOutcome
    where
        B::Device: Clone,
    {
        let board = BoardBits::stock_two_player();
        let mut game = Game::new(board, 2, true);

        let pid_new = if new_plays_first { Pid(0) } else { Pid(1) };
        let pid_old = if new_plays_first { Pid(1) } else { Pid(0) };

        // Create MCTS for both players
        let mcts_new = MCTSSearch::new(mcts_config.clone(), model_new, device.clone());
        let mcts_old = MCTSSearch::new(mcts_config, model_old, device);

        let mut move_count = 0;
        const MAX_MOVES: usize = 200;

        while game.winner.is_none() && move_count < MAX_MOVES {
            // Current player alternates
            let current_player = Pid((move_count % 2) as u8);
            let mcts = if current_player == pid_new {
                &mcts_new
            } else {
                &mcts_old
            };

            // Run MCTS and select best move (deterministic, temperature=0)
            let visit_counts = mcts.search(&game, current_player);
            let best_move = MoveSelector::best_move(&visit_counts);

            match best_move {
                Some(m) => {
                    // Apply move for all players (simplified)
                    let (feedback, _) = game.propose_move(m);

                    if feedback != automatafl_fastlogic::MoveFeedback::Committed {
                        break;
                    }

                    // Duplicate for second player
                    while game.pending_moves.len() < game.player_count as usize {
                        game.propose_move(m);
                    }

                    // Complete round
                    match game.try_complete_round() {
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                None => break,
            }

            move_count += 1;
        }

        // Determine outcome
        match game.winner {
            Some(winner) if winner == pid_new => GameOutcome::NewWins,
            Some(winner) if winner == pid_old => GameOutcome::OldWins,
            _ => GameOutcome::Draw,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum GameOutcome {
    NewWins,
    OldWins,
    Draw,
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::NetConfig;

    type TestBackend = NdArray;

    #[test]
    fn test_evaluation_result_win_rate() {
        let result = EvaluationResult {
            wins: 6,
            losses: 3,
            draws: 1,
        };

        assert!((result.win_rate() - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_should_accept_threshold() {
        let result = EvaluationResult {
            wins: 55,
            losses: 45,
            draws: 0,
        };

        assert!(result.should_accept(0.55));
        assert!(!result.should_accept(0.60));
    }

    #[test]
    fn test_evaluator_basic() {
        let device = NdArrayDevice::Cpu;
        let net_config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };

        let model1 = Arc::new(AutomataflNet::<TestBackend>::new(&net_config, &device));
        let model2 = Arc::new(AutomataflNet::<TestBackend>::new(&net_config, &device));

        let mcts_config = MCTSConfig {
            num_simulations: 5, // Very few for testing
            ..Default::default()
        };

        let result = Evaluator::evaluate(model1, model2, device, mcts_config, 2);

        // Should have played 2 games
        assert_eq!(result.wins + result.losses + result.draws, 2);
    }
}

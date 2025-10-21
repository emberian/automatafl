use automatafl_fastlogic::{BoardBits, Game, Move, Pid};
use burn::tensor::backend::Backend;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::sync::Arc;

use crate::mcts::{MCTSConfig, MCTSSearch, MoveSelector};
use crate::network::{AutomataflNet, BoardEncoder, ACTION_SPACE_SIZE};

/// Training example from self-play
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TrainingExample {
    /// Board state
    pub board: BoardBits,

    /// MCTS policy distribution (probability for each action)
    pub policy: Vec<f32>,

    /// Game outcome from this player's perspective
    pub value: f32,
}

/// Self-play configuration
#[derive(Debug, Clone)]
pub struct SelfPlayConfig {
    /// Number of MCTS simulations per move
    pub mcts_simulations: usize,

    /// Temperature for move sampling (high early, low late)
    pub temperature_init: f32,

    /// Temperature after this many moves
    pub temperature_final: f32,

    /// Move number to switch to final temperature
    pub temperature_threshold: usize,

    /// MCTS exploration constant
    pub c_puct: f32,
}

impl Default for SelfPlayConfig {
    fn default() -> Self {
        Self {
            mcts_simulations: 800,
            temperature_init: 1.0,
            temperature_final: 0.1,
            temperature_threshold: 15,
            c_puct: 1.0,
        }
    }
}

/// Self-play worker that generates training data
pub struct SelfPlayWorker<B: Backend> {
    config: SelfPlayConfig,
    model: Arc<AutomataflNet<B>>,
    device: B::Device,
    rng: StdRng,
}

impl<B: Backend> SelfPlayWorker<B> {
    pub fn new(
        config: SelfPlayConfig,
        model: Arc<AutomataflNet<B>>,
        device: B::Device,
        seed: u64,
    ) -> Self {
        Self {
            config,
            model,
            device,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Play one complete game and return training examples
    pub fn play_game(&mut self, player_id: Pid) -> Vec<TrainingExample> {
        let mut examples = Vec::new();

        // Initialize game
        let board = BoardBits::stock_two_player();
        let mut game = Game::new(board, 2, true);

        let mcts_config = MCTSConfig {
            num_simulations: self.config.mcts_simulations,
            c_puct: self.config.c_puct,
            ..Default::default()
        };

        let mcts = MCTSSearch::new(
            mcts_config,
            self.model.clone(),
            self.device.clone(),
        );

        let mut move_count = 0;

        // Play until game over
        while game.winner.is_none() {
            // Run MCTS
            let visit_counts = mcts.search(&game, player_id);

            // Convert to policy distribution for training
            let policy = MoveSelector::visit_counts_to_policy(&visit_counts, ACTION_SPACE_SIZE);

            // Store example (outcome will be filled in later)
            examples.push(TrainingExample {
                board: game.board.clone(),
                policy,
                value: 0.0, // Placeholder
            });

            // Select move based on temperature
            let temperature = if move_count < self.config.temperature_threshold {
                self.config.temperature_init
            } else {
                self.config.temperature_final
            };

            let selected_move = MoveSelector::select_move(&visit_counts, temperature, &mut self.rng)
                .expect("MCTS should return at least one move");

            // Apply move
            let (feedback, ready) = game.propose_move(selected_move);

            if feedback != automatafl_fastlogic::MoveFeedback::Committed {
                // Move was rejected - this shouldn't happen with proper MCTS
                tracing::warn!("Move rejected: {:?}", feedback);
                break;
            }

            // For 2-player game, duplicate move for second player
            while game.pending_moves.len() < game.player_count as usize {
                game.propose_move(selected_move);
            }

            // Complete round
            match game.try_complete_round() {
                Ok(_) => {}
                Err(_) => {
                    // Conflict resolution needed - for now just break
                    break;
                }
            }

            move_count += 1;

            // Safety: prevent infinite games
            if move_count > 200 {
                break;
            }
        }

        // Fill in game outcome for all examples
        let game_value = match game.winner {
            Some(winner) if winner == player_id => 1.0,
            Some(_) => -1.0,
            None => 0.0, // Draw
        };

        for example in &mut examples {
            example.value = game_value;
        }

        examples
    }

    /// Play multiple games and return all training examples
    pub fn play_games(&mut self, num_games: usize, player_id: Pid) -> Vec<TrainingExample> {
        let mut all_examples = Vec::new();

        for _ in 0..num_games {
            let examples = self.play_game(player_id);
            all_examples.extend(examples);
        }

        all_examples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::NetConfig;

    type TestBackend = NdArray;

    #[test]
    fn test_play_game() {
        let device = NdArrayDevice::Cpu;
        let net_config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let model = Arc::new(AutomataflNet::<TestBackend>::new(&net_config, &device));

        let config = SelfPlayConfig {
            mcts_simulations: 10,
            ..Default::default()
        };

        let mut worker = SelfPlayWorker::new(config, model, device, 42);

        let examples = worker.play_game(Pid(0));

        // Should have generated some training examples
        assert!(!examples.is_empty());

        // All examples should have proper shapes
        for example in &examples {
            assert_eq!(example.policy.len(), ACTION_SPACE_SIZE);
            assert!(example.value.abs() <= 1.0);
        }
    }
}

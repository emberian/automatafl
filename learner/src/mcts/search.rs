use automatafl_fastlogic::{BoardBits, Game, Move, Pid, MoveResult, RoundState};
use burn::tensor::backend::Backend;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

use super::node::MCTSNode;
use crate::network::{AutomataflNet, BatchInference, BoardEncoder};

/// MCTS configuration
#[derive(Debug, Clone)]
pub struct MCTSConfig {
    /// Number of simulations per move
    pub num_simulations: usize,

    /// Exploration constant (c_puct in AlphaZero)
    pub c_puct: f32,

    /// Dirichlet noise alpha for root exploration
    pub dirichlet_alpha: f32,

    /// Weight of Dirichlet noise at root
    pub dirichlet_weight: f32,
}

impl Default for MCTSConfig {
    fn default() -> Self {
        Self {
            num_simulations: 800,
            c_puct: 1.0,
            dirichlet_alpha: 0.3,
            dirichlet_weight: 0.25,
        }
    }
}

/// MCTS search implementation with neural network guidance
pub struct MCTSSearch<B: Backend> {
    config: MCTSConfig,
    model: Arc<AutomataflNet<B>>,
    device: B::Device,
}

impl<B: Backend> MCTSSearch<B> {
    pub fn new(config: MCTSConfig, model: Arc<AutomataflNet<B>>, device: B::Device) -> Self {
        Self {
            config,
            model,
            device,
        }
    }

    /// Run MCTS search from the given game state
    ///
    /// Returns: (move -> visit_count) distribution for training
    pub fn search(&self, game: &Game, player: Pid) -> HashMap<Move, u32> {
        let root = Arc::new(RwLock::new(MCTSNode::new(game.clone(), 1.0)));

        // Run simulations
        for _ in 0..self.config.num_simulations {
            self.simulate(root.clone(), player);
        }

        // Extract visit counts
        let root_read = root.read().unwrap();
        root_read
            .children
            .iter()
            .map(|(m, child)| (*m, child.read().unwrap().visit_count))
            .collect()
    }

    /// Single MCTS simulation: select, expand, evaluate, backup
    fn simulate(&self, node: Arc<RwLock<MCTSNode>>, player: Pid) -> f32 {
        // Read node to check terminal/expansion state
        let is_terminal = {
            let n = node.read().unwrap();
            n.terminal
        };

        if is_terminal {
            // Terminal node - return game outcome
            let n = node.read().unwrap();
            return self.evaluate_terminal(&n.state, player);
        }

        // Check if node needs expansion
        let needs_expansion = {
            let n = node.read().unwrap();
            !n.is_fully_expanded()
        };

        if needs_expansion {
            // Expand and evaluate
            let value = self.expand_and_evaluate(node.clone(), player);

            // Backup
            let mut n = node.write().unwrap();
            n.visit_count += 1;
            n.total_value += value;

            return value;
        }

        // Select best child using UCB
        let best_move = self.select_child(node.clone());

        let child = {
            let n = node.read().unwrap();
            n.children.get(&best_move).unwrap().clone()
        };

        // Recurse on child
        let value = self.simulate(child, player);

        // Backup
        let mut n = node.write().unwrap();
        n.visit_count += 1;
        n.total_value += value;

        value
    }

    /// Select best child using UCB formula
    fn select_child(&self, node: Arc<RwLock<MCTSNode>>) -> Move {
        let n = node.read().unwrap();
        let parent_visits = n.visit_count;

        n.children
            .iter()
            .max_by(|(_, a), (_, b)| {
                let a_score = a.read().unwrap().ucb_score(parent_visits, self.config.c_puct);
                let b_score = b.read().unwrap().ucb_score(parent_visits, self.config.c_puct);
                a_score.partial_cmp(&b_score).unwrap()
            })
            .map(|(m, _)| *m)
            .unwrap()
    }

    /// Expand node and evaluate with neural network
    fn expand_and_evaluate(&self, node: Arc<RwLock<MCTSNode>>, player: Pid) -> f32 {
        let (legal_moves, game_clone, board_clone) = {
            let n = node.read().unwrap();
            let moves = n.legal_moves(player);
            (moves, n.state.clone(), n.state.board.clone())
        };

        // Get neural network policy and value
        let (policy, value) = BatchInference::infer_single(&self.model, &board_clone, &self.device);

        // Create children for all legal moves with their prior probabilities
        let mut children = HashMap::new();

        for m in legal_moves {
            let action_idx = BoardEncoder::move_to_action(&m);
            let prior = policy[action_idx];

            // Simulate move to get child state
            if let Some(child_game) = self.apply_move(&game_clone, &m) {
                let child_node = MCTSNode::new(child_game, prior);
                children.insert(m, Arc::new(RwLock::new(child_node)));
            }
        }

        // Update node
        {
            let mut n = node.write().unwrap();
            n.children = children;
            n.expanded = true;
        }

        value
    }

    /// Apply a move and return the resulting game state
    fn apply_move(&self, game: &Game, m: &Move) -> Option<Game> {
        let mut game_clone = game.clone();

        // Propose move
        let (feedback, _ready) = game_clone.propose_move(*m);

        // Check if move was accepted
        if feedback != automatafl_fastlogic::MoveFeedback::Committed {
            return None;
        }

        // For MCTS, we simulate single-player moves by duplicating the move
        // for all players to satisfy the game's multi-player requirement
        while game_clone.pending_moves.len() < game_clone.player_count as usize {
            game_clone.propose_move(*m);
        }

        // Try to complete round
        match game_clone.try_complete_round() {
            Ok(_) => Some(game_clone),
            Err(_) => {
                // Conflict - just return current state
                Some(game_clone)
            }
        }
    }

    /// Evaluate terminal game state
    fn evaluate_terminal(&self, game: &Game, player: Pid) -> f32 {
        match game.winner {
            Some(winner) if winner == player => 1.0,
            Some(_) => -1.0,
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automatafl_fastlogic::BoardBits;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::NetConfig;

    type TestBackend = NdArray;

    #[test]
    fn test_mcts_search_basic() {
        let device = NdArrayDevice::Cpu;
        let net_config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let model = Arc::new(AutomataflNet::<TestBackend>::new(&net_config, &device));

        let mcts_config = MCTSConfig {
            num_simulations: 10,
            ..Default::default()
        };

        let mcts = MCTSSearch::new(mcts_config, model, device);

        let board = BoardBits::stock_testing();
        let game = Game::new(board, 2, true);

        let policy = mcts.search(&game, Pid(0));

        // Should have visited some moves
        assert!(!policy.is_empty());

        // Total visits should sum to num_simulations
        let total_visits: u32 = policy.values().sum();
        assert!(total_visits > 0);
    }
}

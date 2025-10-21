use automatafl_fastlogic::{BoardBits, Game, Move, Pid};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// MCTS tree node with UCB statistics
#[derive(Debug)]
pub struct MCTSNode {
    /// Game state at this node
    pub state: Game,

    /// Number of times this node has been visited
    pub visit_count: u32,

    /// Total value accumulated (sum of all backpropagated values)
    pub total_value: f32,

    /// Prior probability from neural network
    pub prior: f32,

    /// Children nodes (move -> child)
    pub children: HashMap<Move, Arc<RwLock<MCTSNode>>>,

    /// Whether this node has been expanded
    pub expanded: bool,

    /// Whether this is a terminal node
    pub terminal: bool,
}

impl MCTSNode {
    /// Create a new MCTS node
    pub fn new(state: Game, prior: f32) -> Self {
        let terminal = state.winner.is_some();

        Self {
            state,
            visit_count: 0,
            total_value: 0.0,
            prior,
            children: HashMap::new(),
            expanded: false,
            terminal,
        }
    }

    /// Get the mean value (Q-value) of this node
    pub fn mean_value(&self) -> f32 {
        if self.visit_count == 0 {
            0.0
        } else {
            self.total_value / (self.visit_count as f32)
        }
    }

    /// Calculate UCB score for this node
    ///
    /// UCB = Q + c_puct * P * sqrt(N_parent) / (1 + N)
    ///
    /// where:
    /// - Q: mean value
    /// - c_puct: exploration constant
    /// - P: prior probability
    /// - N_parent: parent visit count
    /// - N: this node's visit count
    pub fn ucb_score(&self, parent_visits: u32, c_puct: f32) -> f32 {
        let q = self.mean_value();
        let u = c_puct * self.prior * ((parent_visits as f32).sqrt() / (1.0 + self.visit_count as f32));
        q + u
    }

    /// Check if this node is fully expanded
    pub fn is_fully_expanded(&self) -> bool {
        self.expanded
    }

    /// Get legal moves from this state
    pub fn legal_moves(&self, player: Pid) -> Vec<Move> {
        // Generate all possible moves for the player
        let board = &self.state.board;
        let mut moves = Vec::new();

        for y in 0..automatafl_fastlogic::H {
            for x in 0..automatafl_fastlogic::W {
                let from = automatafl_fastlogic::Coord {
                    x: x as u8,
                    y: y as u8,
                };

                // Skip if no piece at from position
                if board.is_vacuum(from) || board.is_automaton(from) {
                    continue;
                }

                // Try all axis-aligned destinations
                // Horizontal
                for tx in 0..automatafl_fastlogic::W {
                    if tx == x {
                        continue;
                    }
                    let to = automatafl_fastlogic::Coord {
                        x: tx as u8,
                        y: y as u8,
                    };
                    moves.push(Move { who: player, from, to });
                }

                // Vertical
                for ty in 0..automatafl_fastlogic::H {
                    if ty == y {
                        continue;
                    }
                    let to = automatafl_fastlogic::Coord {
                        x: x as u8,
                        y: ty as u8,
                    };
                    moves.push(Move { who: player, from, to });
                }
            }
        }

        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automatafl_fastlogic::BoardBits;

    #[test]
    fn test_node_creation() {
        let board = BoardBits::stock_testing();
        let game = Game::new(board, 2, true);
        let node = MCTSNode::new(game, 0.5);

        assert_eq!(node.visit_count, 0);
        assert_eq!(node.total_value, 0.0);
        assert_eq!(node.prior, 0.5);
        assert!(!node.expanded);
    }

    #[test]
    fn test_mean_value() {
        let board = BoardBits::stock_testing();
        let game = Game::new(board, 2, true);
        let mut node = MCTSNode::new(game, 0.5);

        assert_eq!(node.mean_value(), 0.0);

        node.visit_count = 10;
        node.total_value = 5.0;
        assert_eq!(node.mean_value(), 0.5);
    }

    #[test]
    fn test_ucb_score() {
        let board = BoardBits::stock_testing();
        let game = Game::new(board, 2, true);
        let mut node = MCTSNode::new(game, 0.5);

        node.visit_count = 10;
        node.total_value = 5.0;

        let ucb = node.ucb_score(100, 1.0);
        // Q = 0.5, U = 1.0 * 0.5 * sqrt(100) / (1 + 10) ≈ 0.4545
        assert!((ucb - 0.9545).abs() < 0.01);
    }

    #[test]
    fn test_legal_moves_generation() {
        let board = BoardBits::stock_testing();
        let game = Game::new(board, 2, true);
        let node = MCTSNode::new(game, 0.5);

        let moves = node.legal_moves(Pid(0));
        assert!(!moves.is_empty());

        // All moves should be axis-aligned
        for m in &moves {
            assert!(m.from.x == m.to.x || m.from.y == m.to.y);
            assert!(m.from != m.to);
        }
    }
}

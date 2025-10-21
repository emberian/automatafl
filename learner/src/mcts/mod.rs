//! Monte Carlo Tree Search implementation
//!
//! MCTS with neural network guidance following the AlphaZero algorithm:
//! - Selection: UCB formula with neural network priors
//! - Expansion: create new node and evaluate with network
//! - Backup: propagate value up the tree

mod node;
mod search;
mod policy;

pub use node::MCTSNode;
pub use search::{MCTSSearch, MCTSConfig};
pub use policy::MoveSelector;

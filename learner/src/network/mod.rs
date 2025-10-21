//! Neural network architecture for Automatafl
//!
//! Implements a ResNet-style architecture with two heads:
//! - Policy head: outputs probability distribution over legal moves
//! - Value head: outputs win probability for current player

mod encoder;
mod model;
mod inference;

pub use encoder::{BoardEncoder, ACTION_SPACE_SIZE, INPUT_PLANES};
pub use model::{AutomataflNet, NetConfig};
pub use inference::BatchInference;

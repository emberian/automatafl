//! Composable move generation system for exhaustive testing and AI players.
//!
//! This module provides a flexible architecture where move generation strategies
//! can be composed, combined, and adapted based on game state. The system supports:
//!
//! - **Basic generators**: Random, conflict-seeking, chain-making, goal-oriented
//! - **Combinators**: Sequential, merged, filtered, weighted, adaptive
//! - **AI players**: Minimax, MCTS, AlphaZero-lite
//!
//! # Example
//!
//! ```rust,ignore
//! use automatafl_v8n::*;
//!
//! // Create a composite generator that seeks conflicts 30% of the time,
//! // otherwise generates random moves
//! let mut generator = WeightedGen::new(vec![
//!     (3, Box::new(ConflictSeekingGen::new(42))),
//!     (7, Box::new(RandomMoveGen::new(42))),
//! ]);
//!
//! let moves = generator.generate_moves(&game);
//! ```

use automatafl_logic::*;
use smallvec::SmallVec;

/// Core trait for all move generators.
///
/// Move generators can be basic (producing moves from scratch) or composite
/// (combining/transforming other generators).
pub trait MoveGenerator {
    /// Generate moves for all eligible players given the current game state.
    ///
    /// Returns a SmallVec to avoid allocations for typical 2-4 player games.
    fn generate_moves(&mut self, game: &Game) -> SmallVec<[Move; 2]>;
}

// Re-export basic generators
pub mod basic;
pub use basic::*;

// Composable combinators
pub mod combinators;
pub use combinators::*;
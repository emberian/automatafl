//! Validation, testing, and benchmarking infrastructure for automatafl.
//!
//! This crate contains:
//! - Move generation strategies (composable and exhaustive)
//! - Board builders for various test scenarios
//! - Game recording and replay functionality
//! - Comprehensive test suites
//! - Performance benchmarks
//!
//! The move generation system is designed to be composable, allowing strategies
//! to combine and build on each other for more creative and exhaustive testing.

pub mod board_builders;
pub mod move_gen;
pub mod recording;

pub use automatafl_logic::*;
pub use board_builders::*;
pub use move_gen::*;
pub use recording::*;

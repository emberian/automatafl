//! Model evaluation and tournament system
//!
//! Pit models against each other to measure improvement:
//! - Play games between old and new models
//! - Track win rates and performance metrics
//! - Determine if new model should replace current best

mod evaluator;

pub use evaluator::{Evaluator, EvaluationResult};

//! Training loop and loss functions
//!
//! Implements the training phase of AlphaZero:
//! - Sample batches from replay buffer
//! - Compute policy and value losses
//! - Update network parameters

mod trainer;
mod config;

pub mod loss;

pub use trainer::Trainer;
pub use config::TrainingConfig;

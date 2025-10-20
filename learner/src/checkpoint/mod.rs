//! Checkpoint management for training state persistence
//!
//! Save and load:
//! - Model weights
//! - Optimizer state
//! - Replay buffer
//! - Training iteration counter
//! - RNG state for reproducibility

mod manager;

pub use manager::{CheckpointManager, CheckpointMetadata};

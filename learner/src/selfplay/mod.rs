//! Self-play game generation
//!
//! Distributed self-play workers that generate training data by playing
//! games using MCTS with the current neural network

mod worker;
mod buffer;
mod parallel;

pub use worker::{SelfPlayWorker, SelfPlayConfig, TrainingExample};
pub use buffer::ReplayBuffer;
pub use parallel::ParallelSelfPlay;

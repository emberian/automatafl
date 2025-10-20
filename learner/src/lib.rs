//! AlphaZero-style training pipeline for Automatafl
//!
//! This crate implements a complete reinforcement learning system for training
//! neural networks to play Automatafl using the AlphaZero algorithm:
//!
//! 1. Self-play: Generate games using MCTS guided by current neural network
//! 2. Training: Train network to match MCTS policies and predict game outcomes
//! 3. Evaluation: Test new models against previous best to measure improvement
//!
//! ## Architecture
//!
//! - `network`: Neural network architecture (ResNet) with policy and value heads
//! - `mcts`: Monte Carlo Tree Search implementation with neural network integration
//! - `selfplay`: Distributed self-play game generation
//! - `training`: Training loop, loss functions, and optimization
//! - `arena`: Model evaluation and tournament system
//! - `checkpoint`: Model and training state persistence

pub mod network;
pub mod mcts;
pub mod selfplay;
pub mod training;
pub mod arena;
pub mod checkpoint;

// Re-export key types for convenience
pub use network::{AutomataflNet, BoardEncoder, NetConfig, ACTION_SPACE_SIZE};
pub use mcts::{MCTSConfig, MCTSSearch, MoveSelector};
pub use selfplay::{SelfPlayConfig, SelfPlayWorker, ReplayBuffer, TrainingExample, ParallelSelfPlay};
pub use training::{TrainingConfig, Trainer};
pub use arena::{Evaluator, EvaluationResult};
pub use checkpoint::{CheckpointManager, CheckpointMetadata};

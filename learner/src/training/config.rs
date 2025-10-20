/// Training configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrainingConfig {
    /// Batch size for training
    pub batch_size: usize,

    /// Learning rate for AdamW optimizer
    pub learning_rate: f64,

    /// Weight decay for L2 regularization
    pub weight_decay: f64,

    /// Number of training steps per iteration
    pub train_steps_per_iteration: usize,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            batch_size: 256,
            learning_rate: 0.001,
            weight_decay: 0.0001,
            train_steps_per_iteration: 100,
        }
    }
}

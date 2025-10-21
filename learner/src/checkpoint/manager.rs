use std::path::{Path, PathBuf};
use burn::module::Module;
use burn::record::{FullPrecisionSettings, Recorder};
use burn::tensor::backend::Backend;
use anyhow::{Result, Context};

use crate::network::AutomataflNet;
use crate::selfplay::ReplayBuffer;

/// Training checkpoint metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointMetadata {
    pub iteration: usize,
    pub timestamp: u64,
}

/// Checkpoint manager for saving and loading training state
pub struct CheckpointManager {
    checkpoint_dir: PathBuf,
    keep_last_n: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager
    ///
    /// - `checkpoint_dir`: Directory to store checkpoints
    /// - `keep_last_n`: Number of recent checkpoints to keep (0 = keep all)
    pub fn new(checkpoint_dir: PathBuf, keep_last_n: usize) -> Result<Self> {
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&checkpoint_dir)
            .context("Failed to create checkpoint directory")?;

        Ok(Self {
            checkpoint_dir,
            keep_last_n,
        })
    }

    /// Save a checkpoint
    ///
    /// Saves:
    /// - Model weights
    /// - Replay buffer
    /// - Metadata (iteration, timestamp)
    pub fn save<B: Backend>(
        &self,
        model: &AutomataflNet<B>,
        buffer: &ReplayBuffer,
        iteration: usize,
    ) -> Result<()> {
        let checkpoint_name = format!("checkpoint_{:06}", iteration);
        let checkpoint_path = self.checkpoint_dir.join(&checkpoint_name);

        // Create checkpoint directory
        std::fs::create_dir_all(&checkpoint_path)?;

        // Save model weights
        let model_path = checkpoint_path.join("model.bin");
        let recorder = burn::record::BinFileRecorder::<FullPrecisionSettings>::default();
        model
            .clone()
            .save_file(model_path, &recorder)
            .map_err(|e| anyhow::anyhow!("Failed to save model: {:?}", e))?;

        // Save replay buffer
        let buffer_path = checkpoint_path.join("buffer.bin");
        buffer.save_to_file(&buffer_path)?;

        // Save metadata
        let metadata = CheckpointMetadata {
            iteration,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        let metadata_path = checkpoint_path.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(metadata_path, metadata_json)?;

        // Clean up old checkpoints
        if self.keep_last_n > 0 {
            self.cleanup_old_checkpoints()?;
        }

        Ok(())
    }

    /// Load a checkpoint
    ///
    /// Returns: (model, buffer, metadata)
    ///
    /// Note: Requires the NetConfig to reconstruct the model architecture
    pub fn load<B: Backend>(
        &self,
        iteration: usize,
        net_config: &crate::network::NetConfig,
        device: &B::Device,
    ) -> Result<(AutomataflNet<B>, ReplayBuffer, CheckpointMetadata)> {
        let checkpoint_name = format!("checkpoint_{:06}", iteration);
        let checkpoint_path = self.checkpoint_dir.join(&checkpoint_name);

        if !checkpoint_path.exists() {
            anyhow::bail!("Checkpoint {} not found", checkpoint_name);
        }

        // Load model - create new instance and load weights
        let model_path = checkpoint_path.join("model.bin");
        let recorder = burn::record::BinFileRecorder::<FullPrecisionSettings>::default();
        let record = recorder
            .load(model_path, device)
            .map_err(|e| anyhow::anyhow!("Failed to load model: {:?}", e))?;

        // Create model with loaded record
        let model = AutomataflNet::<B>::new(net_config, device).load_record(record);

        // Load replay buffer
        let buffer_path = checkpoint_path.join("buffer.bin");
        let buffer = ReplayBuffer::load_from_file(&buffer_path)?;

        // Load metadata
        let metadata_path = checkpoint_path.join("metadata.json");
        let metadata_json = std::fs::read_to_string(metadata_path)?;
        let metadata: CheckpointMetadata = serde_json::from_str(&metadata_json)?;

        Ok((model, buffer, metadata))
    }

    /// Get the latest checkpoint iteration
    pub fn latest_iteration(&self) -> Result<Option<usize>> {
        let mut iterations = Vec::new();

        for entry in std::fs::read_dir(&self.checkpoint_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("checkpoint_") {
                if let Some(iter_str) = name_str.strip_prefix("checkpoint_") {
                    if let Ok(iter) = iter_str.parse::<usize>() {
                        iterations.push(iter);
                    }
                }
            }
        }

        Ok(iterations.into_iter().max())
    }

    /// Clean up old checkpoints, keeping only the last N
    fn cleanup_old_checkpoints(&self) -> Result<()> {
        let mut iterations = Vec::new();

        for entry in std::fs::read_dir(&self.checkpoint_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("checkpoint_") && name_str != "checkpoint_best" {
                if let Some(iter_str) = name_str.strip_prefix("checkpoint_") {
                    if let Ok(iter) = iter_str.parse::<usize>() {
                        iterations.push((iter, entry.path()));
                    }
                }
            }
        }

        // Sort by iteration (descending)
        iterations.sort_by(|a, b| b.0.cmp(&a.0));

        // Remove old checkpoints
        for (_, path) in iterations.iter().skip(self.keep_last_n) {
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            }
        }

        Ok(())
    }

    /// Save the best model (special checkpoint that doesn't get deleted)
    pub fn save_best<B: Backend>(&self, model: &AutomataflNet<B>) -> Result<()> {
        let best_path = self.checkpoint_dir.join("checkpoint_best");
        std::fs::create_dir_all(&best_path)?;

        let model_path = best_path.join("model.bin");
        let recorder = burn::record::BinFileRecorder::<FullPrecisionSettings>::default();
        model
            .clone()
            .save_file(model_path, &recorder)
            .map_err(|e| anyhow::anyhow!("Failed to save best model: {:?}", e))?;

        Ok(())
    }

    /// Load the best model
    pub fn load_best<B: Backend>(
        &self,
        net_config: &crate::network::NetConfig,
        device: &B::Device,
    ) -> Result<AutomataflNet<B>> {
        let best_path = self.checkpoint_dir.join("checkpoint_best");
        let model_path = best_path.join("model.bin");

        if !model_path.exists() {
            anyhow::bail!("Best model checkpoint not found");
        }

        let recorder = burn::record::BinFileRecorder::<FullPrecisionSettings>::default();
        let record = recorder
            .load(model_path, device)
            .map_err(|e| anyhow::anyhow!("Failed to load best model: {:?}", e))?;

        // Create model with loaded record
        let model = AutomataflNet::<B>::new(net_config, device).load_record(record);

        Ok(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::NetConfig;
    use tempfile::TempDir;

    type TestBackend = NdArray;

    #[test]
    fn test_checkpoint_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf(), 5).unwrap();
        assert!(temp_dir.path().exists());
    }

    #[test]
    fn test_latest_iteration_empty() {
        let temp_dir = TempDir::new().unwrap();
        let manager = CheckpointManager::new(temp_dir.path().to_path_buf(), 5).unwrap();
        assert_eq!(manager.latest_iteration().unwrap(), None);
    }
}

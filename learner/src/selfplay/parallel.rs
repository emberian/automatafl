use automatafl_fastlogic::Pid;
use burn::tensor::backend::Backend;
use std::sync::Arc;

use super::worker::{SelfPlayConfig, SelfPlayWorker, TrainingExample};
use crate::network::AutomataflNet;

/// Parallel self-play coordinator
///
/// Note: Currently runs sequentially due to Burn model Sync limitations.
/// True parallelism can be added later by serializing/deserializing models
/// per thread or using a different approach.
pub struct ParallelSelfPlay;

impl ParallelSelfPlay {
    /// Generate training examples across multiple workers
    ///
    /// Creates `num_workers` workers, each playing `games_per_worker` games
    ///
    /// Note: Currently runs sequentially - true parallelism requires
    /// cloning models which Burn doesn't support directly. Each worker
    /// gets a unique seed for reproducibility.
    pub fn generate_examples<B: Backend + 'static>(
        model: Arc<AutomataflNet<B>>,
        device: B::Device,
        config: SelfPlayConfig,
        num_workers: usize,
        games_per_worker: usize,
        player_id: Pid,
    ) -> Vec<TrainingExample>
    where
        B::Device: Clone,
    {
        let mut all_examples = Vec::new();

        // Run workers sequentially (can be parallelized later with more effort)
        for worker_id in 0..num_workers {
            // Each worker gets a unique seed
            let seed = worker_id as u64 * 1000;
            let mut worker = SelfPlayWorker::new(
                config.clone(),
                model.clone(),
                device.clone(),
                seed,
            );

            let examples = worker.play_games(games_per_worker, player_id);
            all_examples.extend(examples);
        }

        all_examples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::NetConfig;

    type TestBackend = NdArray;

    #[test]
    fn test_parallel_self_play() {
        let device = NdArrayDevice::Cpu;
        let net_config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let model = Arc::new(AutomataflNet::<TestBackend>::new(&net_config, &device));

        let config = SelfPlayConfig {
            mcts_simulations: 5, // Very few for testing
            ..Default::default()
        };

        let examples = ParallelSelfPlay::generate_examples(
            model,
            device,
            config,
            2, // 2 workers
            1, // 1 game per worker
            Pid(0),
        );

        // Should have examples from 2 games
        assert!(!examples.is_empty());
    }
}

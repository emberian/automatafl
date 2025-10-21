use super::worker::TrainingExample;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Replay buffer for storing training examples
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplayBuffer {
    /// Maximum capacity
    capacity: usize,

    /// Stored examples (ring buffer)
    examples: VecDeque<TrainingExample>,
}

impl ReplayBuffer {
    /// Create a new replay buffer
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            examples: VecDeque::with_capacity(capacity),
        }
    }

    /// Add examples to the buffer
    ///
    /// If buffer is full, oldest examples are removed (FIFO)
    pub fn add_examples(&mut self, new_examples: Vec<TrainingExample>) {
        for example in new_examples {
            if self.examples.len() >= self.capacity {
                self.examples.pop_front();
            }
            self.examples.push_back(example);
        }
    }

    /// Sample a batch of examples uniformly at random
    pub fn sample(&self, batch_size: usize, rng: &mut impl Rng) -> Vec<TrainingExample> {
        if self.examples.is_empty() {
            return Vec::new();
        }

        let batch_size = batch_size.min(self.examples.len());
        self.examples
            .iter()
            .collect::<Vec<_>>()
            .choose_multiple(rng, batch_size)
            .map(|&ex| ex.clone())
            .collect()
    }

    /// Get the number of examples in the buffer
    pub fn len(&self) -> usize {
        self.examples.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.examples.is_empty()
    }

    /// Clear all examples
    pub fn clear(&mut self) {
        self.examples.clear();
    }

    /// Save buffer to file
    pub fn save_to_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        bincode::serialize_into(file, self)?;
        Ok(())
    }

    /// Load buffer from file
    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let buffer = bincode::deserialize_from(file)?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automatafl_fastlogic::BoardBits;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn create_dummy_example(value: f32) -> TrainingExample {
        TrainingExample {
            board: BoardBits::stock_testing(),
            policy: vec![0.0; 121 * 121],
            value,
        }
    }

    #[test]
    fn test_buffer_creation() {
        let buffer = ReplayBuffer::new(100);
        assert_eq!(buffer.capacity, 100);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_add_examples() {
        let mut buffer = ReplayBuffer::new(10);

        let examples = vec![
            create_dummy_example(1.0),
            create_dummy_example(-1.0),
        ];

        buffer.add_examples(examples);
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buffer = ReplayBuffer::new(5);

        // Add 10 examples (buffer capacity is 5)
        for i in 0..10 {
            buffer.add_examples(vec![create_dummy_example(i as f32)]);
        }

        // Should only keep last 5
        assert_eq!(buffer.len(), 5);

        // Oldest examples should have been discarded
        // Last 5 examples have values 5.0, 6.0, 7.0, 8.0, 9.0
        let mut rng = StdRng::seed_from_u64(42);
        let samples = buffer.sample(5, &mut rng);
        assert_eq!(samples.len(), 5);

        // All samples should have values >= 5.0
        for sample in &samples {
            assert!(sample.value >= 5.0);
        }
    }

    #[test]
    fn test_sampling() {
        let mut buffer = ReplayBuffer::new(100);

        let examples = vec![
            create_dummy_example(1.0),
            create_dummy_example(2.0),
            create_dummy_example(3.0),
        ];

        buffer.add_examples(examples);

        let mut rng = StdRng::seed_from_u64(42);
        let samples = buffer.sample(2, &mut rng);

        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn test_sample_empty_buffer() {
        let buffer = ReplayBuffer::new(100);
        let mut rng = StdRng::seed_from_u64(42);

        let samples = buffer.sample(10, &mut rng);
        assert_eq!(samples.len(), 0);
    }

    #[test]
    fn test_sample_more_than_available() {
        let mut buffer = ReplayBuffer::new(100);

        buffer.add_examples(vec![
            create_dummy_example(1.0),
            create_dummy_example(2.0),
        ]);

        let mut rng = StdRng::seed_from_u64(42);
        let samples = buffer.sample(10, &mut rng); // Request more than available

        // Should return all available examples
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut buffer = ReplayBuffer::new(100);

        buffer.add_examples(vec![
            create_dummy_example(1.0),
            create_dummy_example(2.0),
        ]);

        assert_eq!(buffer.len(), 2);

        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }
}

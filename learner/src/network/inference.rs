use burn::tensor::{Tensor, backend::Backend, activation};
use automatafl_fastlogic::BoardBits;

use super::{AutomataflNet, BoardEncoder};

/// Batch inference utilities for the neural network
pub struct BatchInference;

impl BatchInference {
    /// Run inference on a batch of board states
    ///
    /// Returns: (policy_logits, values)
    /// - policy_logits: [batch, ACTION_SPACE_SIZE]
    /// - values: [batch, 1]
    pub fn infer<B: Backend>(
        model: &AutomataflNet<B>,
        boards: &[BoardBits],
        device: &B::Device,
    ) -> (Tensor<B, 2>, Tensor<B, 2>) {
        // Encode boards to tensor
        let input = BoardEncoder::encode_batch::<B>(boards, device);

        // Forward pass
        model.forward(input)
    }

    /// Run inference on a batch of boards and apply legal move masking
    ///
    /// Returns: (policy_probs, values)
    /// - policy_probs: [batch, ACTION_SPACE_SIZE] - softmax probabilities with masking
    /// - values: [batch, 1]
    pub fn infer_with_masking<B: Backend>(
        model: &AutomataflNet<B>,
        boards: &[BoardBits],
        device: &B::Device,
    ) -> (Tensor<B, 2>, Tensor<B, 2>) {
        // Get raw logits and values
        let (policy_logits, values) = Self::infer(model, boards, device);

        // Create mask for each board
        let masks: Vec<Tensor<B, 1>> = boards
            .iter()
            .map(|board| BoardEncoder::legal_move_mask::<B>(board, device))
            .collect();

        // Stack masks into batch tensor
        let mask = Tensor::stack(masks, 0);

        // Apply mask: set illegal moves to large negative value
        let large_negative = Tensor::ones_like(&policy_logits).mul_scalar(-1e9);
        let masked_logits = policy_logits.clone().mask_where(mask.clone().equal_elem(0.0), large_negative);

        // Apply softmax to get probabilities
        let policy_probs = activation::softmax(masked_logits, 1);

        (policy_probs, values)
    }

    /// Run inference on a single board state
    ///
    /// Returns: (policy_probs, value)
    /// - policy_probs: Vec<f32> of length ACTION_SPACE_SIZE
    /// - value: f32 in [-1, 1]
    pub fn infer_single<B: Backend>(
        model: &AutomataflNet<B>,
        board: &BoardBits,
        device: &B::Device,
    ) -> (Vec<f32>, f32) {
        let (policy_probs, values) = Self::infer_with_masking(model, &[board.clone()], device);

        // Extract from batch
        let num_actions = policy_probs.dims()[1];
        let policy_vec = policy_probs
            .slice([0..1])
            .reshape([num_actions])
            .to_data()
            .to_vec::<f32>()
            .unwrap();

        let value = values
            .slice([0..1])
            .to_data()
            .to_vec::<f32>()
            .unwrap()[0];

        (policy_vec, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::{AutomataflNet, NetConfig};

    type TestBackend = NdArray;

    #[test]
    fn test_batch_inference() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let net = AutomataflNet::<TestBackend>::new(&config, &device);

        let boards = vec![
            BoardBits::stock_testing(),
            BoardBits::stock_testing_empty(),
        ];

        let (policy, values) = BatchInference::infer(&net, &boards, &device);

        assert_eq!(policy.dims()[0], 2);
        assert_eq!(values.dims()[0], 2);
    }

    #[test]
    fn test_single_inference() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let net = AutomataflNet::<TestBackend>::new(&config, &device);

        let board = BoardBits::stock_testing();
        let (policy, value) = BatchInference::infer_single(&net, &board, &device);

        assert_eq!(policy.len(), super::super::encoder::ACTION_SPACE_SIZE);
        assert!(value >= -1.0 && value <= 1.0);
    }

    #[test]
    fn test_inference_with_masking() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let net = AutomataflNet::<TestBackend>::new(&config, &device);

        let board = BoardBits::stock_testing();
        let (policy, value) = BatchInference::infer_single(&net, &board, &device);

        // Policy should sum to approximately 1.0 (softmax output)
        let policy_sum: f32 = policy.iter().sum();
        assert!((policy_sum - 1.0).abs() < 0.01, "Policy should sum to ~1.0, got {}", policy_sum);

        // All probabilities should be non-negative
        assert!(policy.iter().all(|&p| p >= 0.0), "All probabilities should be non-negative");

        // Value should be in [-1, 1]
        assert!(value >= -1.0 && value <= 1.0, "Value should be in [-1, 1], got {}", value);
    }
}

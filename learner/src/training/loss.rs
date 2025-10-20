use burn::tensor::{Tensor, backend::Backend, activation};

/// Compute policy loss (cross-entropy between MCTS policy and network policy)
///
/// Arguments:
/// - policy_logits: Raw policy output from network [batch, action_space]
/// - target_policy: MCTS visit count distribution [batch, action_space]
///
/// Returns: Scalar loss tensor
pub fn policy_loss<B: Backend>(
    policy_logits: Tensor<B, 2>,
    target_policy: Tensor<B, 2>,
) -> Tensor<B, 1> {
    // Apply log softmax to policy logits
    let log_probs = activation::log_softmax(policy_logits, 1);

    // Cross-entropy: -sum(target * log(pred))
    // Negative because we want to maximize log likelihood
    let cross_entropy = target_policy * log_probs;
    let loss = -cross_entropy.sum_dim(1).mean();

    loss
}

/// Compute value loss (MSE between game outcome and network value prediction)
///
/// Arguments:
/// - value_pred: Network value predictions [batch, 1] in range [-1, 1]
/// - value_target: Actual game outcomes [batch, 1] in {-1, 0, 1}
///
/// Returns: Scalar loss tensor
pub fn value_loss<B: Backend>(
    value_pred: Tensor<B, 2>,
    value_target: Tensor<B, 2>,
) -> Tensor<B, 1> {
    // MSE: mean((pred - target)^2)
    let diff = value_pred - value_target;
    let squared = diff.powf_scalar(2.0);
    let loss = squared.mean();

    loss
}

/// Compute combined loss (policy + value)
///
/// AlphaZero uses: L = (z - v)^2 - π^T log p + c||θ||^2
/// where:
/// - (z - v)^2 is value loss
/// - π^T log p is policy loss
/// - c||θ||^2 is L2 regularization (handled by optimizer)
///
/// We combine them with equal weight as in AlphaZero
pub fn combined_loss<B: Backend>(
    policy_logits: Tensor<B, 2>,
    value_pred: Tensor<B, 2>,
    target_policy: Tensor<B, 2>,
    value_target: Tensor<B, 2>,
) -> Tensor<B, 1> {
    let p_loss = policy_loss(policy_logits, target_policy);
    let v_loss = value_loss(value_pred, value_target);

    // Combined with equal weight
    p_loss + v_loss
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;

    type TestBackend = NdArray;

    #[test]
    fn test_policy_loss_shape() {
        let device = NdArrayDevice::Cpu;

        // Create dummy policy logits and targets
        let batch_size = 4;
        let action_space = 10;

        let logits = Tensor::<TestBackend, 2>::zeros([batch_size, action_space], &device);
        let target = Tensor::<TestBackend, 2>::zeros([batch_size, action_space], &device);

        let loss = policy_loss(logits, target);

        // Loss should be a scalar (rank 1 with single element)
        assert_eq!(loss.dims(), [1]);
    }

    #[test]
    fn test_value_loss_shape() {
        let device = NdArrayDevice::Cpu;

        let batch_size = 4;
        let pred = Tensor::<TestBackend, 2>::zeros([batch_size, 1], &device);
        let target = Tensor::<TestBackend, 2>::zeros([batch_size, 1], &device);

        let loss = value_loss(pred, target);

        assert_eq!(loss.dims(), [1]);
    }

    #[test]
    fn test_combined_loss() {
        let device = NdArrayDevice::Cpu;

        let batch_size = 4;
        let action_space = 10;

        let policy_logits = Tensor::<TestBackend, 2>::zeros([batch_size, action_space], &device);
        let value_pred = Tensor::<TestBackend, 2>::zeros([batch_size, 1], &device);
        let target_policy = Tensor::<TestBackend, 2>::zeros([batch_size, action_space], &device);
        let value_target = Tensor::<TestBackend, 2>::zeros([batch_size, 1], &device);

        let loss = combined_loss(policy_logits, value_pred, target_policy, value_target);

        assert_eq!(loss.dims(), [1]);
    }
}

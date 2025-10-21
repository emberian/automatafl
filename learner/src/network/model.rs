use burn::nn::{self, conv::{Conv2d, Conv2dConfig}, BatchNorm, BatchNormConfig, Linear, LinearConfig, PaddingConfig2d};
use burn::module::Module;
use burn::tensor::{Tensor, backend::Backend, activation};
use burn::config::Config;
use automatafl_fastlogic::{W, H};

use super::encoder::{INPUT_PLANES, ACTION_SPACE_SIZE};

/// Configuration for the AutomataflNet neural network
#[derive(Debug, Config)]
pub struct NetConfig {
    /// Number of residual blocks
    #[config(default = 10)]
    pub num_blocks: usize,

    /// Number of channels in residual blocks
    #[config(default = 128)]
    pub num_channels: usize,

    /// Number of channels in policy head
    #[config(default = 32)]
    pub policy_channels: usize,

    /// Number of channels in value head
    #[config(default = 32)]
    pub value_channels: usize,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            num_blocks: 10,
            num_channels: 128,
            policy_channels: 32,
            value_channels: 32,
        }
    }
}

/// Initial convolution block
#[derive(Module, Debug)]
struct ConvBlock<B: Backend> {
    conv: Conv2d<B>,
    bn: BatchNorm<B, 2>,
}

impl<B: Backend> ConvBlock<B> {
    fn new(in_channels: usize, out_channels: usize, device: &B::Device) -> Self {
        let conv = Conv2dConfig::new([in_channels, out_channels], [3, 3])
            .with_padding(PaddingConfig2d::Same)
            .init(device);

        let bn = BatchNormConfig::new(out_channels).init(device);

        Self { conv, bn }
    }

    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        let x = self.conv.forward(input);
        let x = self.bn.forward(x);
        activation::relu(x)
    }
}

/// Residual block with skip connection
#[derive(Module, Debug)]
struct ResidualBlock<B: Backend> {
    conv1: Conv2d<B>,
    bn1: BatchNorm<B, 2>,
    conv2: Conv2d<B>,
    bn2: BatchNorm<B, 2>,
}

impl<B: Backend> ResidualBlock<B> {
    fn new(channels: usize, device: &B::Device) -> Self {
        let conv1 = Conv2dConfig::new([channels, channels], [3, 3])
            .with_padding(PaddingConfig2d::Same)
            .init(device);

        let bn1 = BatchNormConfig::new(channels).init(device);

        let conv2 = Conv2dConfig::new([channels, channels], [3, 3])
            .with_padding(PaddingConfig2d::Same)
            .init(device);

        let bn2 = BatchNormConfig::new(channels).init(device);

        Self {
            conv1,
            bn1,
            conv2,
            bn2,
        }
    }

    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        let residual = input.clone();

        // First conv block
        let x = self.conv1.forward(input);
        let x = self.bn1.forward(x);
        let x = activation::relu(x);

        // Second conv block
        let x = self.conv2.forward(x);
        let x = self.bn2.forward(x);

        // Add skip connection
        let x = x + residual;
        activation::relu(x)
    }
}

/// Policy head - outputs move probabilities
#[derive(Module, Debug)]
struct PolicyHead<B: Backend> {
    conv: Conv2d<B>,
    bn: BatchNorm<B, 2>,
    fc: Linear<B>,
}

impl<B: Backend> PolicyHead<B> {
    fn new(in_channels: usize, policy_channels: usize, device: &B::Device) -> Self {
        let conv = Conv2dConfig::new([in_channels, policy_channels], [1, 1])
            .init(device);

        let bn = BatchNormConfig::new(policy_channels).init(device);

        let fc = LinearConfig::new(policy_channels * W * H, ACTION_SPACE_SIZE)
            .init(device);

        Self { conv, bn, fc }
    }

    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 2> {
        // Conv + BN + ReLU
        let x = self.conv.forward(input);
        let x = self.bn.forward(x);
        let x = activation::relu(x);

        // Flatten
        let [batch_size, channels, height, width] = x.dims();
        let x = x.reshape([batch_size, channels * height * width]);

        // Fully connected to action space
        self.fc.forward(x)
    }
}

/// Value head - outputs win probability
#[derive(Module, Debug)]
struct ValueHead<B: Backend> {
    conv: Conv2d<B>,
    bn: BatchNorm<B, 2>,
    fc1: Linear<B>,
    fc2: Linear<B>,
}

impl<B: Backend> ValueHead<B> {
    fn new(in_channels: usize, value_channels: usize, device: &B::Device) -> Self {
        let conv = Conv2dConfig::new([in_channels, value_channels], [1, 1])
            .init(device);

        let bn = BatchNormConfig::new(value_channels).init(device);

        let fc1 = LinearConfig::new(value_channels * W * H, 256)
            .init(device);

        let fc2 = LinearConfig::new(256, 1)
            .init(device);

        Self { conv, bn, fc1, fc2 }
    }

    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 2> {
        // Conv + BN + ReLU
        let x = self.conv.forward(input);
        let x = self.bn.forward(x);
        let x = activation::relu(x);

        // Flatten
        let [batch_size, channels, height, width] = x.dims();
        let x = x.reshape([batch_size, channels * height * width]);

        // First FC layer + ReLU
        let x = self.fc1.forward(x);
        let x = activation::relu(x);

        // Second FC layer + tanh (value in [-1, 1])
        let x = self.fc2.forward(x);
        activation::tanh(x)
    }
}

/// Neural network for Automatafl with policy and value heads
///
/// Architecture:
/// - Input: [batch, INPUT_PLANES, H, W]
/// - Conv block: INPUT_PLANES -> num_channels
/// - N residual blocks: num_channels -> num_channels
/// - Policy head: num_channels -> ACTION_SPACE_SIZE
/// - Value head: num_channels -> 1 (win probability)
#[derive(Module, Debug)]
pub struct AutomataflNet<B: Backend> {
    initial_conv: ConvBlock<B>,
    residual_blocks: Vec<ResidualBlock<B>>,
    policy_head: PolicyHead<B>,
    value_head: ValueHead<B>,
}

impl<B: Backend> AutomataflNet<B> {
    /// Create a new neural network
    pub fn new(config: &NetConfig, device: &B::Device) -> Self {
        // Initial convolution
        let initial_conv = ConvBlock::new(INPUT_PLANES, config.num_channels, device);

        // Residual tower
        let residual_blocks = (0..config.num_blocks)
            .map(|_| ResidualBlock::new(config.num_channels, device))
            .collect();

        // Policy and value heads
        let policy_head = PolicyHead::new(
            config.num_channels,
            config.policy_channels,
            device,
        );

        let value_head = ValueHead::new(
            config.num_channels,
            config.value_channels,
            device,
        );

        Self {
            initial_conv,
            residual_blocks,
            policy_head,
            value_head,
        }
    }

    /// Forward pass through the network
    ///
    /// Returns: (policy_logits, value)
    /// - policy_logits: [batch, ACTION_SPACE_SIZE] - unnormalized log probabilities
    /// - value: [batch, 1] - win probability for current player in [-1, 1]
    pub fn forward(&self, input: Tensor<B, 4>) -> (Tensor<B, 2>, Tensor<B, 2>) {
        // Initial convolution
        let mut x = self.initial_conv.forward(input);

        // Residual tower
        for block in &self.residual_blocks {
            x = block.forward(x);
        }

        // Separate heads
        let policy = self.policy_head.forward(x.clone());
        let value = self.value_head.forward(x);

        (policy, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;
    use crate::network::BoardEncoder;
    use automatafl_fastlogic::BoardBits;

    type TestBackend = NdArray;

    #[test]
    fn test_network_creation() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig::default();
        let _net = AutomataflNet::<TestBackend>::new(&config, &device);
    }

    #[test]
    fn test_network_forward_shape() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig {
            num_blocks: 2,  // Small for testing
            num_channels: 32,
            policy_channels: 16,
            value_channels: 16,
        };
        let net = AutomataflNet::<TestBackend>::new(&config, &device);

        // Create dummy input
        let board = BoardBits::stock_testing();
        let input = BoardEncoder::encode::<TestBackend>(&board, &device)
            .unsqueeze_dim(0); // Add batch dimension

        let (policy, value) = net.forward(input);

        // Check shapes
        assert_eq!(policy.dims(), [1, ACTION_SPACE_SIZE]);
        assert_eq!(value.dims(), [1, 1]);
    }

    #[test]
    fn test_network_batch_forward() {
        let device = NdArrayDevice::Cpu;
        let config = NetConfig {
            num_blocks: 1,
            num_channels: 16,
            policy_channels: 8,
            value_channels: 8,
        };
        let net = AutomataflNet::<TestBackend>::new(&config, &device);

        // Create batch of inputs
        let boards = vec![
            BoardBits::stock_testing(),
            BoardBits::stock_testing_empty(),
        ];
        let input = BoardEncoder::encode_batch::<TestBackend>(&boards, &device);

        let (policy, value) = net.forward(input);

        // Check batch shapes
        assert_eq!(policy.dims(), [2, ACTION_SPACE_SIZE]);
        assert_eq!(value.dims(), [2, 1]);
    }
}

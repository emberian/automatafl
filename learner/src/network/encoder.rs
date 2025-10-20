use automatafl_fastlogic::{BoardBits, Coord, Move, Particle, W, H};
use burn::tensor::{Tensor, backend::Backend};

/// Number of input planes for the neural network
pub const INPUT_PLANES: usize = 4;

/// Total number of possible actions (from_pos * to_pos)
pub const ACTION_SPACE_SIZE: usize = (W * H) * (W * H);

/// Convert board state to neural network input tensor
///
/// Input encoding (4 planes):
/// - Plane 0: Repulsor positions (1 where repulsor, 0 elsewhere)
/// - Plane 1: Attractor positions (1 where attractor, 0 elsewhere)
/// - Plane 2: Automaton position (1 at automaton location, 0 elsewhere)
/// - Plane 3: Occupied cells (1 where any piece, 0 for vacuum)
///
/// Shape: [4, H, W] for single board or [batch_size, 4, H, W] for batch
pub struct BoardEncoder;

impl BoardEncoder {
    /// Encode a single board state into a tensor
    pub fn encode<B: Backend>(board: &BoardBits, device: &B::Device) -> Tensor<B, 3> {
        let mut planes = vec![0.0f32; INPUT_PLANES * H * W];

        for y in 0..H {
            for x in 0..W {
                let coord = Coord {
                    x: x as u8,
                    y: y as u8,
                };
                let idx = |plane: usize, y: usize, x: usize| plane * (H * W) + y * W + x;

                let particle = board.get(coord);
                match particle {
                    Particle::Repulsor => {
                        planes[idx(0, y, x)] = 1.0;
                        planes[idx(3, y, x)] = 1.0;
                    }
                    Particle::Attractor => {
                        planes[idx(1, y, x)] = 1.0;
                        planes[idx(3, y, x)] = 1.0;
                    }
                    Particle::Automaton => {
                        planes[idx(2, y, x)] = 1.0;
                        planes[idx(3, y, x)] = 1.0;
                    }
                    Particle::Vacuum => {}
                }
            }
        }

        // Convert to tensor: [4, H, W]
        Tensor::<B, 1>::from_floats(planes.as_slice(), device)
            .reshape([INPUT_PLANES, H, W])
    }

    /// Encode a batch of board states
    pub fn encode_batch<B: Backend>(
        boards: &[BoardBits],
        device: &B::Device,
    ) -> Tensor<B, 4> {
        let batch_size = boards.len();
        let mut batch_data = vec![0.0f32; batch_size * INPUT_PLANES * H * W];

        for (b, board) in boards.iter().enumerate() {
            for y in 0..H {
                for x in 0..W {
                    let coord = Coord {
                        x: x as u8,
                        y: y as u8,
                    };
                    let idx = |b: usize, plane: usize, y: usize, x: usize| {
                        b * (INPUT_PLANES * H * W) + plane * (H * W) + y * W + x
                    };

                    let particle = board.get(coord);
                    match particle {
                        Particle::Repulsor => {
                            batch_data[idx(b, 0, y, x)] = 1.0;
                            batch_data[idx(b, 3, y, x)] = 1.0;
                        }
                        Particle::Attractor => {
                            batch_data[idx(b, 1, y, x)] = 1.0;
                            batch_data[idx(b, 3, y, x)] = 1.0;
                        }
                        Particle::Automaton => {
                            batch_data[idx(b, 2, y, x)] = 1.0;
                            batch_data[idx(b, 3, y, x)] = 1.0;
                        }
                        Particle::Vacuum => {}
                    }
                }
            }
        }

        // Convert to tensor: [batch_size, 4, H, W]
        Tensor::<B, 1>::from_floats(batch_data.as_slice(), device)
            .reshape([batch_size, INPUT_PLANES, H, W])
    }

    /// Convert a move to action index
    ///
    /// Action encoding: from_position * (W*H) + to_position
    /// where position = y * W + x
    pub fn move_to_action(m: &Move) -> usize {
        let from_idx = (m.from.y as usize) * W + (m.from.x as usize);
        let to_idx = (m.to.y as usize) * W + (m.to.x as usize);
        from_idx * (W * H) + to_idx
    }

    /// Convert action index to move coordinates (for a given player)
    pub fn action_to_move(action: usize, who: automatafl_fastlogic::Pid) -> Move {
        let from_idx = action / (W * H);
        let to_idx = action % (W * H);

        let from_y = from_idx / W;
        let from_x = from_idx % W;
        let to_y = to_idx / W;
        let to_x = to_idx % W;

        Move {
            who,
            from: Coord {
                x: from_x as u8,
                y: from_y as u8,
            },
            to: Coord {
                x: to_x as u8,
                y: to_y as u8,
            },
        }
    }

    /// Generate a mask tensor for legal moves
    ///
    /// Returns a tensor of shape [ACTION_SPACE_SIZE] where 1.0 indicates
    /// a potentially legal move and 0.0 indicates an illegal move.
    /// This mask can be applied to policy logits before softmax.
    ///
    /// Basic filtering:
    /// - from != to
    /// - from and to share an axis (x or y coordinate)
    /// - from has a piece, to is inbounds
    pub fn legal_move_mask<B: Backend>(
        board: &BoardBits,
        device: &B::Device,
    ) -> Tensor<B, 1> {
        let mut mask = vec![0.0f32; ACTION_SPACE_SIZE];

        for from_y in 0..H {
            for from_x in 0..W {
                let from_coord = Coord {
                    x: from_x as u8,
                    y: from_y as u8,
                };

                // Skip if from position is vacuum
                if board.is_vacuum(from_coord) {
                    continue;
                }

                // Skip if from position is automaton
                if board.is_automaton(from_coord) {
                    continue;
                }

                for to_y in 0..H {
                    for to_x in 0..W {
                        // from != to
                        if from_x == to_x && from_y == to_y {
                            continue;
                        }

                        // Must be axis-aligned (same x or same y)
                        if from_x != to_x && from_y != to_y {
                            continue;
                        }

                        let from_idx = from_y * W + from_x;
                        let to_idx = to_y * W + to_x;
                        let action_idx = from_idx * (W * H) + to_idx;

                        mask[action_idx] = 1.0;
                    }
                }
            }
        }

        Tensor::<B, 1>::from_floats(mask.as_slice(), device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use burn::backend::ndarray::NdArrayDevice;
    use burn_ndarray::NdArray;

    type TestBackend = NdArray;

    #[test]
    fn test_encode_empty_board() {
        let board = BoardBits::stock_testing_empty();
        let device = NdArrayDevice::Cpu;
        let tensor = BoardEncoder::encode::<TestBackend>(&board, &device);

        let shape = tensor.shape();
        assert_eq!(shape.dims, [INPUT_PLANES, H, W]);
    }

    #[test]
    fn test_move_action_roundtrip() {
        use automatafl_fastlogic::Pid;

        let m = Move {
            who: Pid(0),
            from: Coord { x: 2, y: 3 },
            to: Coord { x: 5, y: 3 },
        };

        let action = BoardEncoder::move_to_action(&m);
        let m2 = BoardEncoder::action_to_move(action, Pid(0));

        assert_eq!(m.from, m2.from);
        assert_eq!(m.to, m2.to);
    }

    #[test]
    fn test_legal_move_mask_excludes_vacuum() {
        let board = BoardBits::stock_testing();
        let device = NdArrayDevice::Cpu;
        let mask = BoardEncoder::legal_move_mask::<TestBackend>(&board, &device);

        // Vacuum at (1, 1) should not be a valid source
        let from_coord = Coord { x: 1, y: 1 };
        let to_coord = Coord { x: 1, y: 2 };

        // Check if board confirms it's vacuum
        assert!(board.is_vacuum(from_coord));

        let from_idx = (from_coord.y as usize) * W + (from_coord.x as usize);
        let to_idx = (to_coord.y as usize) * W + (to_coord.x as usize);
        let action = from_idx * (W * H) + to_idx;

        let mask_data = mask.to_data().to_vec::<f32>().unwrap();
        assert_eq!(mask_data[action], 0.0);
    }

    #[test]
    fn test_legal_move_mask_requires_axis_aligned() {
        let board = BoardBits::stock_testing();
        let device = NdArrayDevice::Cpu;
        let mask = BoardEncoder::legal_move_mask::<TestBackend>(&board, &device);

        // Diagonal move should be masked out
        let from_coord = Coord { x: 0, y: 2 }; // Has a piece
        let to_coord = Coord { x: 1, y: 3 }; // Diagonal

        let from_idx = (from_coord.y as usize) * W + (from_coord.x as usize);
        let to_idx = (to_coord.y as usize) * W + (to_coord.x as usize);
        let action = from_idx * (W * H) + to_idx;

        let mask_data = mask.to_data().to_vec::<f32>().unwrap();
        assert_eq!(mask_data[action], 0.0);
    }
}

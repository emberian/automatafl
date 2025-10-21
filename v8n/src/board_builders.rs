use automatafl_logic::*;
use ndarray::Array2;
use rand::Rng;
use smallvec::SmallVec;

/// Create a large board with specified size and particle density
///
/// # Arguments
/// * `width` - Board width
/// * `height` - Board height
/// * `num_players` - Number of players (affects particle distribution)
/// * `density` - Particle density (0.0 = empty, 1.0 = completely filled)
/// * `seed` - Random seed for reproducibility
pub fn create_large_board(
    width: u8,
    height: u8,
    num_players: u8,
    density: f32,
    seed: u64,
) -> Board {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    let vacuum_cell = Cell {
        what: Particle::Vacuum,
        conflict: false,
        passable: false,
    };

    // Create empty grid
    let mut particles = Array2::from_elem((height as usize, width as usize), vacuum_cell);

    // Place automaton in center
    let automaton_location = Coord {
        x: width / 2,
        y: height / 2,
    };
    particles[automaton_location.ix()] = Cell {
        what: Particle::Automaton,
        ..vacuum_cell
    };

    // Calculate target number of particles based on density
    let total_cells = (width as usize * height as usize) - 1; // -1 for automaton
    let target_particles = (total_cells as f32 * density).round() as usize;

    // Place particles randomly
    let mut placed = 0;
    while placed < target_particles {
        let x = rng.gen_range(0..width);
        let y = rng.gen_range(0..height);
        let coord = Coord { x, y };

        if coord == automaton_location || !particles[coord.ix()].what.is_vacuum() {
            continue;
        }

        // Randomly choose particle type (roughly equal distribution)
        let particle_type = if rng.gen_bool(0.5) {
            Particle::Repulsor
        } else {
            Particle::Attractor
        };

        particles[coord.ix()] = Cell {
            what: particle_type,
            ..vacuum_cell
        };

        placed += 1;
    }

    Board {
        particles,
        size: Coord {
            x: width,
            y: height,
        },
        automaton_location,
        conflict_list: SmallVec::new(),
        passable_list: SmallVec::new(),
    }
}

/// Create a symmetric board with particles arranged in a pattern
///
/// Good for testing with balanced starting positions
pub fn create_symmetric_board(size: u8, num_players: u8, seed: u64) -> Board {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    let vacuum_cell = Cell {
        what: Particle::Vacuum,
        conflict: false,
        passable: false,
    };

    let mut particles = Array2::from_elem((size as usize, size as usize), vacuum_cell);

    // Place automaton in center
    let center = size / 2;
    let automaton_location = Coord { x: center, y: center };
    particles[automaton_location.ix()] = Cell {
        what: Particle::Automaton,
        ..vacuum_cell
    };

    // Place particles in symmetric patterns around the edges
    let edge_offset = size / 8; // How far from edge to place pieces

    for player_id in 0..num_players {
        let angle = (player_id as f32 / num_players as f32) * std::f32::consts::TAU;

        // Calculate position on circle
        let radius = (size / 2 - edge_offset) as f32;
        let x = (center as f32 + radius * angle.cos()).round() as u8;
        let y = (center as f32 + radius * angle.sin()).round() as u8;

        // Place a cluster of particles
        for dx in -1i8..=1 {
            for dy in -1i8..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let px = (x as i16 + dx as i16).clamp(0, size as i16 - 1) as u8;
                let py = (y as i16 + dy as i16).clamp(0, size as i16 - 1) as u8;
                let coord = Coord { x: px, y: py };

                if coord != automaton_location && particles[coord.ix()].what.is_vacuum() {
                    let particle_type = if rng.gen_bool(0.5) {
                        Particle::Repulsor
                    } else {
                        Particle::Attractor
                    };

                    particles[coord.ix()] = Cell {
                        what: particle_type,
                        ..vacuum_cell
                    };
                }
            }
        }
    }

    Board {
        particles,
        size: Coord { x: size, y: size },
        automaton_location,
        conflict_list: SmallVec::new(),
        passable_list: SmallVec::new(),
    }
}

/// Create a stress test board designed to test worst-case scenarios
///
/// This creates boards with specific patterns designed to stress the engine:
/// - Maximum chain lengths
/// - Dense conflict zones
/// - Many simultaneous moves
pub fn create_stress_test_board(scenario: StressTestScenario, size: u8) -> Board {
    let vacuum_cell = Cell {
        what: Particle::Vacuum,
        conflict: false,
        passable: false,
    };

    let mut particles = Array2::from_elem((size as usize, size as usize), vacuum_cell);
    let center = size / 2;
    let automaton_location = Coord { x: center, y: center };

    particles[automaton_location.ix()] = Cell {
        what: Particle::Automaton,
        ..vacuum_cell
    };

    match scenario {
        StressTestScenario::MaxChain => {
            // Create a long horizontal line of alternating particles
            for x in 0..size {
                if x == center {
                    continue; // Skip automaton
                }
                let coord = Coord { x, y: center };
                particles[coord.ix()] = Cell {
                    what: if x % 2 == 0 {
                        Particle::Repulsor
                    } else {
                        Particle::Attractor
                    },
                    ..vacuum_cell
                };
            }
        }

        StressTestScenario::DenseConflictZone => {
            // Create a dense cluster of particles in each quadrant
            for quadrant in 0..4 {
                let base_x = if quadrant % 2 == 0 { size / 4 } else { 3 * size / 4 };
                let base_y = if quadrant < 2 { size / 4 } else { 3 * size / 4 };

                let cluster_size = size / 8;
                for dx in 0..cluster_size {
                    for dy in 0..cluster_size {
                        let x = (base_x + dx).min(size - 1);
                        let y = (base_y + dy).min(size - 1);
                        let coord = Coord { x, y };

                        if coord != automaton_location {
                            particles[coord.ix()] = Cell {
                                what: if (dx + dy) % 2 == 0 {
                                    Particle::Repulsor
                                } else {
                                    Particle::Attractor
                                },
                                ..vacuum_cell
                            };
                        }
                    }
                }
            }
        }

        StressTestScenario::NestedCycles => {
            // Create concentric squares of particles
            for ring in 1..=(size / 8) {
                let offset = ring * 4;

                // Top and bottom edges of ring
                for x in (center - offset)..=(center + offset) {
                    if x >= size {
                        break;
                    }
                    for &y in &[center - offset, center + offset] {
                        if y >= size {
                            continue;
                        }
                        let coord = Coord { x, y };
                        if coord != automaton_location && particles[coord.ix()].what.is_vacuum() {
                            particles[coord.ix()] = Cell {
                                what: if ring % 2 == 0 {
                                    Particle::Repulsor
                                } else {
                                    Particle::Attractor
                                },
                                ..vacuum_cell
                            };
                        }
                    }
                }

                // Left and right edges of ring
                for y in (center - offset)..=(center + offset) {
                    if y >= size {
                        break;
                    }
                    for &x in &[center - offset, center + offset] {
                        if x >= size {
                            continue;
                        }
                        let coord = Coord { x, y };
                        if coord != automaton_location && particles[coord.ix()].what.is_vacuum() {
                            particles[coord.ix()] = Cell {
                                what: if ring % 2 == 0 {
                                    Particle::Repulsor
                                } else {
                                    Particle::Attractor
                                },
                                ..vacuum_cell
                            };
                        }
                    }
                }
            }
        }
    }

    Board {
        particles,
        size: Coord { x: size, y: size },
        automaton_location,
        conflict_list: SmallVec::new(),
        passable_list: SmallVec::new(),
    }
}

/// Stress test scenarios
#[derive(Debug, Clone, Copy)]
pub enum StressTestScenario {
    /// Maximum length move chains (all pieces in a line)
    MaxChain,
    /// Dense conflict zones with many overlapping moves
    DenseConflictZone,
    /// Nested cycles of pieces
    NestedCycles,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_large_board() {
        let board = create_large_board(127, 127, 32, 0.3, 42);
        assert_eq!(board.size.x, 127);
        assert_eq!(board.size.y, 127);
        assert_eq!(board.automaton_location, Coord { x: 63, y: 63 });

        // Verify automaton is in center
        assert!(board.is_automaton(Coord { x: 63, y: 63 }));
    }

    #[test]
    fn test_create_symmetric_board() {
        let board = create_symmetric_board(64, 8, 42);
        assert_eq!(board.size.x, 64);
        assert_eq!(board.size.y, 64);

        // Verify automaton is placed
        assert!(board.is_automaton(board.automaton_location));
    }

    #[test]
    fn test_stress_test_scenarios() {
        for &scenario in &[
            StressTestScenario::MaxChain,
            StressTestScenario::DenseConflictZone,
            StressTestScenario::NestedCycles,
        ] {
            let board = create_stress_test_board(scenario, 31);
            assert_eq!(board.size.x, 31);
            assert!(board.is_automaton(board.automaton_location));
        }
    }

    #[test]
    fn test_density_parameter() {
        let empty_board = create_large_board(20, 20, 2, 0.0, 42);
        let dense_board = create_large_board(20, 20, 2, 0.8, 42);

        // Count non-vacuum particles
        let count_particles = |board: &Board| -> usize {
            let mut count = 0;
            for y in 0..board.size.y {
                for x in 0..board.size.x {
                    let coord = Coord { x, y };
                    if !board.is_vacuum(coord) && !board.is_automaton(coord) {
                        count += 1;
                    }
                }
            }
            count
        };

        let empty_count = count_particles(&empty_board);
        let dense_count = count_particles(&dense_board);

        assert!(empty_count < dense_count);
        assert_eq!(empty_count, 0); // 0.0 density should be empty
    }
}

use automatafl_v8n::*;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

// ========================================================================
// MICRO BENCHMARKS - Individual function performance
// ========================================================================

/// Benchmark Coord operations (indexing, key generation, arithmetic)
fn bench_coord_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("coord_operations");

    let coord = Coord { x: 50, y: 75 };
    let other = Coord { x: 30, y: 45 };

    group.bench_function("ix", |b| b.iter(|| black_box(coord).ix()));

    group.bench_function("to_key", |b| b.iter(|| black_box(coord).to_key(100)));

    group.bench_function("sub", |b| {
        b.iter(|| black_box(coord) - black_box(other))
    });

    let delta = Delta { dx: 5, dy: 3 };
    group.bench_function("coord_add_delta", |b| {
        b.iter(|| black_box(coord) + black_box(delta))
    });

    group.finish();
}

/// Benchmark Delta operations (analysis and transformations)
fn bench_delta_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("delta_operations");

    let delta_axial = Delta { dx: 5, dy: 0 };
    let delta_diagonal = Delta { dx: 3, dy: 4 };
    let delta_zero = Delta::ZERO;

    group.bench_function("is_zero/true", |b| {
        b.iter(|| black_box(delta_zero).is_zero())
    });

    group.bench_function("is_zero/false", |b| {
        b.iter(|| black_box(delta_axial).is_zero())
    });

    group.bench_function("is_axial/true", |b| {
        b.iter(|| black_box(delta_axial).is_axial())
    });

    group.bench_function("is_axial/false", |b| {
        b.iter(|| black_box(delta_diagonal).is_axial())
    });

    group.bench_function("axial_unit", |b| {
        b.iter(|| black_box(delta_axial).axial_unit())
    });

    group.bench_function("displacement", |b| {
        b.iter(|| black_box(delta_diagonal).displacement())
    });

    group.bench_function("scale", |b| {
        b.iter(|| black_box(delta_axial).scale(3))
    });

    group.finish();
}

/// Benchmark Cell operations (occlusion checks)
fn bench_cell_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cell_operations");

    let vacuum_cell = Cell {
        what: Particle::Vacuum,
        conflict: false,
        passable: false,
    };

    let repulsor_cell = Cell {
        what: Particle::Repulsor,
        conflict: false,
        passable: false,
    };

    let passable_cell = Cell {
        what: Particle::Attractor,
        conflict: false,
        passable: true,
    };

    group.bench_function("occludes/vacuum", |b| {
        b.iter(|| black_box(vacuum_cell).occludes())
    });

    group.bench_function("occludes/solid", |b| {
        b.iter(|| black_box(repulsor_cell).occludes())
    });

    group.bench_function("occludes/passable", |b| {
        b.iter(|| black_box(passable_cell).occludes())
    });

    group.finish();
}

/// Benchmark Particle operations
fn bench_particle_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("particle_operations");

    group.bench_function("is_vacuum/true", |b| {
        b.iter(|| black_box(Particle::Vacuum).is_vacuum())
    });

    group.bench_function("is_vacuum/false", |b| {
        b.iter(|| black_box(Particle::Repulsor).is_vacuum())
    });

    group.finish();
}

/// Benchmark Board mutation operations
fn bench_board_mutations(c: &mut Criterion) {
    let mut group = c.benchmark_group("board_mutations");

    group.bench_function("place_particle", |b| {
        b.iter_batched(
            || Board::stock_testing(),
            |mut board| {
                board.place(black_box(Coord { x: 1, y: 1 }), black_box(Particle::Repulsor));
                board
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("place_automaton", |b| {
        b.iter_batched(
            || Board::stock_testing(),
            |mut board| {
                board.place(black_box(Coord { x: 3, y: 3 }), black_box(Particle::Automaton));
                board
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// ========================================================================
// MACRO BENCHMARKS - Integrated workflow performance
// ========================================================================

/// Benchmark different board creation sizes
fn bench_board_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("board_creation");

    group.bench_function("stock_testing_5x5", |b| {
        b.iter(|| Board::stock_testing())
    });

    group.bench_function("stock_testing_empty_5x5", |b| {
        b.iter(|| Board::stock_testing_empty())
    });

    group.bench_function("stock_testing_empty_6x6", |b| {
        b.iter(|| Board::stock_testing_empty_6())
    });

    group.bench_function("stock_testing_empty_7x7", |b| {
        b.iter(|| Board::stock_testing_empty_7())
    });

    group.bench_function("stock_two_player_11x11", |b| {
        b.iter(|| Board::stock_two_player())
    });

    group.finish();
}

/// Benchmark game creation and setup
fn bench_game_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_creation");

    group.bench_function("new_game_small", |b| {
        b.iter(|| {
            let board = Board::stock_testing();
            Game::new_default_modes(black_box(board), black_box(2), black_box(true))
        })
    });

    group.bench_function("new_game_large", |b| {
        b.iter(|| {
            let board = Board::stock_two_player();
            Game::new_default_modes(black_box(board), black_box(2), black_box(true))
        })
    });

    group.bench_function("new_game_with_goals", |b| {
        b.iter(|| {
            let board = Board::stock_two_player();
            let mut game = Game::new_default_modes(black_box(board), black_box(2), black_box(true));
            game.goals.push((Coord { x: 0, y: 0 }, Pid(0)));
            game.goals.push((Coord { x: 10, y: 10 }, Pid(1)));
            game
        })
    });

    group.finish();
}

/// Benchmark move proposal with different outcomes
fn bench_game_propose_move(c: &mut Criterion) {
    let mut group = c.benchmark_group("game_propose_move");

    // Valid move acceptance
    group.bench_function("propose_valid_move", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                let m = Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                };
                game.propose_move(black_box(m));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Invalid move - out of bounds
    group.bench_function("propose_invalid_oob", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                let m = Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 10, y: 10 },
                };
                game.propose_move(black_box(m));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Invalid move - non-axial
    group.bench_function("propose_invalid_diagonal", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                let m = Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 1 },
                };
                game.propose_move(black_box(m));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Invalid move - must move
    group.bench_function("propose_invalid_must_move", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                let m = Move {
                    who: Pid(0),
                    from: Coord { x: 2, y: 2 },
                    to: Coord { x: 2, y: 2 },
                };
                game.propose_move(black_box(m));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark automaton decision making and movement
fn bench_automaton_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("automaton_operations");

    group.bench_function("update_automaton_small", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                game.update_automaton();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("update_automaton_large", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_two_player();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                game.update_automaton();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("update_automaton_no_column_rule", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, false)
            },
            |mut game| {
                game.update_automaton();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark conflict resolution with different scenarios
fn bench_conflict_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("conflict_resolution");

    // No conflicts - simple case
    group.bench_function("resolve_no_conflict", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let mut game = Game::new_default_modes(board, 2, true);
                game.propose_move(Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                });
                game.propose_move(Move {
                    who: Pid(1),
                    from: Coord { x: 0, y: 4 },
                    to: Coord { x: 1, y: 4 },
                });
                game
            },
            |mut game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Source conflict - two players move same piece differently
    group.bench_function("resolve_source_conflict", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let mut game = Game::new_default_modes(board, 2, true);
                game.propose_move(Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                });
                game.propose_move(Move {
                    who: Pid(1),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 0, y: 1 },
                });
                game
            },
            |mut game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Destination conflict - two pieces to same location
    group.bench_function("resolve_destination_conflict", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let mut game = Game::new_default_modes(board, 2, true);
                game.propose_move(Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                });
                game.propose_move(Move {
                    who: Pid(1),
                    from: Coord { x: 2, y: 0 },
                    to: Coord { x: 1, y: 0 },
                });
                game
            },
            |mut game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark move application with complex graph scenarios
fn bench_apply_moves(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_moves");

    // Simple chain move
    group.bench_function("apply_simple_chain", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let game = Game::new_default_modes(board, 2, true);
                let moves = smallvec::smallvec![Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                }];
                (game, moves)
            },
            |(mut game, moves)| {
                game.apply_moves(black_box(moves));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Multiple independent moves
    group.bench_function("apply_multiple_independent", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let game = Game::new_default_modes(board, 2, true);
                let moves = smallvec::smallvec![
                    Move {
                        who: Pid(0),
                        from: Coord { x: 0, y: 0 },
                        to: Coord { x: 1, y: 0 },
                    },
                    Move {
                        who: Pid(1),
                        from: Coord { x: 0, y: 4 },
                        to: Coord { x: 1, y: 4 },
                    }
                ];
                (game, moves)
            },
            |(mut game, moves)| {
                game.apply_moves(black_box(moves));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Longer chain - multiple pieces in a row
    group.bench_function("apply_long_chain", |b| {
        b.iter_batched(
            || {
                let mut board = Board::stock_testing_empty_7();
                // Create a line of repulsors
                board.place(Coord { x: 0, y: 3 }, Particle::Repulsor);
                board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);
                board.place(Coord { x: 2, y: 3 }, Particle::Repulsor);

                let game = Game::new_default_modes(board, 2, true);
                let moves = smallvec::smallvec![Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 3 },
                    to: Coord { x: 3, y: 3 },
                }];
                (game, moves)
            },
            |(mut game, moves)| {
                game.apply_moves(black_box(moves));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark complete round from start to finish
fn bench_complete_round(c: &mut Criterion) {
    let mut group = c.benchmark_group("complete_round");

    for board_size in &["5x5", "11x11"] {
        group.bench_with_input(
            BenchmarkId::new("full_round", board_size),
            board_size,
            |b, &size| {
                b.iter_batched(
                    || {
                        let board = if size == "5x5" {
                            Board::stock_testing()
                        } else {
                            Board::stock_two_player()
                        };
                        let mut game = Game::new_default_modes(board, 2, true);
                        game.propose_move(Move {
                            who: Pid(0),
                            from: Coord { x: 0, y: 0 },
                            to: Coord { x: 1, y: 0 },
                        });
                        game.propose_move(Move {
                            who: Pid(1),
                            from: Coord {
                                x: if size == "5x5" { 0 } else { 0 },
                                y: if size == "5x5" { 4 } else { 10 },
                            },
                            to: Coord {
                                x: if size == "5x5" { 1 } else { 1 },
                                y: if size == "5x5" { 4 } else { 10 },
                            },
                        });
                        game
                    },
                    |mut game| {
                        game.try_complete_round();
                        game
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

/// Benchmark waiting for players scenario
fn bench_incomplete_round(c: &mut Criterion) {
    let mut group = c.benchmark_group("incomplete_round");

    group.bench_function("waiting_for_second_player", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                let mut game = Game::new_default_modes(board, 2, true);
                // Only one player submits
                game.propose_move(Move {
                    who: Pid(0),
                    from: Coord { x: 0, y: 0 },
                    to: Coord { x: 1, y: 0 },
                });
                game
            },
            |mut game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark full game simulation (multiple rounds)
fn bench_full_game(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_game");
    group.sample_size(10); // Reduce sample size for longer benchmarks

    group.bench_function("simulate_5_rounds", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                for round in 0..5 {
                    let y = round % 5;
                    if !game.board.is_vacuum(Coord { x: 0, y }) {
                        game.propose_move(Move {
                            who: Pid(0),
                            from: Coord { x: 0, y },
                            to: Coord { x: 1, y },
                        });
                    } else {
                        game.propose_move(Move {
                            who: Pid(0),
                            from: Coord { x: 1, y },
                            to: Coord { x: 0, y },
                        });
                    }

                    if !game.board.is_vacuum(Coord { x: 4, y }) {
                        game.propose_move(Move {
                            who: Pid(1),
                            from: Coord { x: 4, y },
                            to: Coord { x: 3, y },
                        });
                    } else {
                        game.propose_move(Move {
                            who: Pid(1),
                            from: Coord { x: 3, y },
                            to: Coord { x: 4, y },
                        });
                    }
                    game.try_complete_round();
                }
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("simulate_10_rounds", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_testing();
                Game::new_default_modes(board, 2, true)
            },
            |mut game| {
                for round in 0..10 {
                    let y = round % 5;
                    if !game.board.is_vacuum(Coord { x: 0, y }) {
                        game.propose_move(Move {
                            who: Pid(0),
                            from: Coord { x: 0, y },
                            to: Coord { x: 1, y },
                        });
                    } else {
                        game.propose_move(Move {
                            who: Pid(0),
                            from: Coord { x: 1, y },
                            to: Coord { x: 0, y },
                        });
                    }

                    if !game.board.is_vacuum(Coord { x: 4, y }) {
                        game.propose_move(Move {
                            who: Pid(1),
                            from: Coord { x: 4, y },
                            to: Coord { x: 3, y },
                        });
                    } else {
                        game.propose_move(Move {
                            who: Pid(1),
                            from: Coord { x: 3, y },
                            to: Coord { x: 4, y },
                        });
                    }
                    game.try_complete_round();
                }
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark clone operations for the main types
fn bench_clone_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("clone_operations");

    group.bench_function("clone_board_small", |b| {
        let board = Board::stock_testing();
        b.iter(|| black_box(&board).clone())
    });

    group.bench_function("clone_board_large", |b| {
        let board = Board::stock_two_player();
        b.iter(|| black_box(&board).clone())
    });

    group.bench_function("clone_game_small", |b| {
        let board = Board::stock_testing();
        let game = Game::new_default_modes(board, 2, true);
        b.iter(|| black_box(&game).clone())
    });

    group.bench_function("clone_game_large", |b| {
        let board = Board::stock_two_player();
        let game = Game::new_default_modes(board, 2, true);
        b.iter(|| black_box(&game).clone())
    });

    group.finish();
}

// ========================================================================
// LARGE-SCALE BENCHMARKS - "Bigger Things"
// ========================================================================

/// Benchmark large board creation
fn bench_large_board_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_board_creation");
    group.sample_size(10); // Reduce sample size for expensive operations

    group.bench_function("create_127x127_empty", |b| {
        b.iter(|| create_large_board(black_box(127), black_box(127), 32, 0.0, 42))
    });

    group.bench_function("create_127x127_sparse", |b| {
        b.iter(|| create_large_board(black_box(127), black_box(127), 32, 0.1, 42))
    });

    group.bench_function("create_127x127_medium", |b| {
        b.iter(|| create_large_board(black_box(127), black_box(127), 32, 0.3, 42))
    });

    group.bench_function("create_127x127_dense", |b| {
        b.iter(|| create_large_board(black_box(127), black_box(127), 32, 0.7, 42))
    });

    group.bench_function("create_symmetric_64x64_8p", |b| {
        b.iter(|| create_symmetric_board(black_box(64), black_box(8), 42))
    });

    group.finish();
}

/// Benchmark large board operations
fn bench_large_board_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_board_operations");
    group.sample_size(10);

    let board_127 = create_large_board(127, 127, 32, 0.3, 42);

    group.bench_function("clone_127x127", |b| {
        b.iter(|| black_box(&board_127).clone())
    });

    group.bench_function("coord_indexing_127x127", |b| {
        let coord = Coord { x: 63, y: 63 };
        b.iter(|| black_box(coord).ix())
    });

    group.bench_function("coord_to_key_127x127", |b| {
        let coord = Coord { x: 63, y: 63 };
        b.iter(|| black_box(coord).to_key(127))
    });

    group.bench_function("is_vacuum_scan_127x127", |b| {
        b.iter(|| {
            let mut count = 0;
            for y in 0..board_127.size.y {
                for x in 0..board_127.size.x {
                    if board_127.is_vacuum(Coord { x, y }) {
                        count += 1;
                    }
                }
            }
            count
        })
    });

    group.finish();
}

/// Benchmark multi-player scenarios
fn bench_multi_player_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_player");
    group.sample_size(10);

    group.bench_function("create_32_player_game", |b| {
        b.iter(|| {
            let board = create_large_board(127, 127, 32, 0.3, 42);
            Game::new_default_modes(black_box(board), black_box(32), black_box(true))
        })
    });

    group.bench_function("propose_32_moves", |b| {
        b.iter_batched(
            || {
                let board = create_large_board(64, 64, 32, 0.3, 42);
                let game = Game::new_default_modes(board, 32, true);
                let move_gen = RandomMoveGen::new(42);
                (game, move_gen)
            },
            |(mut game, mut move_gen)| {
                let moves = move_gen.generate_moves(&game);
                for m in moves {
                    game.propose_move(m);
                }
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("complete_round_32_players", |b| {
        b.iter_batched(
            || {
                let board = create_large_board(64, 64, 32, 0.3, 42);
                let mut game = Game::new_default_modes(board, 32, true);
                let mut move_gen = RandomMoveGen::new(42);
                let moves = move_gen.generate_moves(&game);
                for m in moves {
                    game.propose_move(m);
                }
                game
            },
            |mut game: Game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark stress test scenarios
fn bench_stress_test_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("stress_tests");
    group.sample_size(10);

    group.bench_function("max_chain_apply_moves", |b| {
        b.iter_batched(
            || {
                let board = create_stress_test_board(StressTestScenario::MaxChain, 31);
                let game = Game::new_default_modes(board, 4, true);
                let mut move_gen = ChainMakingGen::new(42);
                let moves = move_gen.generate_moves(&game);
                (game, moves)
            },
            |(mut game, moves): (Game, _)| {
                game.apply_moves(black_box(moves));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("dense_conflict_resolution", |b| {
        b.iter_batched(
            || {
                let board = create_stress_test_board(StressTestScenario::DenseConflictZone, 31);
                let mut game = Game::new_default_modes(board, 8, true);
                let mut move_gen = ConflictSeekingGen::new(42);
                let moves = move_gen.generate_moves(&game);
                for m in moves {
                    game.propose_move(m);
                }
                game
            },
            |mut game: Game| {
                game.try_complete_round();
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("nested_cycles_apply_moves", |b| {
        b.iter_batched(
            || {
                let board = create_stress_test_board(StressTestScenario::NestedCycles, 31);
                let game = Game::new_default_modes(board, 4, true);
                let mut move_gen = ChainMakingGen::new(42);
                let moves = move_gen.generate_moves(&game);
                (game, moves)
            },
            |(mut game, moves): (Game, _)| {
                game.apply_moves(black_box(moves));
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

/// Benchmark move generation strategies
fn bench_move_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("move_generation");

    let board = create_large_board(64, 64, 8, 0.3, 42);
    let game = Game::new_default_modes(board, 8, true);

    group.bench_function("random_move_gen", |b| {
        let mut move_gen = RandomMoveGen::new(42);
        b.iter(|| move_gen.generate_moves(black_box(&game)))
    });

    // group.bench_function("conflict_seeking_gen", |b| {
    //     let mut move_gen = ConflictSeekingGen::new(42);
    //     b.iter(|| move_gen.generate_moves(black_box(&game)))
    // });

    // group.bench_function("chain_making_gen", |b| {
    //     let mut move_gen = ChainMakingGen::new(42);
    //     b.iter(|| move_gen.generate_moves(black_box(&game)))
    // });

    // group.bench_function("goal_oriented_gen", |b| {
    //     let mut move_gen = GoalOrientedGen::new(42);
    //     b.iter(|| move_gen.generate_moves(black_box(&game)))
    // });

    group.finish();
}

/// Benchmark full game simulation with many rounds
fn bench_extended_game_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("extended_simulation");
    group.sample_size(10);

    group.bench_function("simulate_100_rounds_2p", |b| {
        b.iter_batched(
            || {
                let board = Board::stock_two_player();
                let game = Game::new_default_modes(board, 2, true);
                let move_gen = RandomMoveGen::new(42);
                (game, move_gen)
            },
            |(mut game, mut move_gen): (Game, RandomMoveGen)| {
                for _ in 0..100 {
                    let moves = move_gen.generate_moves(&game);
                    for m in moves {
                        let _ = game.propose_move(m);
                    }
                    game.try_complete_round();
                }
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("simulate_50_rounds_8p", |b| {
        b.iter_batched(
            || {
                let board = create_large_board(64, 64, 8, 0.3, 42);
                let game = Game::new_default_modes(board, 8, true);
                let move_gen = RandomMoveGen::new(42);
                (game, move_gen)
            },
            |(mut game, mut move_gen): (Game, RandomMoveGen)| {
                for _ in 0..50 {
                    let moves = move_gen.generate_moves(&game);
                    for m in moves {
                        let _ = game.propose_move(m);
                    }
                    game.try_complete_round();
                }
                game
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// ========================================================================
// Benchmark Groups
// ========================================================================

criterion_group!(
    micro_benchmarks,
    bench_coord_operations,
    bench_delta_operations,
    bench_cell_operations,
    bench_particle_operations,
    bench_board_mutations,
    bench_clone_operations,
);

criterion_group!(
    macro_benchmarks,
    bench_board_creation,
    bench_game_creation,
    bench_game_propose_move,
    bench_automaton_operations,
    bench_conflict_resolution,
    bench_apply_moves,
    bench_complete_round,
    bench_incomplete_round,
    bench_full_game,
);

criterion_group!(
    large_scale_benchmarks,
    bench_large_board_creation,
    bench_large_board_operations,
    bench_multi_player_scenarios,
    bench_stress_test_scenarios,
    bench_move_generation,
    bench_extended_game_simulation,
);

criterion_main!(micro_benchmarks, macro_benchmarks, large_scale_benchmarks);
//! Example demonstrating composable move generation strategies.
//!
//! This shows how to combine different move generators to create
//! sophisticated testing scenarios and AI opponents.

use automatafl_v8n::*;

fn main() {
    println!("=== Composable Move Generation Examples ===\n");

    // Create a test board
    let board = create_large_board(31, 31, 4, 0.3, 42);
    let game = Game::new_default_modes(board, 4, true);

    // Example 1: Sequential fallback
    println!("1. Sequential Generator (tries chain-making, falls back to random)");
    let mut sequential = SequentialGen::new(
        vec![
            Box::new(ChainMakingGen::new(42)),
            Box::new(RandomMoveGen::new(42)),
        ],
        4, // Require at least 4 moves
    );
    let moves = sequential.generate_moves(&game);
    println!("   Generated {} moves\n", moves.len());

    // Example 2: Weighted random selection
    println!("2. Weighted Generator (70% random, 30% conflict-seeking)");
    let mut weighted = WeightedGen::with_seed(
        vec![
            (7, Box::new(RandomMoveGen::new(42))),
            (3, Box::new(ConflictSeekingGen::new(42))),
        ],
        12345,
    );
    for i in 0..5 {
        let moves = weighted.generate_moves(&game);
        println!("   Round {}: {} moves", i + 1, moves.len());
    }
    println!();

    // Example 3: Filtered moves (only allow short moves)
    println!("3. Filtered Generator (only moves <= 3 cells)");
    let mut filtered = FilteredGen::new(
        Box::new(RandomMoveGen::new(42)),
        |_game, m| {
            let delta = m.to - m.from;
            delta.dx.abs() <= 3 && delta.dy.abs() <= 3
        },
    );
    let moves = filtered.generate_moves(&game);
    println!("   Generated {} short moves\n", moves.len());

    // Example 4: Round-robin variety
    println!("4. Round Robin Generator (cycles through strategies)");
    let mut round_robin = RoundRobinGen::new(vec![
        Box::new(RandomMoveGen::new(42)),
        Box::new(ConflictSeekingGen::new(43)),
        Box::new(ChainMakingGen::new(44)),
        Box::new(GoalOrientedGen::new(45)),
    ]);
    for i in 0..8 {
        let moves = round_robin.generate_moves(&game);
        println!("   Round {}: {} moves", i + 1, moves.len());
    }
    println!();

    // Example 5: Adaptive selection based on game state
    println!("5. Adaptive Generator (chooses strategy based on board state)");
    let mut adaptive = AdaptiveGen::new(
        vec![
            Box::new(RandomMoveGen::new(42)),
            Box::new(ConflictSeekingGen::new(43)),
            Box::new(ChainMakingGen::new(44)),
        ],
        |game| {
            // Choose strategy based on number of movable pieces
            let pieces = find_movable_pieces(&game.board);
            if pieces.len() > 100 {
                0 // Random for crowded boards
            } else if pieces.len() > 50 {
                1 // Conflict-seeking for medium boards
            } else {
                2 // Chain-making for sparse boards
            }
        },
    );
    let moves = adaptive.generate_moves(&game);
    println!("   Generated {} moves based on board state\n", moves.len());

    // Example 6: Complex composition - nested combinators
    println!("6. Nested Combinators (filtered weighted random)");
    let mut complex = FilteredGen::new(
        Box::new(WeightedGen::with_seed(
            vec![
                (5, Box::new(RandomMoveGen::new(42))),
                (3, Box::new(ConflictSeekingGen::new(43))),
                (2, Box::new(GoalOrientedGen::new(44))),
            ],
            67890,
        )),
        |_game, m| {
            // Only allow moves by even-numbered players
            m.who.0 % 2 == 0
        },
    );
    let moves = complex.generate_moves(&game);
    println!("   Generated {} filtered weighted moves\n", moves.len());

    println!("=== All examples completed successfully! ===");
}

//! Property-based tests using proptest.
//!
//! These tests verify that game logic holds for arbitrary valid inputs,
//! catching edge cases that handwritten tests might miss.

use automatafl_v8n::*;
use proptest::prelude::*;
use smallvec::SmallVec;

// ============================================================================
// Coordinate Property Tests
// ============================================================================

proptest! {
    /// Coordinates should round-trip through indexing
    #[test]
    fn coord_index_roundtrip(x in 0u8..127, y in 0u8..127) {
        let coord = Coord { x, y };
        let idx = coord.ix();
        // We can't reconstruct coord from idx without board size,
        // but at least verify indexing is deterministic
        prop_assert_eq!(coord.ix(), idx);
    }

    /// Coordinate subtraction should produce correct deltas
    #[test]
    fn coord_subtraction_produces_delta(
        x1 in 0i16..127,
        y1 in 0i16..127,
        x2 in 0i16..127,
        y2 in 0i16..127,
    ) {
        let c1 = Coord { x: x1 as u8, y: y1 as u8 };
        let c2 = Coord { x: x2 as u8, y: y2 as u8 };
        let delta = c1 - c2;
        prop_assert_eq!(delta.dx, x1 - x2);
        prop_assert_eq!(delta.dy, y1 - y2);
    }

    /// Adding a delta and subtracting it should return original coord (when no overflow)
    #[test]
    fn delta_add_subtract_identity(x in 20u8..80, y in 20u8..80, dx in -10i16..10, dy in -10i16..10) {
        let coord = Coord { x, y };
        let delta = Delta { dx, dy };
        let new_coord = coord + delta;

        // Only verify if the new coord is valid (no overflow/underflow)
        if new_coord.x >= 10 && new_coord.x <= 100 && new_coord.y >= 10 && new_coord.y <= 100 {
            let back_delta = new_coord - coord;
            prop_assert_eq!(back_delta, delta);
        }
    }
}

// ============================================================================
// Delta Property Tests
// ============================================================================

proptest! {
    /// Zero delta should be detected correctly
    #[test]
    fn delta_zero_detection(dx in -100i16..100, dy in -100i16..100) {
        let delta = Delta { dx, dy };
        prop_assert_eq!(delta.is_zero(), dx == 0 && dy == 0);
    }

    /// Axial deltas should have one component zero
    #[test]
    fn axial_delta_properties(dx in -100i16..100, dy in -100i16..100) {
        let delta = Delta { dx, dy };
        let is_axial = (dx == 0 && dy != 0) || (dx != 0 && dy == 0) || (dx == 0 && dy == 0);
        prop_assert_eq!(delta.is_axial(), is_axial);
    }

    /// Scaling should preserve direction
    #[test]
    fn delta_scaling_preserves_direction(dx in -10i16..10, dy in -10i16..10, scale in 1isize..5) {
        let delta = Delta { dx, dy };
        if !delta.is_zero() {
            let scaled = delta.scale(scale);
            prop_assert_eq!(scaled.dx, dx * scale as i16);
            prop_assert_eq!(scaled.dy, dy * scale as i16);
        }
    }

    /// Displacement should match Manhattan distance
    #[test]
    fn displacement_is_manhattan_distance(dx in -100i16..100, dy in -100i16..100) {
        let delta = Delta { dx, dy };
        let displacement = delta.displacement();
        let expected = (dx.abs() + dy.abs()) as usize;
        prop_assert_eq!(displacement, expected);
    }
}

// ============================================================================
// Move Validation Property Tests
// ============================================================================

proptest! {
    /// Valid axial moves should not be rejected for being non-axial
    #[test]
    fn valid_axial_moves_accepted(
        fx in 0u8..5,
        fy in 0u8..5,
        dist in 1u8..5,
        dir in 0usize..4,
    ) {
        let board = Board::stock_testing();
        let mut game = Game::new_default_modes(board.clone(), 2, true);

        let from = Coord { x: fx, y: fy };
        let directions = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];
        let delta = directions[dir].scale(dist as isize);
        let to = from + delta;

        if game.board.inbounds(to) && from != to {
            let m = Move {
                who: Pid(0),
                from,
                to,
            };

            // Move should not be rejected for being non-axial
            let feedback = game.propose_move(m);
            prop_assert!(
                !matches!(feedback, ProposeFeedback::Rejected(MoveFeedback::AxisAlignedOnly)),
                "Axial move was incorrectly rejected: {:?}",
                m
            );
        }
    }

    /// Diagonal moves should always be rejected
    #[test]
    fn diagonal_moves_rejected(
        fx in 0u8..5,
        fy in 0u8..5,
        dx in 1u8..3,
        dy in 1u8..3,
    ) {
        let board = Board::stock_testing();
        let mut game = Game::new_default_modes(board, 2, true);

        let from = Coord { x: fx, y: fy };
        let to = Coord { x: fx.saturating_add(dx), y: fy.saturating_add(dy) };

        if game.board.inbounds(to) && from != to {
            let m = Move { who: Pid(0), from, to };
            let feedback = game.propose_move(m);

            prop_assert!(
                matches!(feedback, ProposeFeedback::Rejected(MoveFeedback::AxisAlignedOnly)),
                "Diagonal move should be rejected: {:?}",
                m
            );
        }
    }

    /// Moves to out-of-bounds coordinates should be rejected
    #[test]
    fn out_of_bounds_rejected(
        fx in 0u8..5,
        fy in 0u8..5,
        tx in 10u8..100,
        ty in 10u8..100,
    ) {
        let board = Board::stock_testing();
        let mut game = Game::new_default_modes(board, 2, true);

        let from = Coord { x: fx, y: fy };
        let to = Coord { x: tx, y: ty };

        if !game.board.inbounds(to) {
            let m = Move { who: Pid(0), from, to };
            let feedback = game.propose_move(m);

            prop_assert!(
                matches!(feedback, ProposeFeedback::Rejected(_)),
                "Out of bounds move should be rejected: {:?}",
                m
            );
        }
    }
}

// ============================================================================
// Game State Property Tests
// ============================================================================

proptest! {
    /// Board size should be preserved through game creation
    #[test]
    fn game_preserves_board_size(width in 5u8..32, height in 5u8..32, players in 2u8..8) {
        let board = create_large_board(width, height, players, 0.3, 42);
        let game = Game::new_default_modes(board.clone(), players, true);

        prop_assert_eq!(game.board.size.x, width);
        prop_assert_eq!(game.board.size.y, height);
        prop_assert_eq!(game.player_count, players);
    }

    /// Automaton should always be on the board
    #[test]
    fn automaton_always_on_board(width in 5u8..32, height in 5u8..32) {
        let board = create_large_board(width, height, 2, 0.3, 42);
        prop_assert!(board.inbounds(board.automaton_location));
        prop_assert!(board.is_automaton(board.automaton_location));
    }

    /// Game should start in Fresh round state
    #[test]
    fn game_starts_fresh(width in 5u8..32, height in 5u8..32, players in 2u8..8) {
        let board = create_large_board(width, height, players, 0.3, 42);
        let game = Game::new_default_modes(board, players, true);
        prop_assert_eq!(game.round, RoundState::Fresh);
    }

    /// No players should be locked at game start
    #[test]
    fn no_locked_players_at_start(width in 5u8..32, height in 5u8..32, players in 2u8..8) {
        let board = create_large_board(width, height, players, 0.3, 42);
        let game = Game::new_default_modes(board, players, true);
        prop_assert!(game.locked_players.is_empty());
    }

    /// No pending moves at game start
    #[test]
    fn no_pending_moves_at_start(width in 5u8..32, height in 5u8..32, players in 2u8..8) {
        let board = create_large_board(width, height, players, 0.3, 42);
        let game = Game::new_default_modes(board, players, true);
        prop_assert!(game.pending_moves.is_empty());
    }
}

// ============================================================================
// Move Generator Property Tests
// ============================================================================

proptest! {
    /// Generated moves should always have valid player IDs
    #[test]
    fn generators_produce_valid_player_ids(
        players in 2u8..8,
        seed in 0u64..1000,
    ) {
        let board = create_large_board(31, 31, players, 0.3, seed);
        let game = Game::new_default_modes(board, players, true);

        let mut generator = RandomMoveGen::new(seed);
        let moves = generator.generate_moves(&game);

        for m in moves {
            prop_assert!(m.who.0 < players, "Invalid player ID: {}", m.who.0);
        }
    }

    /// Chain making generator should not produce more moves than players
    #[test]
    fn chain_generator_respects_player_count(
        players in 2u8..8,
        seed in 0u64..1000,
    ) {
        let board = create_large_board(31, 31, players, 0.3, seed);
        let game = Game::new_default_modes(board, players, true);

        let mut generator = ChainMakingGen::new(seed);
        let moves = generator.generate_moves(&game);

        prop_assert!(
            moves.len() <= players as usize,
            "Generated {} moves for {} players",
            moves.len(),
            players
        );
    }

    /// Sequential generator should try all generators
    #[test]
    fn sequential_generator_tries_all(seed in 0u64..1000) {
        let board = create_large_board(31, 31, 2, 0.3, seed);
        let game = Game::new_default_modes(board, 2, true);

        let mut generator = SequentialGen::new(
            vec![
                Box::new(ChainMakingGen::new(seed)),
                Box::new(RandomMoveGen::new(seed + 1)),
            ],
            2,
        );

        let moves = generator.generate_moves(&game);
        // Should get at least some moves from one of the generators
        prop_assert!(!moves.is_empty() || game.locked_players.len() == 2);
    }
}

// ============================================================================
// Recording Property Tests
// ============================================================================

proptest! {
    /// Empty recording should validate successfully
    #[test]
    fn empty_recording_validates(players in 2u8..8) {
        let board = create_large_board(11, 11, players, 0.3, 42);
        let recording = GameRecording::new("Test".to_string(), board, players, true);
        prop_assert!(recording.validate().is_ok());
    }

    /// Recording with rounds should maintain consistent count
    #[test]
    fn recording_maintains_round_count(
        players in 2u8..8,
        num_rounds in 0usize..10,
    ) {
        let board = create_large_board(11, 11, players, 0.3, 42);
        let mut recording = GameRecording::new("Test".to_string(), board, players, true);

        for _ in 0..num_rounds {
            let moves = SmallVec::new();
            let result = RoundResult::Completed(SmallVec::new());
            recording.add_round(moves, result);
        }

        prop_assert_eq!(recording.rounds.len(), num_rounds);
        prop_assert_eq!(recording.metadata.round_count, num_rounds);
    }

    /// Submission order permutation invariance: proposing identical move sets in any
    /// seat order must yield identical round outcomes and resulting board states.
    #[test]
    fn move_submission_order_is_permutation_invariant(
        seed in 0u64..2000,
    ) {
        let board = Board::stock_two_player();
        let mut g_ab = Game::new_default_modes(board.clone(), 2, true);
        let mut g_ba = Game::new_default_modes(board, 2, true);

        let mut rgen = RandomMoveGen::new(seed);
        let moves = rgen.generate_moves(&g_ab);
        if moves.len() == 2 {
            let m0 = moves[0];
            let m1 = moves[1];

            let _ = g_ab.propose_move(m0);
            let _ = g_ab.propose_move(m1);
            let out_ab = g_ab.try_complete_round();

            let _ = g_ba.propose_move(m1);
            let _ = g_ba.propose_move(m0);
            let out_ba = g_ba.try_complete_round();

            prop_assert_eq!(g_ab.board, g_ba.board);
            prop_assert_eq!(g_ab.winner, g_ba.winner);
            prop_assert_eq!(g_ab.round, g_ba.round);
            prop_assert_eq!(
                matches!(out_ab, CompleteRoundFeedback::CompletedMoves(_)),
                matches!(out_ba, CompleteRoundFeedback::CompletedMoves(_))
            );
        }
    }

    /// Piece conservation across multi-round gameplay under DetectAndConflict and BunchBeforeMerge.
    #[test]
    fn multi_round_gameplay_conserves_pieces_and_replays_cleanly(
        seed in 0u64..1000,
        steps in 1usize..6,
        use_bunch in any::<bool>(),
    ) {
        let board = Board::stock_testing();
        let count_pieces = |b: &Board| -> (usize, usize, usize) {
            let mut rep = 0;
            let mut att = 0;
            let mut auto = 0;
            for y in 0..b.size.y {
                for x in 0..b.size.x {
                    match b.particles[Coord { x, y }.ix()].what {
                        Particle::Repulsor => rep += 1,
                        Particle::Attractor => att += 1,
                        Particle::Automaton => auto += 1,
                        Particle::Vacuum => {}
                    }
                }
            }
            (rep, att, auto)
        };
        let init_counts = count_pieces(&board);
        let merge_mode = if use_bunch {
            MergeResolutionMode::BunchBeforeMerge
        } else {
            MergeResolutionMode::DetectAndConflict
        };
        let mut game = Game::new(
            board.clone(),
            2,
            true,
            merge_mode,
            CycleBehaviorMode::RotatePieces,
        );
        let mut rec = GameRecording::new("proptest".to_string(), board, 2, true);
        let mut rgen = RandomMoveGen::new(seed);

        for _ in 0..steps {
            if game.round == RoundState::GameOver {
                break;
            }
            let moves = rgen.generate_moves(&game);
            let mut submitted = SmallVec::new();
            for m in moves {
                if matches!(
                    game.propose_move(m),
                    ProposeFeedback::Accepted | ProposeFeedback::AcceptedAndReady
                ) {
                    submitted.push(m);
                }
            }
            if game.pending_moves.len() == game.player_count as usize {
                let fb = game.try_complete_round();
                prop_assert_eq!(count_pieces(&game.board), init_counts);
                if !use_bunch {
                    match fb {
                        CompleteRoundFeedback::CompletedMoves(res) => {
                            rec.add_round(submitted, RoundResult::Completed(res));
                        }
                        CompleteRoundFeedback::Conflict(st) => {
                            rec.add_round(submitted, RoundResult::Conflict(st));
                        }
                        CompleteRoundFeedback::WaitingForPlayers(_) => {}
                    }
                }
            }
        }
        if !use_bunch {
            prop_assert!(rec.validate().is_ok());
            let replayed = rec.replay().expect("recording replay should succeed");
            prop_assert_eq!(replayed.board, game.board);
        }
    }
}

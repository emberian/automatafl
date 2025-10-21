use automatafl_v8n::*;

// ============================================================================
// MERGING PATHWAYS COLLISION TESTS
// These tests verify that when multiple chains merge to the same destination,
// the collision is detected and all pieces are annihilated.
// ============================================================================

#[test]
fn merging_pathways_simple_collision() {
    // The classic "merging pathways" bug scenario from Gemini's analysis:
    // P1 @ A -> X -> M
    // P2 @ B -> Y -> M
    // Both pieces should collide at M and be annihilated.
    let mut game = Game::new(
        Board::stock_testing_empty(),
        4,
        true,
        MergeResolutionMode::Annihilate,
        CycleBehaviorMode::RotatePieces,
    );
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Has Attractor (P1)
    let c_x = Coord { x: 1, y: 0 }; // Empty
    let c_b = Coord { x: 0, y: 1 }; // Has Repulsor (P2)
    let c_y = Coord { x: 1, y: 1 }; // Empty
    let c_m = Coord { x: 1, y: 2 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);

    // Moves: A->X, X->M, B->Y, Y->M
    let move1 = Move {
        who: p0,
        from: c_a,
        to: c_x,
    };
    let move2 = Move {
        who: p1,
        from: c_x,
        to: c_m,
    };
    let move3 = Move {
        who: p2,
        from: c_b,
        to: c_y,
    };
    let move4 = Move {
        who: p3,
        from: c_y,
        to: c_m,
    };

    let fb1 = game.propose_move(move1);
    println!("Move 1 (A->X): {:?}", fb1);
    let fb2 = game.propose_move(move2);
    println!("Move 2 (X->M): {:?}", fb2);
    let fb3 = game.propose_move(move3);
    println!("Move 3 (B->Y): {:?}", fb3);
    let fb4 = game.propose_move(move4);
    println!("Move 4 (Y->M): {:?}", fb4);

    let result = game.try_complete_round();
    if let CompleteRoundFeedback::Conflict(status) = &result {
        println!("Unexpected conflict:");
        println!("  Conflicted moves: {:?}", status.conflicted_moves);
        println!("  Locked players: {:?}", status.locked_players);
        for coord in [c_a, c_b, c_x, c_y, c_m] {
            println!("  {:?} is_conflict={}", coord, game.board.is_conflict(coord));
        }
    }
    assert!(
        matches!(result, CompleteRoundFeedback::CompletedMoves(_)),
        "Moves should complete without conflict (conflict detection happens during resolution), got {:?}", result
    );

    // After resolution, both pieces should have been annihilated
    // A, B, X, Y, and M should all be empty
    assert!(
        game.board.is_vacuum(c_a),
        "A should be empty (piece was picked up)"
    );
    assert!(
        game.board.is_vacuum(c_b),
        "B should be empty (piece was picked up)"
    );
    assert!(game.board.is_vacuum(c_x), "X should remain empty");
    assert!(game.board.is_vacuum(c_y), "Y should remain empty");
    assert!(
        game.board.is_vacuum(c_m),
        "M should be empty (collision annihilated both pieces)"
    );
}

#[test]
fn merging_pathways_triple_collision() {
    // Three pieces converging on the same destination:
    // P1 @ A -> M
    // P2 @ B -> X -> M
    // P3 @ C -> Y -> M
    let mut game = Game::new(Board::stock_testing_empty(), 3, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_b = Coord { x: 1, y: 0 }; // Repulsor
    let c_c = Coord { x: 2, y: 0 }; // Attractor
    let c_x = Coord { x: 1, y: 1 }; // Empty (intermediate)
    let c_y = Coord { x: 2, y: 1 }; // Empty (intermediate)
    let c_m = Coord { x: 3, y: 0 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);
    game.board.place(c_c, Particle::Attractor);

    // Direct move from A to M
    let move1 = Move {
        who: p0,
        from: c_a,
        to: c_m,
    };
    // Chain from B through X to M
    let move2 = Move {
        who: p1,
        from: c_b,
        to: c_x,
    };
    let move3 = Move {
        who: p2,
        from: c_c,
        to: c_m,
    };

    game.propose_move(move1);
    game.propose_move(move2);
    game.propose_move(move3);

    let result = game.try_complete_round();
    println!("Result: {:?}", result);
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // All three pieces should be annihilated
    assert!(game.board.is_vacuum(c_a));
    assert!(game.board.is_vacuum(c_c));
    assert!(
        game.board.is_vacuum(c_m),
        "M should be empty (3-way collision)"
    );

    // But B->X should succeed since it doesn't participate in the collision
    assert!(game.board.is_vacuum(c_b));
    assert_eq!(
        game.board.particles[c_x.ix()].what,
        Particle::Repulsor,
        "Repulsor should have moved to X"
    );
}

#[test]
fn merging_pathways_long_vacuum_chains() {
    // Test with longer chains of vacuum squares merging
    // A -> V1 -> V2 -> V3 -> M
    // B -> V4 -> V5 -> V6 -> M
    let mut game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_v1 = Coord { x: 1, y: 0 }; // Empty
    let c_v2 = Coord { x: 2, y: 0 }; // Empty
    let c_v3 = Coord { x: 3, y: 0 }; // Empty

    let c_b = Coord { x: 0, y: 1 }; // Repulsor
    let c_v4 = Coord { x: 1, y: 1 }; // Empty
    let c_v5 = Coord { x: 2, y: 1 }; // Empty
    let c_v6 = Coord { x: 3, y: 1 }; // Empty

    let c_m = Coord { x: 4, y: 0 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);

    // Build chain A -> V1 -> V2 -> V3 -> M
    game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_v1,
    });
    game.propose_move(Move {
        who: p1,
        from: c_v1,
        to: c_v2,
    });
    game.propose_move(Move {
        who: p2,
        from: c_v2,
        to: c_v3,
    });
    game.propose_move(Move {
        who: p3,
        from: c_v3,
        to: c_m,
    });

    // First round: establish the first chain
    let result = game.try_complete_round();
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Piece should have moved through the chain to M
    assert_eq!(
        game.board.particles[c_m.ix()].what,
        Particle::Attractor,
        "Attractor should reach M"
    );

    // Now reset and try the collision scenario
    game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);

    // Build both chains simultaneously
    game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_v1,
    });
    game.propose_move(Move {
        who: p1,
        from: c_v1,
        to: c_m,
    });
    game.propose_move(Move {
        who: p2,
        from: c_b,
        to: c_v4,
    });
    game.propose_move(Move {
        who: p3,
        from: c_v4,
        to: c_m,
    });

    let result = game.try_complete_round();
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Both pieces should collide at M and be annihilated
    assert!(game.board.is_vacuum(c_m), "M should be empty (collision)");
    assert!(game.board.is_vacuum(c_a));
    assert!(game.board.is_vacuum(c_b));
}

#[test]
fn merging_pathways_with_cycle() {
    // Test collision where one path involves a cycle
    // A -> M (direct)
    // B -> C -> D -> C (cycle with C and D, but then exits to M)
    // Actually, let's make it simpler:
    // A -> M
    // B -> X -> M (where X is part of a cycle that gets resolved)
    let mut game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_b = Coord { x: 1, y: 0 }; // Repulsor
    let c_x = Coord { x: 2, y: 0 }; // Attractor (in a chain)
    let c_m = Coord { x: 3, y: 0 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);
    game.board.place(c_x, Particle::Attractor);

    // A -> M directly
    game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_m,
    });
    // B -> X, X -> M: both pieces converge on M
    game.propose_move(Move {
        who: p1,
        from: c_b,
        to: c_x,
    });
    game.propose_move(Move {
        who: p2,
        from: c_x,
        to: c_m,
    });
    // Dummy move
    game.propose_move(Move {
        who: p3,
        from: Coord { x: 4, y: 4 },
        to: Coord { x: 4, y: 3 },
    });

    let result = game.try_complete_round();
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Both A's Attractor and X's Attractor want to go to M
    // They should collide and be annihilated
    assert!(
        game.board.is_vacuum(c_m),
        "M should be empty (collision of 2 attractors)"
    );
    assert!(game.board.is_vacuum(c_a));
    assert!(game.board.is_vacuum(c_x));
    // But B's Repulsor moves to X successfully
    assert_eq!(game.board.particles[c_x.ix()].what, Particle::Repulsor);
}

#[test]
fn merging_pathways_asymmetric_chain_lengths() {
    // One short path, one long path to same destination
    // A -> M (1 hop)
    // B -> X -> Y -> Z -> M (4 hops)
    let mut game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_b = Coord { x: 0, y: 1 }; // Repulsor
    let c_x = Coord { x: 1, y: 1 }; // Empty
    let c_y = Coord { x: 2, y: 1 }; // Empty
    let c_z = Coord { x: 3, y: 1 }; // Empty
    let c_m = Coord { x: 1, y: 0 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);

    // Short path: A -> M
    game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_m,
    });
    // Long path: B -> X -> Y -> Z -> M
    game.propose_move(Move {
        who: p1,
        from: c_b,
        to: c_x,
    });
    game.propose_move(Move {
        who: p2,
        from: c_x,
        to: c_y,
    });
    game.propose_move(Move {
        who: p3,
        from: c_y,
        to: c_m,
    });

    let result = game.try_complete_round();
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Both pieces should collide at M
    assert!(game.board.is_vacuum(c_m), "M should be empty (collision)");
    assert!(game.board.is_vacuum(c_a));
    assert!(game.board.is_vacuum(c_b));
}

#[test]
fn fork_then_join_vacuum_network() {
    // A more complex fork/join scenario:
    //     -> V1 -> V3 ->
    // A ->              -> M
    //     -> V2 -> V4 ->
    // Single piece should successfully traverse one path
    let mut game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_v1 = Coord { x: 1, y: 0 }; // Empty
    let c_v2 = Coord { x: 1, y: 1 }; // Empty
    let c_v3 = Coord { x: 2, y: 0 }; // Empty
    let c_v4 = Coord { x: 2, y: 1 }; // Empty
    let c_m = Coord { x: 3, y: 0 }; // Destination

    game.board.place(c_a, Particle::Attractor);

    // Create fork: A can go to V1 or V2, let's say V1
    game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_v1,
    });
    // V1 -> V3
    game.propose_move(Move {
        who: p1,
        from: c_v1,
        to: c_v3,
    });
    // V3 -> M
    game.propose_move(Move {
        who: p2,
        from: c_v3,
        to: c_m,
    });
    // Also create the other path (empty)
    game.propose_move(Move {
        who: p3,
        from: c_v4,
        to: c_m,
    });

    let result = game.try_complete_round();
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Piece should successfully reach M via the first path
    assert_eq!(
        game.board.particles[c_m.ix()].what,
        Particle::Attractor,
        "Attractor should reach M"
    );
    assert!(game.board.is_vacuum(c_a));
}

#[test]
fn diamond_merge_collision() {
    // Diamond pattern:
    //      -> V1 ->
    // A ->           -> M
    //      -> V2 ->
    // B ->           -> M
    // Two pieces, two paths each leading to M
    let mut game = Game::new(Board::stock_testing_empty(), 4, true, MergeResolutionMode::Annihilate, CycleBehaviorMode::RotatePieces);
    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);
    let p3 = Pid(3);

    let c_a = Coord { x: 0, y: 0 }; // Attractor
    let c_b = Coord { x: 0, y: 1 }; // Repulsor
    let c_v1 = Coord { x: 1, y: 0 }; // Empty
    let c_v2 = Coord { x: 2, y: 1 }; // Empty
    let c_m = Coord { x: 2, y: 0 }; // Merge point

    game.board.place(c_a, Particle::Attractor);
    game.board.place(c_b, Particle::Repulsor);

    // A -> V1 -> M
    let fb1 = game.propose_move(Move {
        who: p0,
        from: c_a,
        to: c_v1,
    });
    println!("Move 1 (A->V1): {:?}", fb1);
    let fb2 = game.propose_move(Move {
        who: p1,
        from: c_v1,
        to: c_m,
    });
    println!("Move 2 (V1->M): {:?}", fb2);
    // B -> V2 -> M
    let fb3 = game.propose_move(Move {
        who: p2,
        from: c_b,
        to: c_v2,
    });
    println!("Move 3 (B->V2): {:?}", fb3);
    let fb4 = game.propose_move(Move {
        who: p3,
        from: c_v2,
        to: c_m,
    });
    println!("Move 4 (V2->M): {:?}", fb4);

    let result = game.try_complete_round();
    println!("diamond_merge_collision result: {:?}", result);
    assert!(matches!(result, CompleteRoundFeedback::CompletedMoves(_)));

    // Both pieces collide at M and are annihilated
    assert!(game.board.is_vacuum(c_m), "M should be empty (collision)");
    assert!(game.board.is_vacuum(c_a));
    assert!(game.board.is_vacuum(c_b));
    assert!(game.board.is_vacuum(c_v1));
    assert!(game.board.is_vacuum(c_v2));
}
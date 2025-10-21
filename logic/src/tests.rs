use crate::*;

#[derive(Debug)]
struct AutMoveError {
    board: Board,
    expected_move: Delta,
    actual_move: Delta,
}

type AutMoveTest = Result<(), AutMoveError>;

fn expect_automaton_move(game: &mut Game, by: Delta) -> AutMoveTest {
    let t0 = game.board.automaton_location;
    let d = game.automaton_move() - t0;
    if d == by {
        Ok(())
    } else {
        Err(AutMoveError {
            board: game.board.clone(),
            expected_move: by,
            actual_move: d,
        })
    }
}

fn testing_game() -> Game {
    Game::new(Board::stock_testing_empty(), 2, true)
}

#[test]
fn automaton_stays_put() -> AutMoveTest {
    let board = Board::stock_two_player();
    let mut game = Game::new(board, 2, true);
    expect_automaton_move(&mut game, Delta::ZERO)
}

#[test]
fn unbalanced_pair() -> AutMoveTest {
    for &d in Delta::AXIAL_UNITS.iter() {
        let mut game = testing_game();
        let loc = game.board.automaton_location;
        game.board.place(loc + d.scale(2), Particle::Attractor);
        game.board.place(loc + d.scale(-2), Particle::Repulsor);
        println!("* empty UnP delta {:?}", d);
        expect_automaton_move(&mut game, d)?;

        let clean_board = game.board.clone();
        let perp = d.perpendicular();

        game.board.place(loc + perp.scale(2), Particle::Attractor);
        println!("* UnP delta {:?} unaffected by unipolar attractor", d);
        expect_automaton_move(&mut game, d)?;
        game.board.place(loc + perp.scale(-2), Particle::Attractor);
        println!("* UnP delta {:?} unaffected by bipolar attractor", d);
        expect_automaton_move(&mut game, d)?;

        game.board = clean_board;

        game.board.place(loc + perp.scale(2), Particle::Repulsor);
        println!("* UnP delta {:?} unaffected by unipolar repulsor", d);
        expect_automaton_move(&mut game, d)?;
        game.board.place(loc + perp.scale(-2), Particle::Repulsor);
        println!("* UnP delta {:?} unaffected by bipolar repulsor", d);
        expect_automaton_move(&mut game, d)?;
    }
    Ok(())
}

#[test]
fn unbalanced_pair_limits() -> AutMoveTest {
    for &d in Delta::AXIAL_UNITS.iter() {
        let mut game = testing_game();
        let loc = game.board.automaton_location;

        let clean_board = game.board.clone();

        game.board.place(loc + d.scale(1), Particle::Attractor);
        game.board.place(loc + d.scale(-2), Particle::Repulsor);
        expect_automaton_move(&mut game, Delta::ZERO)?;
        println!("* no move when adjacent to UnP attractor, delta {:?}", d);

        game.board = clean_board;

        game.board.place(loc + d.scale(2), Particle::Attractor);
        game.board.place(loc + d.scale(-1), Particle::Repulsor);
        println!("* still moves when UnP repulsor is adjacent, delta {:?}", d);
        expect_automaton_move(&mut game, d)?;
    }
    Ok(())
}

#[test]
fn repulsor() -> AutMoveTest {
    for &d in Delta::AXIAL_UNITS.iter() {
        let mut game = testing_game();
        let loc = game.board.automaton_location;
        let perp = d.perpendicular();

        let try_with_attractors = |g: &mut Game, e: Delta| -> AutMoveTest {
            g.board.place(loc + perp.scale(2), Particle::Attractor);
            println!("* ...with unipolar attractor");
            expect_automaton_move(g, e)?;
            g.board.place(loc + perp.scale(-2), Particle::Attractor);
            println!("* ...with bipolar attractor");
            expect_automaton_move(g, e)?;
            Ok(())
        };

        let clean_board = game.board.clone();

        game.board.place(loc + d.scale(-1), Particle::Repulsor);
        println!("* away from adjacent repulsor, unipolar, delta {:?}", d);
        expect_automaton_move(&mut game, d)?;
        try_with_attractors(&mut game, d)?;

        game.board = clean_board.clone();
        game.board.place(loc + d.scale(-2), Particle::Repulsor);
        println!("* away from far repulsor, unipolar, delta {:?}", d);
        expect_automaton_move(&mut game, d)?;
        try_with_attractors(&mut game, d)?;

        game.board = clean_board.clone();
        game.board.place(loc + d.scale(-1), Particle::Repulsor);
        game.board.place(loc + d.scale(2), Particle::Repulsor);
        println!("* away from nearer repulsor, bipolar, delta {:?}", d);
        expect_automaton_move(&mut game, d)?;
        try_with_attractors(&mut game, d)?;
    }
    Ok(())
}

#[test]
fn trapped_all_sides() -> AutMoveTest {
    // TODO
    Ok(())
}

#[test]
fn conflict_resolution_basic() {
    let mut game = Game::new(Board::stock_testing(), 2, true);

    // Place pieces for conflict
    game.board.place(Coord { x: 1, y: 2 }, Particle::Attractor);

    let p0 = Pid(0);
    let p1 = Pid(1);

    // Both players try to move to same destination - should conflict
    let move1 = Move {
        who: p0,
        from: Coord { x: 0, y: 2 },
        to: Coord { x: 1, y: 2 },
    };
    let move2 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 2 },
    };

    game.board.place(move1.from, Particle::Attractor);
    game.board.place(move2.from, Particle::Repulsor);

    let (fb1, _) = game.propose_move(move1);
    let (fb2, ready) = game.propose_move(move2);

    assert_eq!(fb1, MoveFeedback::Committed);
    assert_eq!(fb2, MoveFeedback::Committed);
    assert!(ready);

    // Should result in conflict
    let result = game.try_complete_round();
    assert!(result.is_err());

    // Both players should have their moves removed
    assert_eq!(game.pending_moves.len(), 0);

    // Destination should be marked as conflict
    assert!(game.board.is_conflict(Coord { x: 1, y: 2 }));

    // Neither player should be locked (they conflicted)
    assert_eq!(game.locked_players.len(), 0);
}

#[test]
fn conflict_resolution_source_conflict() {
    let mut game = Game::new(Board::stock_testing(), 2, true);

    let p0 = Pid(0);
    let p1 = Pid(1);

    // Both players try to move the same piece - should conflict
    let shared_source = Coord { x: 1, y: 3 };
    let move1 = Move {
        who: p0,
        from: shared_source,
        to: Coord { x: 0, y: 3 },
    };
    let move2 = Move {
        who: p1,
        from: shared_source,
        to: Coord { x: 1, y: 4 },
    };

    game.board.place(shared_source, Particle::Attractor);

    let (fb1, _) = game.propose_move(move1);
    let (fb2, ready) = game.propose_move(move2);

    assert_eq!(fb1, MoveFeedback::Committed);
    assert_eq!(fb2, MoveFeedback::Committed);
    assert!(ready);

    // Should result in conflict
    let result = game.try_complete_round();
    assert!(result.is_err());

    // Source should be marked as conflict
    assert!(game.board.is_conflict(shared_source));
}

#[test]
fn conflict_resolution_locked_players() {
    let mut game = Game::new(Board::stock_testing(), 3, true);

    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);

    // P0 and P1 conflict on destination, P2 is fine
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);
    game.board.place(Coord { x: 3, y: 1 }, Particle::Attractor);

    let move1 = Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 1, y: 1 },
    };
    let move2 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 1 },
    }; // Conflicts with P0
    let move3 = Move {
        who: p2,
        from: Coord { x: 3, y: 1 },
        to: Coord { x: 3, y: 0 },
    }; // No conflict

    let (_, _) = game.propose_move(move1);
    let (_, _) = game.propose_move(move2);
    let (_, ready) = game.propose_move(move3);

    assert!(ready, "Should be ready when all players submitted");

    let result = game.try_complete_round();
    assert!(result.is_err());

    // P2 should be locked (didn't conflict), P0 and P1 should NOT be locked
    assert_eq!(game.locked_players.len(), 1);
    assert!(game.locked_players.contains(&p2));
    assert!(!game.locked_players.contains(&p0));
    assert!(!game.locked_players.contains(&p1));

    // P2's move should still be pending
    assert_eq!(game.pending_moves.len(), 1);
    assert_eq!(game.pending_moves[0].who, p2);

    // Now P0 and P1 resubmit
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

    let move4 = Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 0, y: 0 },
    };
    let move5 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 4 },
    };

    let (_, _) = game.propose_move(move4);
    let (_, ready2) = game.propose_move(move5);

    assert!(ready2, "Should be ready when all players submitted again");

    // Now should succeed
    let result2 = game.try_complete_round();
    assert!(result2.is_ok());
}

#[test]
fn conflict_resolution_resubmit() {
    let mut game = Game::new(Board::stock_testing(), 2, true);

    let p0 = Pid(0);
    let p1 = Pid(1);

    // Setup conflict: both players move to same destination
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

    let move1 = Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 1, y: 1 },
    };
    let move2 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 1 },
    };

    let (fb1, ready_after_1) = game.propose_move(move1);
    let (fb2, ready_after_2) = game.propose_move(move2);

    assert_eq!(fb1, MoveFeedback::Committed);
    assert_eq!(fb2, MoveFeedback::Committed);
    assert!(
        !ready_after_1 && ready_after_2,
        "Should be ready after both players submit"
    );

    let result = game.try_complete_round();
    assert!(result.is_err());
    assert!(game.board.is_conflict(Coord { x: 1, y: 1 }));

    // Now conflicted players should be able to resubmit
    // But they can't use the conflicted square
    let move3 = Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 0, y: 0 },
    };
    let (fb3, _) = game.propose_move(move3);
    assert_eq!(fb3, MoveFeedback::Committed); // Should work now

    // P1 also needs to resubmit
    let move4 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 4 },
    };
    let (fb4, ready) = game.propose_move(move4);
    assert_eq!(fb4, MoveFeedback::Committed);
    assert!(ready, "Should be ready after both players resubmit");

    // Now it should complete successfully
    let result2 = game.try_complete_round();
    assert!(result2.is_ok());
}

#[test]
fn move_cycle_basic() {
    let mut game = Game::new(Board::stock_testing(), 2, true);

    let p0 = Pid(0);
    let p1 = Pid(1);

    // Setup: piece at (1,1) and (2,1), swap them
    game.board.place(Coord { x: 1, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 2, y: 1 }, Particle::Repulsor);

    let move1 = Move {
        who: p0,
        from: Coord { x: 1, y: 1 },
        to: Coord { x: 2, y: 1 },
    };
    let move2 = Move {
        who: p1,
        from: Coord { x: 2, y: 1 },
        to: Coord { x: 1, y: 1 },
    };

    game.propose_move(move1);
    game.propose_move(move2);

    let result = game.try_complete_round();
    assert!(result.is_ok());

    let results = result.unwrap();
    // Both moves should succeed due to passable marking
    assert_eq!(results.len(), 2);
}

#[test]
fn locked_players_cleared_after_success() {
    let mut game = Game::new(Board::stock_testing(), 3, true);

    let p0 = Pid(0);
    let p1 = Pid(1);
    let p2 = Pid(2);

    // First round: P0 and P1 conflict, P2 locks
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);
    game.board.place(Coord { x: 3, y: 1 }, Particle::Attractor);

    let (_, _) = game.propose_move(Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 1, y: 1 },
    });
    let (_, _) = game.propose_move(Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 1 },
    });
    let (_, ready1) = game.propose_move(Move {
        who: p2,
        from: Coord { x: 3, y: 1 },
        to: Coord { x: 3, y: 0 },
    });

    assert!(ready1);

    game.try_complete_round().unwrap_err();
    assert_eq!(game.locked_players.len(), 1);
    assert!(game.locked_players.contains(&p2));

    // Now P0 and P1 resubmit without conflict
    // Reset the pieces since they're still at their original locations
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

    let (_, _) = game.propose_move(Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 0, y: 0 },
    });
    let (_, ready2) = game.propose_move(Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 4 },
    });

    assert!(ready2, "Should be ready after conflicted players resubmit");

    // P2's move is still pending from before, so we should have 3 moves now
    assert_eq!(game.pending_moves.len(), 3);

    let result = game.try_complete_round();
    assert!(result.is_ok());

    // locked_players should be cleared after successful round
    assert_eq!(game.locked_players.len(), 0);
}

#[test]
fn pending_moves_cleared_after_successful_round() {
    let mut game = Game::new(Board::stock_testing(), 2, true);

    let p0 = Pid(0);
    let p1 = Pid(1);

    // Setup two valid non-conflicting moves
    game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

    let move1 = Move {
        who: p0,
        from: Coord { x: 0, y: 1 },
        to: Coord { x: 0, y: 0 },
    };
    let move2 = Move {
        who: p1,
        from: Coord { x: 1, y: 3 },
        to: Coord { x: 1, y: 4 },
    };

    let (fb1, _) = game.propose_move(move1);
    let (fb2, ready) = game.propose_move(move2);

    assert_eq!(fb1, MoveFeedback::Committed);
    assert_eq!(fb2, MoveFeedback::Committed);
    assert!(ready, "Should be ready when both players submitted");
    assert_eq!(
        game.pending_moves.len(),
        2,
        "Should have 2 pending moves before round completes"
    );

    // Complete the round successfully
    let result = game.try_complete_round();
    assert!(result.is_ok(), "Round should complete successfully");

    // CRITICAL: pending_moves must be cleared for the next round
    assert_eq!(
        game.pending_moves.len(),
        0,
        "pending_moves should be cleared after successful round"
    );
    assert_eq!(
        game.round,
        RoundState::Fresh,
        "Round should be back to Fresh state"
    );

    // Now players should be able to submit new moves for the next round
    game.board.place(Coord { x: 0, y: 0 }, Particle::Attractor);
    game.board.place(Coord { x: 1, y: 4 }, Particle::Repulsor);

    let move3 = Move {
        who: p0,
        from: Coord { x: 0, y: 0 },
        to: Coord { x: 1, y: 0 },
    };
    let move4 = Move {
        who: p1,
        from: Coord { x: 1, y: 4 },
        to: Coord { x: 2, y: 4 },
    };

    let (fb3, _) = game.propose_move(move3);
    let (fb4, ready2) = game.propose_move(move4);

    assert_eq!(
        fb3,
        MoveFeedback::Committed,
        "Should be able to submit new moves"
    );
    assert_eq!(
        fb4,
        MoveFeedback::Committed,
        "Should be able to submit new moves"
    );
    assert!(ready2, "Should be ready for second round");
    assert_eq!(
        game.pending_moves.len(),
        2,
        "Should have 2 new pending moves"
    );
}

#[test]
fn automaton_unbalanced_tiebreaker_prefers_further_repulsor() -> AutMoveTest {
    let mut game = testing_game();
    let loc = game.board.automaton_location;

    game.board
        .place(loc + Delta::XP.scale(2), Particle::Attractor);
    game.board
        .place(loc + Delta::XN.scale(1), Particle::Repulsor);

    game.board
        .place(loc + Delta::YP.scale(2), Particle::Attractor);
    game.board
        .place(loc + Delta::YN.scale(2), Particle::Repulsor);

    expect_automaton_move(&mut game, Delta::XP)
}

#[test]
fn automaton_unbalanced_tiebreaker_prefers_closer_repulsor() -> AutMoveTest {
    let mut game = Game::new(Board::stock_testing_empty_6(), 2, true);
    let loc = game.board.automaton_location;

    game.board
        .place(loc + Delta::XP.scale(2), Particle::Attractor);
    game.board
        .place(loc + Delta::XN.scale(2), Particle::Repulsor);

    game.board
        .place(loc + Delta::YN.scale(2), Particle::Attractor);
    game.board
        .place(loc + Delta::YP.scale(3), Particle::Repulsor);

    expect_automaton_move(&mut game, Delta::XP)
}

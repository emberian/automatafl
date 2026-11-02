//! The win condition, end to end.
//!
//! `Game::try_complete_round` decides the winner by scanning `Game::goals` for
//! the automaton's square. Before this suite existed nothing in `logic/` ever
//! put anything in `goals`, so the scan could not match, `winner` stayed `None`
//! forever, and `logic/src/main.rs`'s `while !game.winner.is_some()` was an
//! infinite loop. These tests pin the README's "Initial Setup" and
//! "Win Condition" to executable behaviour.

use automatafl_v8n::*;

/// README, Initial Setup: "In a two-player game, each player picks two corners
/// that are in the same row." The stock assignment gives seat 0 the `y = 0` row
/// and seat 1 the `y = size - 1` row.
#[test]
fn stock_two_player_goals_are_the_four_corners() {
    let game = Game::new_default_modes(Board::stock_two_player(), 2, true);
    assert_eq!(
        game.goals.as_slice(),
        &[
            (Coord { x: 0, y: 0 }, Pid(0)),
            (Coord { x: 10, y: 0 }, Pid(0)),
            (Coord { x: 0, y: 10 }, Pid(1)),
            (Coord { x: 10, y: 10 }, Pid(1)),
        ],
        "stock two-player goals are wrong"
    );
}

/// The stock assignment is only defined for two players; anything else has to
/// be set explicitly by the caller (README: four-player games pick one corner
/// each, and the pairing is a player decision, not a board property).
#[test]
fn non_two_player_games_get_no_stock_goals() {
    let game = Game::new_default_modes(Board::stock_two_player(), 4, true);
    assert!(
        game.goals.is_empty(),
        "a four-player game must not silently inherit the two-player assignment"
    );
}

/// A whole game, played to a win, through the public state machine only.
///
/// Both players submit vacuum-to-vacuum moves, which change no pieces, so the
/// only thing that moves is the automaton. Two repulsors drive it out of the
/// centre and into seat 1's corner in exactly four rounds.
#[test]
fn a_two_player_game_reaches_a_winner() {
    let mut game = Game::new_default_modes(Board::stock_testing_empty(), 2, true);
    assert!(!game.goals.is_empty(), "no goals means no reachable winner");

    // Neither repulsor sits on a goal corner, so neither blocks a win.
    game.board.place(Coord { x: 0, y: 2 }, Particle::Repulsor);
    game.board.place(Coord { x: 4, y: 1 }, Particle::Repulsor);

    // Moves between vacuum squares on a row the automaton never visits: legal,
    // non-conflicting, and they leave the board untouched.
    let idle = [
        Move {
            who: Pid(0),
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 1, y: 1 },
        },
        Move {
            who: Pid(1),
            from: Coord { x: 2, y: 1 },
            to: Coord { x: 3, y: 1 },
        },
    ];

    let expected_walk = [
        Coord { x: 3, y: 2 },
        Coord { x: 4, y: 2 },
        Coord { x: 4, y: 3 },
        Coord { x: 4, y: 4 },
    ];

    for (round, expected) in expected_walk.iter().enumerate() {
        assert!(
            game.winner.is_none(),
            "game ended before round {round}, at {:?}",
            game.board.automaton_location
        );

        for m in idle {
            match game.propose_move(m) {
                ProposeFeedback::Rejected(why) => panic!("round {round}: {m:?} rejected: {why}"),
                _ => {}
            }
        }

        match game.try_complete_round() {
            CompleteRoundFeedback::CompletedMoves(_) => {}
            other => panic!("round {round}: expected the round to complete, got {other:?}"),
        }

        assert_eq!(
            game.board.automaton_location, *expected,
            "round {round}: automaton walked somewhere unexpected"
        );
    }

    assert_eq!(
        game.winner,
        Some(Pid(1)),
        "the automaton reached seat 1's corner {:?} but no one won",
        game.board.automaton_location
    );
    assert_eq!(game.round, RoundState::GameOver);
}

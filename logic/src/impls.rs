use crate::*;

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {}", coord, feedback)?
        }
        Ok(())
    }
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl std::ops::Sub for Coord {
    type Output = Delta;

    fn sub(self, other: Coord) -> Delta {
        Delta {
            dx: (self.x as i8 - other.x as i8),
            dy: (self.y as i8 - other.y as i8),
        }
    }
}

impl std::ops::Add<Delta> for Coord {
    type Output = Coord;

    fn add(self, other: Delta) -> Coord {
        Coord {
            // TODO: Make this saturate rather than wrap
            x: (self.x as i8 + other.dx) as u8,
            y: (self.y as i8 + other.dy) as u8,
        }
    }
}

impl std::ops::Mul<isize> for Delta {
    type Output = Delta;

    fn mul(self, other: isize) -> Delta {
        Delta {
            dx: self.dx * other as i8,
            dy: self.dy * other as i8,
        }
    }
}

impl Coord {
    pub fn ix(self) -> (usize, usize) {
        (self.x as usize, self.y as usize)
    }
}

impl Delta {
    pub(crate) const ZERO: Delta = Delta { dx: 0, dy: 0 };
    pub(crate) const XP: Delta = Delta { dx: 1, dy: 0 };
    pub(crate) const XN: Delta = Delta { dx: -1, dy: 0 };
    pub(crate) const YP: Delta = Delta { dx: 0, dy: 1 };
    pub(crate) const YN: Delta = Delta { dx: 0, dy: -1 };
    #[cfg(test)]
    pub(crate) const AXIAL_UNITS: [Delta; 4] = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];

    pub(crate) fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    pub(crate) fn is_axial(self) -> bool {
        self.dx == 0 || self.dy == 0 && !self.is_zero()
    }

    pub(crate) fn axial_unit(self) -> Delta {
        if self.is_zero() {
            Delta::ZERO
        } else {
            // Fencepost: prefer Y ("column rule"). This shouldn't be relied upon; in general, call
            // this only on axial deltas.
            if !self.is_axial() {
                error!("{:?} is not an axial unit", self);
            }
            if self.dx.abs() > self.dy.abs() {
                Delta {
                    dx: self.dx.signum(),
                    dy: 0,
                }
            } else {
                Delta {
                    dx: 0,
                    dy: self.dy.signum(),
                }
            }
        }
    }

    pub(crate) fn displacement(self) -> usize {
        self.dx.abs() as usize + self.dy.abs() as usize
    }

    #[cfg(test)]
    pub(crate) fn perpendicular(self) -> Delta {
        Delta {
            dx: -self.dy,
            dy: self.dx,
        }
    }
}

impl Particle {
    pub(crate) fn is_vacuum(self) -> bool {
        self == Particle::Vacuum
    }
}

impl Cell {
    pub(crate) fn occludes(&self) -> bool {
        // Vacuum can always be passed through, non-vacuum if passable is set.
        !(self.what.is_vacuum() || self.passable)
    }
}

impl AutomatonDecision {
    pub(crate) fn priority(&self) -> usize {
        use AutomatonDecision::*;
        match self {
            None => 0,
            TowardAttractor { .. } => 10,
            // "Frank correction": this is higher priority
            FromRepulsor { .. } => 20,
            UnbalancedPair { .. } => 30,
        }
    }

    pub(crate) fn delta(&self, axis: Delta) -> Delta {
        use AutomatonDecision::*;
        fn sgn(&b: &bool) -> isize {
            if b { 1 } else { -1 }
        }

        match self {
            UnbalancedPair { pos, .. } | FromRepulsor { pos, .. } | TowardAttractor { pos, .. } => {
                axis * sgn(pos)
            }
            None => Delta::ZERO,
        }
    }
}

impl PartialOrd for AutomatonDecision {
    fn partial_cmp(&self, other: &AutomatonDecision) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for AutomatonDecision {
    fn eq(&self, other: &AutomatonDecision) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for AutomatonDecision {}

#[cfg(test)]
mod tests {
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
            game.board.place(loc + d * 2, Particle::Attractor);
            game.board.place(loc + d * (-2), Particle::Repulsor);
            println!("* empty UnP delta {:?}", d);
            expect_automaton_move(&mut game, d)?;

            let clean_board = game.board.clone();
            let perp = d.perpendicular();

            game.board.place(loc + perp * 2, Particle::Attractor);
            println!("* UnP delta {:?} unaffected by unipolar attractor", d);
            expect_automaton_move(&mut game, d)?;
            game.board.place(loc + perp * (-2), Particle::Attractor);
            println!("* UnP delta {:?} unaffected by bipolar attractor", d);
            expect_automaton_move(&mut game, d)?;

            game.board = clean_board;

            game.board.place(loc + perp * 2, Particle::Repulsor);
            println!("* UnP delta {:?} unaffected by unipolar repulsor", d);
            expect_automaton_move(&mut game, d)?;
            game.board.place(loc + perp * (-2), Particle::Repulsor);
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

            game.board.place(loc + d * 1, Particle::Attractor);
            game.board.place(loc + d * (-2), Particle::Repulsor);
            expect_automaton_move(&mut game, Delta::ZERO)?;
            println!("* no move when adjacent to UnP attractor, delta {:?}", d);

            game.board = clean_board;

            game.board.place(loc + d * 2, Particle::Attractor);
            game.board.place(loc + d * (-1), Particle::Repulsor);
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
                g.board.place(loc + perp * 2, Particle::Attractor);
                println!("* ...with unipolar attractor");
                expect_automaton_move(g, e)?;
                g.board.place(loc + perp * (-2), Particle::Attractor);
                println!("* ...with bipolar attractor");
                expect_automaton_move(g, e)?;
                Ok(())
            };

            let clean_board = game.board.clone();

            game.board.place(loc + d * (-1), Particle::Repulsor);
            println!("* away from adjacent repulsor, unipolar, delta {:?}", d);
            expect_automaton_move(&mut game, d)?;
            try_with_attractors(&mut game, d)?;

            game.board = clean_board.clone();
            game.board.place(loc + d * (-2), Particle::Repulsor);
            println!("* away from far repulsor, unipolar, delta {:?}", d);
            expect_automaton_move(&mut game, d)?;
            try_with_attractors(&mut game, d)?;

            game.board = clean_board.clone();
            game.board.place(loc + d * (-1), Particle::Repulsor);
            game.board.place(loc + d * 2, Particle::Repulsor);
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
        assert!(ready_after_2, "Should be ready after both players submit");

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
}

impl core::fmt::Debug for Board {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{}x{} board with automaton at {}",
            self.size.x, self.size.y, self.automaton_location
        )
    }
}

impl Ord for AutomatonDecision {
    /// Which of these two automaton decisions is more urgent?
    fn cmp(&self, other: &AutomatonDecision) -> Ordering {
        use AutomatonDecision::*;

        // Yo, what's up with all these .reverse() calls?
        //
        // Well, the README.md describes the rules in a particular way, and to
        // make the code easy to verify, the code is written that way too.

        self.priority()
            .cmp(&other.priority())
            .then_with(|| match (self, other) {
                // same priority means same enum variant!
                (
                    UnbalancedPair {
                        att_dist, rep_dist, ..
                    },
                    UnbalancedPair {
                        att_dist: o_att_dist,
                        rep_dist: o_rep_dist,
                        ..
                    },
                ) => att_dist
                    .cmp(o_att_dist)
                    .reverse()
                    .then(rep_dist.cmp(o_rep_dist).reverse()),
                (
                    FromRepulsor { rep_dist, .. },
                    FromRepulsor {
                        rep_dist: o_rep_dist,
                        ..
                    },
                ) => rep_dist.cmp(o_rep_dist).reverse(),
                (
                    TowardAttractor { att_dist, .. },
                    TowardAttractor {
                        att_dist: o_att_dist,
                        ..
                    },
                ) => att_dist.cmp(o_att_dist).reverse(),
                (None, None) => Ordering::Equal,
                _ => unreachable!(),
            })
    }
}

impl From<Particle> for Option<String> {
    fn from(particle: Particle) -> Self {
        match particle {
            Particle::Repulsor => Some("repulsor".to_string()),
            Particle::Attractor => Some("attractor".to_string()),
            Particle::Automaton => Some("automaton".to_string()),
            Particle::Vacuum => None,
        }
    }
}

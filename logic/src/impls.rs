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
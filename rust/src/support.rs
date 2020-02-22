use crate::*;
use smallvec::SmallVec;
/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pid(pub u8);

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CoordsFeedback {
    pub data: SmallVec<[(Coord, CoordFeedback); 2]>,
}

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {}", coord, feedback)?
        }
        Ok(())
    }
}

/// Coordinate on the board. TODO: microbenchmark different coord sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Delta {
    dx: i8,
    dy: i8,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub who: Pid,
    pub from: Coord,
    pub to: Coord,
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

#[derive(Debug, Clone)]
pub(crate) struct Raycast {
    pub(crate) what: Particle,
    pub(crate) hit: Option<Coord>,
    pub(crate) dist: usize,
}

// Program invariant: by the time a coord ever hits a Board method (besides
// inbounds), it's inbounds.

impl Board {
    /// Standard board layout for a two player game.
    pub fn stock_two_player() -> Board {
        let o = Cell {
            what: Particle::Vacuum,
            conflict: false,
            passable: false,
        };
        let r = Cell {
            what: Particle::Repulsor,
            ..o
        };
        let a = Cell {
            what: Particle::Attractor,
            ..o
        };
        let d = Cell {
            what: Particle::Automaton,
            ..o
        };
        Board {
            particles: arr2(&[
                [r, r, o, o, r, r, r, o, o, r, r],
                [o, o, o, a, r, r, r, a, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [a, a, o, o, o, o, o, o, o, a, a],
                [r, r, o, o, o, d, o, o, o, r, r],
                [a, a, o, o, o, o, o, o, o, a, a],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, a, r, r, r, a, o, o, o],
                [r, r, o, o, r, r, r, o, o, r, r],
            ]),
            size: Coord { x: 11, y: 11 },
            automaton_location: Coord { x: 5, y: 5 },
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        }
    }

    /// 5x5 board with an automaton and some particles.
    pub fn stock_testing() -> Board {
        let o = Cell {
            what: Particle::Vacuum,
            conflict: false,
            passable: false,
        };
        let r = Cell {
            what: Particle::Repulsor,
            ..o
        };
        let a = Cell {
            what: Particle::Attractor,
            ..o
        };
        let d = Cell {
            what: Particle::Automaton,
            ..o
        };
        Board {
            particles: arr2(&[
                [r, o, a, o, r],
                [o, o, o, o, o],
                [r, o, d, o, r],
                [o, o, o, o, o],
                [r, o, a, o, r],
            ]),
            size: Coord { x: 5, y: 5 },
            automaton_location: Coord { x: 2, y: 2 },
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        }
    }

    /// Empty 5x5 board containing a lonely automaton.
    pub fn stock_testing_empty() -> Board {
        let o = Cell {
            what: Particle::Vacuum,
            conflict: false,
            passable: false,
        };
        let d = Cell {
            what: Particle::Automaton,
            ..o
        };
        Board {
            particles: arr2(&[
                [o, o, o, o, o],
                [o, o, o, o, o],
                [o, o, d, o, o],
                [o, o, o, o, o],
                [o, o, o, o, o],
            ]),
            size: Coord { x: 5, y: 5 },
            automaton_location: Coord { x: 2, y: 2 },
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        }
    }

    /// Place a particle on the board.
    ///
    /// If the particle isn't the automaton, this increases the matter on the
    /// board. If it is the automaton, it is forcibly moved from wherever else it
    /// is on the board, leaving a vacuum in its place. (This restriction might
    /// later be lifted to allow more than one automaton on the board, but this
    /// method is unlikely to abide such a change.)
    pub fn place(&mut self, c: Coord, w: Particle) {
        if w == Particle::Automaton {
            self.particles[self.automaton_location.ix()].what = Particle::Vacuum;
            self.automaton_location = c;
        }
        self.particles[c.ix()].what = w;
    }

    /// Mark a coordinate as passable, because some move specifies it as a source.
    pub(crate) fn mark_passable(&mut self, c: Coord) {
        self.particles[c.ix()].passable = true;
        self.passable_list.push(c);
    }

    /// Forcibly swap two positions on the board.
    ///
    /// Conserves total particle counts. This can also do weird, probably
    /// illogical things, like swapping conflict flags. This method also does
    /// absolutely no bounds checking, and thus can panic if the coordinate is
    /// out of bounds.
    pub(crate) fn force_move(&mut self, from: Coord, to: Coord) {
        self.particles.swap(from.ix(), to.ix());

        if self.automaton_location == from {
            debug_assert_eq!(self.particles[to.ix()].what, Particle::Automaton);
            self.automaton_location = to;
        }
    }

    /// Raycast on the board down an axis from a coordinate.
    ///
    /// The ray starts from, but does not include, the "from" coordinate.
    ///
    /// The axis SHOULD be a unit vector, but any nonzero Delta is acceptable. This method only
    /// tests the integer multiples of that offset, and relies on the ray eventually containing
    /// out-of-bounds points to terminate.
    ///
    /// The raycast's dist field is set to the integer at which iteration terminated. If iteration
    /// terminated in-bounds, this is guaranteed to be on a non-Vacuum particle. If it terminated
    /// out-of-bounds, i is the first integer multiple of axis that is out of bounds (and the
    /// particle is Vacuum). (These facts are depended upon in the automaton's reasoning; see
    /// evaluate_axis.)
    #[instrument]
    pub(crate) fn raycast(&self, from: Coord, axis: Delta) -> Raycast {
        debug_assert_ne!(axis, Delta::ZERO);

        for i in 1isize.. {
            let co = from + axis * i;
            if !self.inbounds(co) {
                return Raycast {
                    what: Particle::Vacuum,
                    hit: None,
                    dist: i as usize,
                };
            }
            let c = self.particles[co.ix()];
            /* NB: The following condition could also be `c.occludes()`, but this is almost
             * certainly being called within Board::automaton_move, and by this time the marks
             * should be clear anyway.
             */
            if !c.what.is_vacuum() {
                return Raycast {
                    what: c.what,
                    hit: Some(co),
                    dist: i as usize,
                };
            }
        }

        unreachable!()
    }

    /// Mark a cell as conflicted.
    ///
    /// Moves specified by plebeians may not specify conflicted squares as a rule (see
    /// MoveError::Conflicted, CoordFeedback::Conflict).
    ///
    /// Conflicted cells come about during conflict resolution (RoundState::ResolvingConflict) to
    /// indicate that two plebeians attempted to move the same particle differently, or move
    /// different particles to the same cell. When conflict resolution ends, the marks are cleared.
    pub(crate) fn mark_conflict(&mut self, c: Coord) {
        self.particles[c.ix()].conflict = true;
        self.conflict_list.push(c);
    }

    /// Clear all conflict/passable marks.
    ///
    /// This is done at the end of conflict resolution (RoundState::ResolvingConflict).
    pub(crate) fn clear_marks(&mut self) {
        for c in self.conflict_list.drain(..) {
            self.particles[c.ix()].conflict = false;
        }
        for c in self.passable_list.drain(..) {
            self.particles[c.ix()].passable = false;
        }
    }

    /// Test if there is a conflict in the addressed cell.
    ///
    /// If this is true, the cell may not be specified as a source or destination of any move
    /// (MoveError::Conflicted).
    pub(crate) fn is_conflict(&self, c: Coord) -> bool {
        self.particles[c.ix()].conflict
    }

    /// Test whether the addressed cell is vacuum.
    pub(crate) fn is_vacuum(&self, c: Coord) -> bool {
        self.particles[c.ix()].what.is_vacuum()
    }

    /// Test whether the addressed cell is the automaton.
    pub(crate) fn is_automaton(&self, c: Coord) -> bool {
        self.automaton_location == c
    }

    /// Test whether the coordinate is within the boundaries of the board.
    ///
    /// It is illegal to specify an out-of-bounds coordinate as the source or destination of a move
    /// (MoveError::Oob).
    pub(crate) fn inbounds(&self, c: Coord) -> bool {
        c.x < self.size.x && c.y < self.size.y
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
            if b {
                1
            } else {
                -1
            }
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
        write!(f, "{}x{} board with automaton at {}", self.size.x, self.size.y, self.automaton_location)
    }
}
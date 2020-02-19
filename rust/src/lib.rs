//! Reference implementation of the automatafl board game.
//!
//! General crate design notes:
//!
//! - Max board size is "small" (256x256) but easy to bump (Coord).
//! - Max player count is "small" (256) but easy to bump (Pid).
//! - `SmallVec` is used to size everything to require zero allocations
//!   during a standard four-goal, two-player game.
//! - Every error condition is uniquely identified and with
//!   nice Display implementations.
//! - Lots of state is public. If you EVER MUTATE ANYTHING, the game rules
//!   might break or the code might panic! Only calling methods will avoid this.
//!   Inspect state away :)

extern crate displaydoc;
extern crate ndarray;
extern crate smallvec;

use displaydoc::Display;
use ndarray::{arr2, Array2 as Grid};
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::iter::FromIterator;

/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pid(pub u8);

/// Coordinate on the board. TODO: microbenchmark different coord sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
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
    fn ix(self) -> (usize, usize) {
        (self.x as usize, self.y as usize)
    }
}

impl Delta {
    const ZERO: Delta = Delta { dx: 0, dy: 0 };
    const XP: Delta = Delta { dx: 1, dy: 0 };
    const XN: Delta = Delta { dx: -1, dy: 0 };
    const YP: Delta = Delta { dx: 0, dy: 1 };
    const YN: Delta = Delta { dx: 0, dy: -1 };

    fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    fn is_axial(self) -> bool {
        self.dx == 0 || self.dy == 0 && !self.is_zero()
    }

    fn axial_unit(self) -> Delta {
        if self.is_zero() {
            Delta::ZERO
        } else {
            // Fencepost: prefer Y ("column rule"). This shouldn't be relied upon; in general, call
            // this only on axial deltas.
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

    fn displacement(self) -> usize {
        self.dx.abs() as usize + self.dy.abs() as usize
    }
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// "x, y {}"
#[derive(Debug, Display, PartialEq, Eq, Clone)]
pub enum CoordFeedback {
    /// is OK
    Ok,
    /// is conflicted
    Conflict,
    /// is not on the board
    Oob,
    /// is the automaton, which is off-limits
    Automaton,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CoordsFeedback {
    data: SmallVec<[(Coord, CoordFeedback); 2]>,
}

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {}", coord, feedback)?
        }
        Ok(())
    }
}

/// "Your move {}."
#[derive(Debug, Display, Clone, PartialEq, Eq)]
pub enum MoveFeedback {
    /// is now pending waiting for the other player
    Committed,
    /// had some problems: {0}
    SeeCoords(CoordsFeedback),
    /// must have different source and destination squares
    MustMove,
    /// must move the piece only along a row or column (like a chess Rook)
    AxisAlignedOnly,
    /// cannot be performed while other players are resolving conflicts
    WaitYourTurn,
    /// doesn't matter once the game is over
    GameOver,
}

/// Game status:
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum RoundState {
    /// not yet started
    Fresh,
    /// has players waiting
    PartiallySubmitted,
    /// is resolving conflicts
    ResolvingConflict,
    /// is over
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub who: Pid,
    pub from: Coord,
    pub to: Coord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

impl Particle {
    fn is_vacuum(self) -> bool {
        self == Particle::Vacuum
    }
}

// TODO: this is 3 bytes when it could be 1 :/
#[derive(Copy, Clone, Debug)]
pub struct Cell {
    pub what: Particle,
    pub conflict: bool,
    pub passable: bool,
}

impl Cell {
    fn is_vacuum(&self) -> bool {
        self.what.is_vacuum()
    }
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            what: Particle::Vacuum,
            conflict: false,
            passable: false,
        }
    }
}

/// Player 0 move: {}
#[derive(Display, PartialEq, Eq)]
pub enum MoveResult {
    /// failed because there was never a piece to move at the source.
    NoSource,
    /// failed the move is occluded between source and destination by a piece at {0}.
    OccupiedAt(Coord),
    /// applied!
    Applied,
}

#[derive(Debug, Clone)]
struct Raycast {
    what: Particle,
    hit: Option<Coord>,
    dist: usize,
}

#[derive(Debug)]
pub struct Board {
    particles: Grid<Cell>,
    size: Coord,
    automaton_location: Coord,
    conflict_list: SmallVec<[Coord; 16]>, // TODO: compare performance scanning this list to scanning the whole grid
    passable_list: SmallVec<[Coord; 16]>,
}

// By the time a coord ever hits a Board method (besides inbounds), it's inbounds.

impl Board {
    /// Prepare a standard board layout for a two player game.
    pub fn stock_two_player() -> Board {
        let r = Cell {
            what: Particle::Repulsor,
            ..Default::default()
        };
        let a = Cell {
            what: Particle::Attractor,
            ..Default::default()
        };
        let o = Cell {
            what: Particle::Vacuum,
            ..Default::default()
        };
        let d = Cell {
            what: Particle::Automaton,
            ..Default::default()
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

    /// Mark a coordinate as passable, because some move specifies it as a source.
    fn mark_passable(&mut self, c: Coord) {
        self.particles[c.ix()].passable = true;
        self.passable_list.push(c);
    }

    /// Attempt to move a piece. This can fail, and no move is attempted in that
    /// case.
    ///
    /// This function considers it allowable to move the automaton, and is part
    /// of the call graph of Game::update_automaton.
    fn do_move(&mut self, from: Coord, to: Coord) -> MoveResult {
        use MoveResult::*;

        debug_assert!(self.inbounds(from) && self.inbounds(to));

        let delta = to - from;
        debug_assert!(!delta.is_zero()); // established by propose_move
        debug_assert!(delta.is_axial());

        let src = self.particles[from.ix()];
        let dst = self.particles[to.ix()];

        debug_assert!(!src.is_vacuum());

        debug_assert!(!src.conflict && !dst.conflict);

        let axis = delta.axial_unit();
        for offset in 1..=delta.displacement() {
            let c = from + axis * offset as isize;
            if !self.particles[c.ix()].is_vacuum() {
                return OccupiedAt(c);
            }
        }

        self.force_move(from, to);

        Applied
    }

    /// Forcibly swap two positions on the board (assuring the number of particles is constant).
    /// This can also do weird, probably illogical things, like swapping conflict flags. This
    /// function also does absolutely no bounds checking, and thus can panic if the coordinate is
    /// out of bounds. Only use this if you know what you're doing.
    fn force_move(&mut self, from: Coord, to: Coord) {
        self.particles.swap(from.ix(), to.ix());

        if self.automaton_location == from {
            self.automaton_location = to;
        }
    }

    /// Raycast on the board down an axis from a coordinate.
    ///
    /// The ray starts from, but does not include, the "from" coordinate.
    ///
    /// The axis SHOULD be a unit vector, but any nonzero Delta is acceptable. This function only
    /// tests the integer multiples of that offset, and relies on the ray eventually containing
    /// out-of-bounds points to terminate.
    ///
    /// The raycast's dist field is set to the integer at which iteration terminated. If iteration
    /// terminated in-bounds, this is guaranteed to be on a non-Vacuum particle. If it terminated
    /// out-of-bounds, i is the first integer multiple of axis that is out of bounds (and the
    /// particle is Vacuum). (These facts are depended upon in the automaton's reasoning; see
    /// evaluate_axis.)
    fn raycast(&self, from: Coord, axis: Delta) -> Raycast {
        debug_assert!(axis != Delta::ZERO);

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
            if !c.is_vacuum() {
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
    fn mark_conflict(&mut self, c: Coord) {
        self.particles[c.ix()].conflict = true;
        self.conflict_list.push(c);
    }

    /// Clear all marked cells.
    ///
    /// This is done at the end of conflict resolution (RoundState::ResolvingConflict).
    fn clear_marks(&mut self) {
        for c in self.conflict_list.drain(..) {
            self.particles[c.ix()].conflict = false;
        }
        for c in self.passable_list.drain(..) {
            self.particles[c.ix()].passable = false;
        }
    }

    /// Test if there is a conflict in the given cell.
    ///
    /// If this is true, the cell may not be specified as a source or destination of any move
    /// (MoveError::Conflicted).
    fn is_conflict(&self, c: Coord) -> bool {
        self.particles[c.ix()].conflict
    }

    /// Test whether the cell is empty.
    ///
    /// A source cell must be nonempty (MoveError::NoSource), and all other cells included on the
    /// movement to the destination cell must be empty or passable (MoveError::OccupiedAt).
    fn is_vacuum(&self, c: Coord) -> bool {
        self.particles[c.ix()].is_vacuum()
    }

    /// Test whether the cell contains the automaton.
    fn is_automaton(&self, c: Coord) -> bool {
        self.automaton_location == c
    }

    /// Test whether the coordinate is within the boundaries of the board.
    ///
    /// It is illegal to specify an out-of-bounds coordinate as the source or destination of a move
    /// (MoveError::Oob).
    fn inbounds(&self, c: Coord) -> bool {
        c.x < self.size.x && c.y < self.size.y
    }
}

/// Decisions of the Automaton on one axis.
#[derive(Debug, Clone)]
enum AutomatonDecision {
    UnbalancedPair {
        pos: bool,
        att_dist: usize,
        rep_dist: usize,
    },
    FromRepulsor {
        pos: bool,
        rep_dist: usize,
    },
    TowardAttractor {
        pos: bool,
        att_dist: usize,
    },
    None,
}

impl AutomatonDecision {
    fn priority(&self) -> usize {
        use AutomatonDecision::*;
        match self {
            None => 0,
            TowardAttractor { .. } => 10,
            // "Frank correction": this is higher priority
            FromRepulsor { .. } => 20,
            UnbalancedPair { .. } => 30,
        }
    }

    fn delta(&self, axis: Delta) -> Delta {
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

impl Ord for AutomatonDecision {
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
                (None, _) => Ordering::Equal,
                _ => unreachable!(),
            })
    }
}

#[derive(Debug)]
pub struct Game {
    pub winner: Option<Pid>,
    pub locked_players: SmallVec<[Pid; 2]>,
    pub board: Board,
    pub round: RoundState,
    pub pending_moves: SmallVec<[Move; 2]>,
    pub goals: SmallVec<[(Coord, Pid); 4]>,
    pub player_count: u8,
    pub use_column_rule: bool,
}

impl Game {
    pub fn new(board: Board, player_count: u8, use_column_rule: bool) -> Game {
        Game {
            winner: None,
            locked_players: SmallVec::new(),
            board,
            round: RoundState::Fresh,
            pending_moves: SmallVec::new(),
            goals: SmallVec::new(),
            player_count,
            use_column_rule,
        }
    }

    /// Propose a move, returning some feedback about it, and true if the state
    /// machine is ready to advance (try_complete_round preconditions are met).
    ///
    /// Returns false if try_complete_round would panic.
    pub fn propose_move(&mut self, m: Move) -> (MoveFeedback, bool) {
        use MoveFeedback::*;

        let mut cfs = CoordsFeedback {
            data: SmallVec::new(),
        };

        // rules for a single coord, returns false if we shouldn't continue
        fn consider(cfs: &mut CoordsFeedback, b: &Board, c: Coord) -> bool {
            use CoordFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_automaton(c) {
                Automaton
            } else if b.is_conflict(c) {
                Conflict
            } else {
                Ok
            };
            let res = feedback == Ok;
            cfs.data.push((c, feedback));
            res
        }

        let res = if self.round == RoundState::GameOver {
            GameOver
        } else if self.locked_players.contains(&m.who) {
            WaitYourTurn //                XXX XXX XXX  ~~(v)~~ XXX XXX XXX
        } else if !consider(&mut cfs, &self.board, m.from) | !consider(&mut cfs, &self.board, m.to)
        {
            //      load bearing non-short-circuiting  ~~~(^)~~~ to accumulate both coord results!
            SeeCoords(cfs)
        } else if m.from == m.to {
            MustMove
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            AxisAlignedOnly
        } else {
            Committed
        };

        if res == Committed {
            self.pending_moves.push(m);
        }

        (res, self.pending_moves.len() == self.player_count as usize)
    }

    /// Returns Ok with the list of applied move to apply, or else the list of
    /// conflicting moves.
    fn resolve_conflicts(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Move; 2]>> {
        debug_assert!(self.pending_moves.len() == self.player_count as usize);

        let mut seen_pairs = SmallVec::<[(Coord, Coord); 2]>::new();

        let mut seen_from = SmallVec::<[Coord; 2]>::new();
        let mut seen_to = SmallVec::<[Coord; 2]>::new();

        let mut conflict_moves = SmallVec::<[Move; 2]>::new();
        let mut locked_moves = SmallVec::<[Move; 2]>::new();

        for &m in &self.pending_moves {
            let mut conflict = false;
            let this_pair = (m.from, m.to);
            if seen_pairs.contains(&this_pair) {
                // multiple players specifying the same move is OK!
                continue;
            }
            seen_pairs.push(this_pair);

            // See if there's a source conflict...
            if seen_from.contains(&m.from) {
                conflict_moves.push(m);
                self.board.mark_conflict(m.from);
                conflict = true;
            } else {
                seen_from.push(m.from);
            }

            // Or a dest conflict...
            if seen_to.contains(&m.to) {
                conflict_moves.push(m);
                self.board.mark_conflict(m.to);
                conflict = true;
            } else {
                seen_to.push(m.to);
            }

            if conflict {
                conflict_moves.push(m);
                // We conflicted with some previous move, pull them out of the
                // locked list and into the conflict list.
                locked_moves = SmallVec::from_iter(locked_moves.into_iter().filter_map(|p| {
                    if p.from == m.from || p.to == m.to {
                        conflict_moves.push(p);
                        None
                    } else {
                        Some(p)
                    }
                }));
            } else {
                // We'll consider this move locked unless someone else conflicts with it.
                locked_moves.push(m);
            }
        }

        if conflict_moves.len() == 0 {
            Ok(locked_moves)
        } else {
            Err(conflict_moves)
        }
    }

    /// Return the list of move results if everything was gucci, else enter conflict resolution.
    pub fn try_complete_round(&mut self) -> Result<SmallVec<[(Move, MoveResult); 2]>, ()> {
        match self.resolve_conflicts() {
            Ok(mut moves_to_apply) => {
                // Lift the moved pieces off the board

                for m in &moves_to_apply {
                    self.board.mark_passable(m.from);
                }

                let mut results = SmallVec::with_capacity(moves_to_apply.len());

                while moves_to_apply.len() != 0 {
                    let mut made_progress = false;
                    // FIXME: does this terminate? how does the python even work? it appears to
                    // depend critically on passable not mucking with the particle type
                    moves_to_apply.retain(|m| {
                        if self.board.is_vacuum(m.from) {
                            true
                        } else {
                            results.push((*m, self.board.do_move(m.from, m.to)));
                            made_progress = true;
                            false
                        }
                    });

                    if !made_progress {
                        for m in moves_to_apply.drain(..) {
                            results.push((m, MoveResult::NoSource))
                        }
                    }
                }

                self.update_automaton();

                let &mut Game {
                    ref mut goals,
                    ref mut board,
                    ref mut round,
                    ref mut winner,
                    ..
                } = self;

                if !goals.iter().any(|&(c, who)| {
                    if c == board.automaton_location {
                        *round = RoundState::GameOver;
                        *winner = Some(who);
                        true
                    } else {
                        false
                    }
                }) {
                    self.board.clear_marks();
                    *round = RoundState::Fresh;
                }

                Ok(results)
            }
            Err(moves_conflicted) => {
                self.round = RoundState::ResolvingConflict;
                self.pending_moves
                    .retain(|e| !moves_conflicted.contains(&e));
                for m in &moves_conflicted {
                    if !self.locked_players.contains(&m.who) {
                        self.locked_players.push(m.who);
                    }
                }
                Err(())
            }
        }
    }

    /// Calculate the coordinate to which the automaton would move right now.
    fn automaton_move(&self) -> Coord {
        fn evaluate_axis(pos: &Raycast, neg: &Raycast) -> AutomatonDecision {
            use AutomatonDecision::*;
            use Particle::*;

            match (pos.what, neg.what) {
                (Attractor, Repulsor) if pos.dist > 1 => UnbalancedPair {
                    pos: true,
                    att_dist: pos.dist,
                    rep_dist: neg.dist,
                },
                (Repulsor, Attractor) if neg.dist > 1 => UnbalancedPair {
                    pos: false,
                    att_dist: neg.dist,
                    rep_dist: pos.dist,
                },
                (Repulsor, Repulsor) if pos.dist != neg.dist => FromRepulsor {
                    pos: pos.dist > neg.dist,
                    rep_dist: std::cmp::min(pos.dist, neg.dist),
                },
                (Repulsor, Vacuum) if neg.dist > 1 => FromRepulsor {
                    pos: false,
                    rep_dist: pos.dist,
                },
                (Vacuum, Repulsor) if pos.dist > 1 => FromRepulsor {
                    pos: true,
                    rep_dist: neg.dist,
                },
                (Attractor, Attractor) if pos.dist != neg.dist => TowardAttractor {
                    pos: pos.dist < neg.dist,
                    att_dist: std::cmp::min(pos.dist, neg.dist),
                },
                (Attractor, Vacuum) if pos.dist > 1 => TowardAttractor {
                    pos: true,
                    att_dist: pos.dist,
                },
                (Vacuum, Attractor) if neg.dist > 1 => TowardAttractor {
                    pos: false,
                    att_dist: neg.dist,
                },
                _ => None,
            }
        }

        let xp = self.board.raycast(self.board.automaton_location, Delta::XP);
        let xn = self.board.raycast(self.board.automaton_location, Delta::XN);
        let yp = self.board.raycast(self.board.automaton_location, Delta::YP);
        let yn = self.board.raycast(self.board.automaton_location, Delta::YN);

        let x_decision = evaluate_axis(&xp, &xn);
        let y_decision = evaluate_axis(&yp, &yn);

        self.board.automaton_location
            + if x_decision > y_decision {
                x_decision.delta(Delta::XP)
            } else {
                if !self.use_column_rule && x_decision == y_decision {
                    Delta::ZERO
                } else {
                    y_decision.delta(Delta::YP)
                }
            }
    }

    /// Cause the automaton to move.
    fn update_automaton(&mut self) {
        let new_location = self.automaton_move();
        if new_location != self.board.automaton_location {
            debug_assert!(
                self.board
                    .do_move(self.board.automaton_location, new_location)
                    == MoveResult::Applied
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    #[test]
    fn automaton_stays_put() {
        let board = Board::stock_two_player();
        let mut game = Game::new(board, 2, true);
        let before_move = game.board.automaton_location;
        game.update_automaton();
        let after_move = game.board.automaton_location;
        assert_eq!(before_move, after_move)
    }
}

use crate::*;
use serde::{Deserialize, Serialize};

use ndarray::Array2 as Grid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub particles: Grid<Cell>,
    pub size: Coord,
    pub automaton_location: Coord,
    pub conflict_list: SmallVec<[Coord; 16]>, // TODO: compare performance scanning this list to scanning the whole grid
    pub passable_list: SmallVec<[Coord; 16]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub what: Particle,
    pub conflict: bool,
    pub passable: bool,
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

    /// Empty 6x6 board containing a lonely automaton.
    pub fn stock_testing_empty_6() -> Board {
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
                [o, o, o, o, o, o],
                [o, o, o, o, o, o],
                [o, o, d, o, o, o],
                [o, o, o, o, o, o],
                [o, o, o, o, o, o],
                [o, o, o, o, o, o],
            ]),
            size: Coord { x: 6, y: 6 },
            automaton_location: Coord { x: 2, y: 2 },
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        }
    }


    /// Empty 6x6 board containing a lonely automaton.
   pub fn stock_testing_empty_7() -> Board {
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
                [o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o],
                [o, o, o, d, o, o, o],
                [o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o],
            ]),
            size: Coord { x: 7, y: 7 },
            automaton_location: Coord { x: 3, y: 3 },
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
            let co = from + axis.scale(i);
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

    /// Attempt to move a non-vacuum piece. This can fail, and no move is attempted in that case.
    ///
    /// This method considers it allowable to move the automaton, and is part of the call graph
    /// of Game::update_automaton.
    pub(crate) fn do_move(&mut self, from: Coord, to: Coord) -> MoveResult {
        use MoveResult::*;

        // debug_assert checks invariants that should be established by propose_move
        debug_assert!(self.inbounds(from) && self.inbounds(to));

        let delta = to - from;
        debug_assert!(!delta.is_zero());
        debug_assert!(delta.is_axial());

        let src = self.particles[from.ix()];
        let dst = self.particles[to.ix()];

        debug_assert!(!src.what.is_vacuum());

        debug_assert!(!src.conflict && !dst.conflict);

        let axis = delta.axial_unit();
        for offset in 1..=delta.displacement() {
            let c = from + axis.scale(offset as isize);
            if self.particles[c.ix()].occludes() {
                return OccupiedAt(c);
            }
        }

        self.force_move(from, to);

        Applied
    }
}

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
//!

extern crate displaydoc;
extern crate ndarray;
extern crate smallvec;

use displaydoc::Display;
use ndarray::{arr2, Array2 as Grid};
use smallvec::SmallVec;

/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Pid(u8);

/// Coordinate on the board. TODO: microbenchmark different coord sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Coord {
    x: u8,
    y: u8,
}

impl Coord {
    fn ix(self) -> (usize, usize) {
        (self.x as usize, self.y as usize)
    }
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}, {}", self.x, self.y)
    }
}

/// "x, y {}"
#[derive(Debug, Display)]
enum CoordFeedback {
    /// is OK
    Ok,
    /// is conflicted
    Conflict,
    /// is not on the board
    Oob,
    /// is the automaton, which is off-limits
    Automaton,
}

struct CoordsFeedback {
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
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash)]
enum MoveFeedback {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
enum RoundState {
    Fresh,
    PartiallySubmitted,
    ResolvingConflict,
    GameOver,
}

#[derive(Debug, Clone, Copy)]
struct Move {
    who: Pid,
    from: Coord,
    to: Coord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

impl Particle {
    fn is_vacuum(self) {
        self == Vacuum
    }
}

// TODO: this is 2 bytes when it could be 1 :/
#[derive(Copy, Clone)]
struct Cell {
    what: Particle,
    conflict: bool,
}

struct Board {
    particles: Grid<Cell>,
    size: Coord,
    automaton_location: Coord,
    conflict_list: SmallVec<[Coord; 16]>, // TODO: compare performance scanning this list to scanning the whole grid
}

// By the time a coord ever hits a Board method (besides inbounds), it's inbounds.

impl Board {
    fn stock_two_player() -> Board {
        let r = Cell {
            what: Particle::Repulsor,
            conflict: false,
        };
        let a = Cell {
            what: Particle::Attractor,
            conflict: false,
        };
        let o = Cell {
            what: Particle::Vacuum,
            conflict: false,
        };
        let a = Cell {
            what: Particle::Automaton,
            conflict: false,
        };
        let mut board = Board {
            particles: arr2(&[
                [r, r, o, o, r, r, r, o, o, r, r],
                [o, o, o, a, r, r, r, a, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [a, a, o, o, o, o, o, o, o, a, a],
                [r, r, o, o, o, a, o, o, o, r, r],
                [a, a, o, o, o, o, o, o, o, a, a],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, o, o, o, o, o, o, o, o],
                [o, o, o, a, r, r, r, a, o, o, o],
                [r, r, o, o, r, r, r, o, o, r, r],
            ]),
            size: Coord { x: 11, y: 11 },
            automaton_location: Coord { x: 5, y: 5 },
            conflict_list: SmallVec::new(),
        };

        board
    }

    /// Lift any particle off the board, putting vacuum in its place.
    ///
    /// Panics if coord is conflicted.
    fn pluck(&mut self, c: Coord) -> Particle {
        let mut replacement = Cell {
            what: Particle::Vacuum,
            conflict: false,
        };
        core::mem::swap(&mut replacement, &mut self.board.particles[c.ix()]);
        debug_assert!(replacement.conflict == false);
        replacement.what
    }

    fn do_move(&mut self, from: Coord, to: Coord, what: Particle) {
        // scan the axis to make sure it's passable...
        //
        // if it is, plop it down in to. otherwise, put what back down.
    }

    fn mark_conflict(&mut self, c: Coord) {
        self.particles[c.ix()].conflict = true;
        self.conflict_list.push(c);
    }

    fn clear_conflicts(&mut self) {
        for c in self.conflict_list.drain() {
            self.particles[c.ix()].conflict = false;
        }
    }

    fn is_conflict(&self, c: Coord) -> bool {
        self.particles[c.ix()].conflict
    }

    fn is_vacuum(&self, c: Coord) -> bool {
        self.particles[c.ix()].is_vacuum()
    }

    fn is_automaton(&self, c: Coord) -> bool {
        self.automaton_location == c
    }

    fn inbounds(&self, c: Coord) -> bool {
        c.x < size.x && c.y < size.y
    }
}

struct Game {
    winner: Option<Pid>,
    locked_players: SmallVec<[Pid; 2]>,
    board: Board,
    round: RoundState,
    pending_moves: SmallVec<[Move; 2]>,
    goals: SmallVec<[(Coord, Pid); 4]>,
    waiting_players: u8,
    player_count: u8,
}

impl Game {
    pub fn propose_move(&mut self, m: Move) -> MoveFeedback {
        use MoveFeedback::*;

        let mut cfs = CoordsFeedback {
            data: SmallVec::new(),
        };

        // rules for a single coord, returns false if we shouldn't continue
        fn consider_coord(cfs: &mut CoordsFeedback, b: &Board, c: Coord) -> bool {
            use CoordsFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_automaton(c) {
                Automaton
            } else if b.is_conflict(c) {
                Conflict
            } else {
                Ok
            };
            cfs.data.push((c, feedback));
            feedback == Ok
        }

        if self.round == RoundState::GameOver {
            return GameOver;
        } else if self.locked_players.contains(m.who) {
            return WaitYourTurn; //   XXX XXX XXX  ~~(v)~~ XXX XXX XXX
        } else if !consider(cfs, &self.board, m.from) | !consider(cfs, &self.board, m.to) {
            // load bearing non-short-circuiting  ~~~(^)~~~ to accumulate both coord results!
            return SeeCoords(cfs);
        } else if m.from == m.to {
            return MustMove;
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            return AxisAlignedOnly;
        } else {
            if self.pending_moves.insert(m.who.0 as usize, m).is_none() {
                self.waiting_players -= 1
            }

            Confirmed
        }
    }

    /// Returns Ok with the list of applied move to apply, or else the list of
    /// conflicting moves.
    fn resolve_conflicts(&self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Move; 2]>> {
        debug_assert!(self.pending_moves.len() == self.player_count);

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
                conflict = true;
            } else {
                seen_from.push(m.from);
            }

            // Or a dest conflict...
            if seen_to.contains(&m.to) {
                conflict_moves.push(m);
                conflict = true;
            } else {
                seen_to.push(m.to);
            }

            if conflict {
                conflict_moves.push(m);
                // We conflicted with some previous move, pull them out of the
                // locked list and into the conflict list.
                prev_moves = SmallVec::from_iter(prev_moves.into_iter().filter_map(|p| {
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

    fn try_complete_round(&mut self) {
        match self.resolve_conflicts() {
            Ok(moves_to_apply) => {
                // Lift the moved pieces off the board

                let mut moved = moves_to_apply
                    .iter()
                    .map(|m| (m, self.board.pluck(m.from)))
                    .collect::<SmallVec<[_; 2]>>();

                let mut results = vec![];

                while moved.len() != 0 {
                    // FIXME: does this terminate? how does the python even work? it appears to
                    // depend critically on passable not mucking with the particle type
                    moved.retain(|(m, particle)| {
                        if self.board.is_vacuum(m.from) || !self.board.is_vacuum(m.to) {
                            true
                        } else {
                            results.push(m, self.board.do_move(m.from, m.to, particle));
                            false
                        }
                    });
                }

                self.update_automaton();

                if !self.goals.iter().any(|(c, who)| {
                    if c == self.board.automaton_location {
                        self.round = RoundState::GameOver;
                        self.winner = Some(who);
                        true
                    } else {
                        false
                    }
                }) {
                    self.round == RoundState::Fresh;
                }
            }
            Err(moves_conflicted) => {
                self.round = RoundState::ResolvingConflict;
                self.pending_moves.retain(|e| !moves_conflicted.contains(e));
                for m in &moves_conflicted {
                    if !self.locked_players.contains(m.who) {
                        self.locked_players.push(m.who);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

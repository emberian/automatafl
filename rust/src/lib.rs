#![allow(unused_doc_comments)]

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

mod support;

pub use support::*;

use spandoc::spandoc;
use tracing::{error, info, instrument, trace};

use displaydoc::Display;
use ndarray::{arr2, Array2 as Grid};
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::iter::FromIterator;

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
pub enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

// TODO: this is 3 bytes when it could be 1 :/
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Cell {
    pub what: Particle,
    pub conflict: bool,
    pub passable: bool,
}

/// Player 0 move: {}
#[derive(Debug, Display, PartialEq, Eq)]
pub enum MoveResult {
    /// failed because there was never a piece to move at the source.
    NoSource,
    /// failed the move is occluded between source and destination by a piece at {0}.
    OccupiedAt(Coord),
    /// applied!
    Applied,
}

#[derive(Clone, PartialEq)]
pub struct Board {
    pub particles: Grid<Cell>,
    pub size: Coord,
    pub automaton_location: Coord,
    pub conflict_list: SmallVec<[Coord; 16]>, // TODO: compare performance scanning this list to scanning the whole grid
    pub passable_list: SmallVec<[Coord; 16]>,
}

impl Board {
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
            let c = from + axis * offset as isize;
            if self.particles[c.ix()].occludes() {
                return OccupiedAt(c);
            }
        }

        self.force_move(from, to);

        Applied
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

#[derive(Debug, Clone, PartialEq)]
pub struct Game {
    pub winner: Option<Pid>,
    /// Players who cannot submit moves during RoundState::ResolvingConflict
    pub locked_players: SmallVec<[Pid; 2]>,
    pub board: Board,
    pub round: RoundState,
    /// Submitted moves so far
    pub pending_moves: SmallVec<[Move; 2]>,
    /// Goal locations, when the automaton enters one of these that player wins.
    pub goals: SmallVec<[(Coord, Pid); 4]>,
    pub player_count: u8,
    pub use_column_rule: bool,
}

impl Game {
    /// Create a new game using the given board.
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
    #[instrument]
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
    #[spandoc]
    #[instrument]
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
                trace!("marking source conflict on {}", coord = m.from);
                conflict_moves.push(m);
                self.board.mark_conflict(m.from);
                conflict = true;
            } else {
                seen_from.push(m.from);
            }

            // Or a dest conflict...
            if seen_to.contains(&m.to) {
                trace!("marking dest conflict on {}", coord = m.to);
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
                        trace!("caused conflict with player {:?}", pid = p.who);
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
    #[instrument]
    #[spandoc]
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

                match self
                    .goals
                    .iter()
                    .copied()
                    .find(|(c, _)| c == &self.board.automaton_location)
                {
                    Some((_, who)) => {
                        self.round = RoundState::GameOver;
                        self.winner = Some(who);
                    }
                    None => {
                        self.board.clear_marks();
                        self.round = RoundState::Fresh;
                    }
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

    /// Update the automaton, returning true if it moved
    pub fn update_automaton(&mut self) {
        let new_location = self.automaton_move();
        if new_location != self.board.automaton_location {
            debug_assert_eq!(
                self.board
                    .do_move(self.board.automaton_location, new_location),
                MoveResult::Applied
            );
        }
    }

    /// Calculate the coordinate to which the automaton would move right now.
    #[spandoc]
    #[instrument]
    fn automaton_move(&self) -> Coord {
        #[instrument]
        fn evaluate_axis(pos: &Raycast, neg: &Raycast) -> AutomatonDecision {
            use AutomatonDecision::*;
            use Particle::{Attractor as A, Repulsor as R, Vacuum as V};

            match (pos.what, neg.what) {
                (A, R) if pos.dist > 1 => UnbalancedPair {
                    pos: true,
                    att_dist: pos.dist,
                    rep_dist: neg.dist,
                },
                (R, A) if neg.dist > 1 => UnbalancedPair {
                    pos: false,
                    att_dist: neg.dist,
                    rep_dist: pos.dist,
                },
                (R, R) if pos.dist != neg.dist => FromRepulsor {
                    pos: pos.dist > neg.dist,
                    rep_dist: std::cmp::min(pos.dist, neg.dist),
                },
                (R, V) if neg.dist > 1 => FromRepulsor {
                    pos: false,
                    rep_dist: pos.dist,
                },
                (V, R) if pos.dist > 1 => FromRepulsor {
                    pos: true,
                    rep_dist: neg.dist,
                },
                (A, A) if pos.dist != neg.dist => TowardAttractor {
                    pos: pos.dist < neg.dist,
                    att_dist: std::cmp::min(pos.dist, neg.dist),
                },
                (A, V) if pos.dist > 1 => TowardAttractor {
                    pos: true,
                    att_dist: pos.dist,
                },
                (V, A) if neg.dist > 1 => TowardAttractor {
                    pos: false,
                    att_dist: neg.dist,
                },
                _ => None,
            }
        }

        /// Find the nearest particles in the four directions.
        let xp = self.board.raycast(self.board.automaton_location, Delta::XP);
        let xn = self.board.raycast(self.board.automaton_location, Delta::XN);
        let yp = self.board.raycast(self.board.automaton_location, Delta::YP);
        let yn = self.board.raycast(self.board.automaton_location, Delta::YN);

        let x_decision = evaluate_axis(&xp, &xn);
        let y_decision = evaluate_axis(&yp, &yn);

        let offset = if x_decision > y_decision {
            x_decision.delta(Delta::XP)
        } else {
            // If the options are equally preferable, don't move unless we're using the column rule.
            if !self.use_column_rule && x_decision == y_decision {
                info!("avoided applying the column rule");
                Delta::ZERO
            } else {
                y_decision.delta(Delta::YP)
            }
        };

        self.board.automaton_location + offset
    }
}

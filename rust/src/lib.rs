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
extern crate smallvec;

use displaydoc::Display;
use smallvec::SmallVec;

use std::collections::HashSet;

/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Pid(u8);

/// Coordinate on the board. TODO: microbenchmark different coord sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Coord {
    x: u8,
    y: u8,
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}, {}", self.x, self.y)
    }
}

#[derive(Debug, Display)]
enum CoordFeedback {
    /// is OK
    Ok,
    /// is conflicted
    Conflict,
    /// is not on the board
    Oob,
    /// is the agent, which is off-limits
    Agent,
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

// "Your move {}."
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Hash)]
enum MoveFeedback {
    /// is now pending waiting for the other player
    Committed,
    /// had some problems: {0}
    SeeCoords(CoordsFeedback),
    /// must have different source and destination squares
    MustMove,
    /// must specify a piece to move
    EmptySource,
    /// must move the piece only along a row or column (like a chess Rook)
    AxisAlignedOnly,
    /// cannot be performed while other players are resolving conflicts
    WaitYourTurn,
    /// doesn't matter once the game is over
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
enum Piece {
    None,
    Black,
    White,
    Agent,
}

#[derive(Debug, Clone)]
struct Cell {
    piece: Piece,
    conflict: bool,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell {
            piece: Piece::None,
            conflict: false,
        }
    }
}

struct Board {
    cells: Vec<Vec<Cell>>,
    size: Coord,
    agent_cache: Option<Coord>,
    conflict_cache: HashSet<Coord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PlacementStatus {
    Ok,
    Oob,
    ExistingPiece(Piece),
    AgentAlreadyAt(Coord),
}

impl Board {
    fn with_size(size: Coord) -> Board {
        Board {
            cells: std::iter::repeat_with(|| {
                std::iter::repeat_with(Default::default)
                    .take(size.y as usize)
                    .collect()
            })
            .take(size.x as usize)
            .collect(),
            size: size,
            agent_cache: None,
            conflict_cache: HashSet::new(),
        }
    }

    fn cell(&self, coord: Coord) -> Option<&Cell> {
        if !self.inbounds(coord) {
            None
        } else {
            Some(&self.cells[coord.x as usize][coord.y as usize])
        }
    }

    fn cell_mut(&mut self, coord: Coord) -> Option<&mut Cell> {
        if !self.inbounds(coord) {
            None
        } else {
            Some(&mut self.cells[coord.x as usize][coord.y as usize])
        }
    }

    fn place_piece(&mut self, coord: Coord, piece: Piece) -> PlacementStatus {
        if !self.inbounds(coord) {
            return PlacementStatus::Oob;
        }

        let cell = self.cell_mut(coord).unwrap();
        if piece != Piece::None && cell.piece != Piece::None {
            return PlacementStatus::ExistingPiece(cell.piece);
        }

        if piece == Piece::Agent {
            if let Some(c) = self.agent_cache {
                return PlacementStatus::AgentAlreadyAt(c);
            }
        }

        // Actually commit to placing this piece

        if piece == Piece::None && cell.piece == Piece::Agent {
            self.agent_cache = None;
        }

        cell.piece = piece;

        if piece == Piece::Agent {
            self.agent_cache = Some(coord);
        }

        PlacementStatus::Ok
    }

    fn mark_conflict(&mut self, c: Coord) {
        self.cell_mut(c).map(|cell| {
            cell.conflict = true;
            self.conflict_cache.insert(c);
        });
    }

    fn clear_conflicts(&mut self) {
        for coord in self.conflict_cache.drain() {
            self.cell_mut(coord).unwrap().conflict = false;
        }
    }

    fn is_conflict(&self, c: Coord) -> bool {
        self.cell(c).map_or(false, |cell| cell.conflict)
    }

    fn is_agent(&self, c: Coord) -> bool {
        self.agent_cache.map_or(false, |co| co == c)
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
        fn consider(cfs: &mut CoordsFeedback, b: &Board, c: Coord) -> bool {
            use CoordsFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_agent(c) {
                Agent
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
            return WaitYourTurn;
        } else if !consider(cfs, &self.board, m.from) | !consider(cfs, &self.board, m.to) {
            // note load bearing non-short-circuiting | to accumulate both coord results!
            return SeeCoords(cfs);
        } else if self.board.is_empty(m.from) {
            return EmptySource;
        } else if m.from == m.to {
            return MustMove;
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            return AxisAlignedOnly;
        } else {
            if self
                .latest_confirmed_moves
                .insert(m.who.0 as usize, m)
                .is_none()
            {
                self.waiting_players -= 1
            }

            Confirmed
        }
    }

    /// Tries to complete a round,
    ///
    /// Returns Ok with the list of applied moves, or else the list of newly
    /// conflicted coordinates.
    fn try_complete_round(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Coord; 2]>> {
        debug_assert!(self.pending_moves.len() == self.player_count);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

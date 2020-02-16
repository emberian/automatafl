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

//! Player ID within a single game
struct Pid(u8);

/// Coordinate on the board. TODO: microbenchmark different coord sizes
struct Coord {
    x: u8,
    y: u8,
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}, {}", self.x, self.y)
    }
}

#[derive(Display)]
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
#[derive(Display)]
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

enum RoundState {
    Fresh,
    PartiallySubmitted,
    ResolvingConflict,
    GameOver,
}

struct Move {
    who: Pid,
    from: Coord,
    to: Coord,
}

struct Board {}

impl Board {
    fn mark_conflict(&mut self, c: Coord) {}

    fn clear_conflicts(&mut self) {}

    fn is_conflict(&self, c: Coord) -> bool {}

    fn is_agent(&self, c: Coord) -> bool {}

    fn inbounds(&self, c: Coord) -> bool {}
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

        if self.round == RoundState::GameOver {
            return GameOver;
        }

        if self.locked_players.contains(m.who) {
            return WaitYourTurn;
        }

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

        // note load bearing non-short-circuiting | to accumulate both coord results!

        if !consider(cfs, &self.board, m.from) | !consider(cfs, &self.board, m.to) {
            return SeeCoords(cfs);
        }

        if self.board.is_empty(m.from) {
            return EmptySource;
        }

        if m.from == m.to {
            return MustMove;
        }

        if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            return AxisAlignedOnly;
        }

        if self.latest_confirmed_moves.insert(m.who.0 as usize, m).is_none() {
            self.waiting_players -= 1
        }

        Confirmed
    }

    /// Tries to complete a round, 
    /// 
    /// Returns Ok with the list of applied moves, or else the list of newly
    /// conflicted coordinates.
    fn try_complete_round(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Coord; 2]>> {
        debug_assert!(self.pending_moves.len() == self.player_count)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}

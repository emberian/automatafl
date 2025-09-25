use crate::*;

use serde::{Serialize, Deserialize};
use displaydoc::Display;
use smallvec::SmallVec;

/// "x, y {}"
#[derive(Debug, Display, PartialEq, Eq, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Display, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, Serialize, Deserialize)]
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

/// Player 0 move: {}
#[derive(Debug, Display, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveResult {
    /// failed because there was never a piece to move at the source.
    NoSource,
    /// failed the move is occluded between source and destination by a piece at {0}.
    OccupiedAt(Coord),
    /// applied!
    Applied,
}

/// Decisions of the Automaton on one axis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AutomatonDecision {
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

/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pid(pub u8);

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct CoordsFeedback {
    pub data: SmallVec<[(Coord, CoordFeedback); 2]>,
}

/// Coordinate on the board. TODO: microbenchmark different coord sizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coord {
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Delta {
    pub dx: i8,
    pub dy: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub who: Pid,
    pub from: Coord,
    pub to: Coord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Raycast {
    pub(crate) what: Particle,
    pub(crate) hit: Option<Coord>,
    pub(crate) dist: usize,
}

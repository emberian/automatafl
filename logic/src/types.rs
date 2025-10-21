use crate::*;

use displaydoc::Display;
use serde::{Deserialize, Serialize};
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

/// "Your move {}."
#[derive(Debug, Display, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposeFeedback {
    /// is now pending waiting for the other player
    Accepted,
    /// is now pending and all players have submitted moves
    AcceptedAndReady,
    /// is rejected: {0}, try again
    Rejected(MoveFeedback),
}

/// Game round {}
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

/// Player move {}
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveResult {
    /// failed because there was never a piece to move at the source.
    NoSource,
    /// failed because the move is occluded between source and destination by a piece at {0}.
    OccupiedAt(Coord),
    /// applied!
    Applied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictStatus {
    pub conflicted_moves: SmallVec<[Move; 2]>,
    pub locked_players: SmallVec<[Pid; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompleteRoundFeedback {
    CompletedMoves(SmallVec<[(Move, MoveResult); 2]>),
    Conflict(ConflictStatus),
    WaitingForPlayers(usize),
}

/// How to handle merging pathways (multiple chains converging on same destination)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeResolutionMode {
    /// Detect merging pathways in conflict phase and reject them
    /// Extends the weakest precondition to treat merges as conflicts
    DetectAndConflict,

    /// Allow merges, but annihilate all pieces that converge
    /// (Useful for tactical "denial" plays: "If I can't have it, nobody can")
    Annihilate,

    /// Pieces stop one square BEFORE the merge point if it would cause collision
    /// Merge point M remains empty when multiple chains converge on it
    BunchBeforeMerge,

    /// Pieces "stack up" along chains, stopping when they hit another piece
    /// (@Grissess): Process in reverse path order - each piece moves as far as it can
    /// First piece along longest path gets M, others compress behind
    BunchedStacking,
}

/// How pieces behave when moves form cycles of length >2
/// Note: 2-cycles always stay in place (unambiguous), 1-cycles forbidden by propose_move
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleBehaviorMode {
    /// Pieces advance one position around cycles of length >2
    /// (@ember): "Every edge fired once"
    RotatePieces,

    /// All moves succeed but pieces remain in place even for >2-cycles
    /// Useful for maintaining board state during automaton step
    NoMovement,
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
    pub dx: i16,
    pub dy: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Move {
    pub who: Pid,
    pub from: Coord,
    pub to: Coord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Raycast {
    pub what: Particle,
    pub hit: Option<Coord>,
    pub dist: usize,
}

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
            dx: (self.x as i16 - other.x as i16),
            dy: (self.y as i16 - other.y as i16),
        }
    }
}

impl std::ops::Add<Delta> for Coord {
    type Output = Coord;

    fn add(self, other: Delta) -> Coord {
        Coord {
            x: (self.x as i16 + other.dx) as u8,
            y: (self.y as i16 + other.dy) as u8,
        }
    }
}

impl Coord {
    pub fn ix(self) -> (usize, usize) {
        (self.y as usize, self.x as usize)
    }

    pub fn to_key(self, width: u8) -> usize {
        self.y as usize * width as usize + self.x as usize
    }
}

impl Delta {
    pub const ZERO: Delta = Delta { dx: 0, dy: 0 };
    pub const XP: Delta = Delta { dx: 1, dy: 0 };
    pub const XN: Delta = Delta { dx: -1, dy: 0 };
    pub const YP: Delta = Delta { dx: 0, dy: 1 };
    pub const YN: Delta = Delta { dx: 0, dy: -1 };
    pub const AXIAL_UNITS: [Delta; 4] = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];

    pub fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    pub fn is_axial(self) -> bool {
        self.dx == 0 || self.dy == 0 && !self.is_zero()
    }

    pub fn axial_unit(self) -> Delta {
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

    pub fn displacement(self) -> usize {
        self.dx.abs() as usize + self.dy.abs() as usize
    }

    pub fn scale(self, factor: isize) -> Delta {
        Delta {
            dx: self.dx * (factor as i16),
            dy: self.dy * (factor as i16),
        }
    }

    pub fn perpendicular(self) -> Delta {
        Delta {
            dx: -self.dy,
            dy: self.dx,
        }
    }
}

impl Particle {
    pub fn is_vacuum(self) -> bool {
        self == Particle::Vacuum
    }
}

impl Cell {
    pub fn occludes(&self) -> bool {
        // Vacuum can always be passed through, non-vacuum if passable is set.
        !(self.what.is_vacuum() || self.passable)
    }
}

impl core::fmt::Debug for Board {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        // --- Header ---
        writeln!(
            f,
            "Board ({}x{}) with automaton at {}",
            self.size.x, self.size.y, self.automaton_location
        )?;

        write!(f, "  ┌")?;
        for _ in 0..self.size.x {
            write!(f, "───")?;
        }
        writeln!(f, "┐")?;

        // --- Board Rows (from top to bottom) ---
        // Y-axis is printed from highest to lowest to match typical top-left origin consoles
        for y in (0..self.size.y).rev() {
            write!(f, "{:2}│", y)?;

            for x in 0..self.size.x {
                let cell = self.particles[Coord { x, y }.ix()];

                let conflict_char = if cell.conflict { '!' } else { ' ' };
                let passable_char = if cell.passable { '~' } else { ' ' };

                let particle_char = match cell.what {
                    Particle::Repulsor => 'R',
                    Particle::Attractor => 'A',
                    Particle::Automaton => 'D', // 'D' for Daemon/Automaton
                    Particle::Vacuum => '.',
                };

                // Format is: [Conflict Char][Particle Char][Passable Char]
                // e.g., " R ", "!A ", " .~"
                write!(f, "{}{}{}", conflict_char, particle_char, passable_char)?;
            }
            writeln!(f, "│")?;
        }

        write!(f, "  └")?;
        for _ in 0..self.size.x {
            write!(f, "───")?;
        }
        writeln!(f, "┘")?;
        write!(f, "    ")?; // Padding for row headers

        for x in 0..self.size.x {
            write!(f, "{:<3}", x)?;
        }
        writeln!(f)?;

        Ok(())
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

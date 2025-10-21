#![cfg_attr(not(feature = "std"), no_std)]
#![allow(unused_doc_comments)]

//! Zero-allocation bitboard implementation of the automatafl board game.
//!
//! Optimized for AlphaZero-style MCTS with millions of playouts:
//! - Flat POD state: bitboards + automaton coord, Clone is memcpy
//! - Zero heap allocation: ArrayVec everywhere, stack scratch buffers
//! - Transposed bitboards: O(1) axial raycasts in any direction
//! - Branchless path checks: bit-twiddling instead of loops
//! - Make/Undo: apply move + push Diff, undo via bit flips (no Clone)
//! - Cache-friendly: ~700 bytes for board state, all sequential access
//!
//! Design notes:
//! - Board size: const W/H (currently 11×11, easily changed)
//! - Max player count: 256 (ArrayVec capacity)
//! - no_std compatible for WASM/GPU/embedded
//! - SIMD-friendly layout (u128 bitboards = 4×u32 lanes)

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use alloc::string::{String, ToString};
use core::cmp::Ordering;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// arrayvec is no_std compatible
use arrayvec::ArrayVec;

// tracing is optional, disable for no_std minimal builds
#[cfg(feature = "tracing")]
use tracing::{error, info, instrument};

#[cfg(not(feature = "tracing"))]
macro_rules! instrument {
    (#[cfg($meta:meta)] $item:item) => { #[cfg($meta)] $item };
    ($item:item) => { $item };
}

#[cfg(not(feature = "tracing"))]
macro_rules! error { ($($t:tt)*) => { let _ = ($($t)*); }; }
#[cfg(not(feature = "tracing"))]
macro_rules! info { ($($t:tt)*) => { let _ = ($($t)*); }; }
#[cfg(not(feature = "tracing"))]
macro_rules! trace { ($($t:tt)*) => { let _ = ($($t)*); }; }

// ============================================================================
// CONSTANTS & CORE TYPES
// ============================================================================

/// Board width (columns). Change this for different game sizes.
pub const W: usize = 11;

/// Board height (rows). Change this for different game sizes.
pub const H: usize = 11;

/// Maximum number of players supported (ArrayVec capacity)
const MAX_PLAYERS: usize = 4;

/// Bitboard row: lower W bits represent cell occupancy
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct BitRow(u128);

/// Bitboard column: lower H bits represent cell occupancy
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct BitCol(u128);

impl BitRow {
    const EMPTY: BitRow = BitRow(0);
}

impl BitCol {
    const EMPTY: BitCol = BitCol(0);
}

// ============================================================================
// BIT MANIPULATION HELPERS
// ============================================================================

/// Returns bit mask for position x (or y)
#[inline(always)]
const fn bit(x: u8) -> u128 {
    1u128 << x
}

/// Check if bit x is set in row
#[inline(always)]
fn row_has(b: &BitRow, x: u8) -> bool {
    (b.0 & bit(x)) != 0
}

/// Check if bit y is set in column
#[inline(always)]
fn col_has(b: &BitCol, y: u8) -> bool {
    (b.0 & bit(y)) != 0
}

/// Set or clear bit x in row (branchless)
#[inline(always)]
fn set_row(b: &mut BitRow, x: u8, v: bool) {
    let mask = bit(x);
    // Branchless: if v=false, -(0 as i128)=0, so 0u128 & mask = 0
    //             if v=true,  -(1 as i128)=-1, so !0u128 & mask = mask
    b.0 = (b.0 & !mask) | ((-(v as i128) as u128) & mask);
}

/// Set or clear bit y in column (branchless)
#[inline(always)]
fn set_col(b: &mut BitCol, y: u8, v: bool) {
    let mask = bit(y);
    // Branchless: same technique as set_row
    b.0 = (b.0 & !mask) | ((-(v as i128) as u128) & mask);
}

/// Mask of bits strictly between lo and hi (exclusive both ends)
/// Works for both x and y coordinates
#[inline(always)]
fn between_mask(lo: u8, hi: u8) -> u128 {
    debug_assert!(lo < hi);
    let lo_mask = bit(lo + 1).wrapping_sub(1);  // 0000...0111 (lo)
    let hi_mask = bit(hi).wrapping_sub(1);       // 0000...0111 (hi-1)
    hi_mask ^ lo_mask
}

/// Check if path is clear (no bits set between endpoints)
#[inline(always)]
fn path_clear_bits(occ: u128, a: u8, b: u8) -> bool {
    if a == b {
        return true;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (occ & between_mask(lo, hi)) == 0
}

/// Linearize coord to index for bitset arrays
#[inline(always)]
fn idx(c: Coord) -> usize {
    (c.y as usize) * W + (c.x as usize)
}

// ============================================================================
// BIT SCANS FOR AUTOMATON RAYCAST
// ============================================================================

/// Find next set bit to the right (higher index), None if no bits set
#[inline(always)]
fn next_set_bit_right(occ: u128, x: u8) -> Option<u8> {
    // Mask out everything at x and to the left
    let mask = !(bit(x + 1).wrapping_sub(1));
    let m = occ & mask;
    if m == 0 {
        None
    } else {
        Some(m.trailing_zeros() as u8)
    }
}

/// Find next set bit to the left (lower index), None if no bits set
#[inline(always)]
fn next_set_bit_left(occ: u128, x: u8) -> Option<u8> {
    // Mask out everything at x and to the right
    let mask = bit(x).wrapping_sub(1);
    let m = occ & mask;
    if m == 0 {
        None
    } else {
        Some(127 - m.leading_zeros() as u8)
    }
}

// ============================================================================
// TYPES
// ============================================================================

/// Coordinate on the board
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Coord {
    pub x: u8,
    pub y: u8,
}

impl Coord {
    pub fn ix(self) -> (usize, usize) {
        (self.x as usize, self.y as usize)
    }
}

/// Delta between two coordinates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Delta {
    pub dx: i8,
    pub dy: i8,
}

impl Delta {
    pub const ZERO: Delta = Delta { dx: 0, dy: 0 };
    pub const XP: Delta = Delta { dx: 1, dy: 0 };
    pub const XN: Delta = Delta { dx: -1, dy: 0 };
    pub const YP: Delta = Delta { dx: 0, dy: 1 };
    pub const YN: Delta = Delta { dx: 0, dy: -1 };

    #[cfg(test)]
    pub const AXIAL_UNITS: [Delta; 4] = [Delta::XP, Delta::XN, Delta::YP, Delta::YN];

    pub fn is_zero(self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    pub fn is_axial(self) -> bool {
        (self.dx == 0 || self.dy == 0) && !self.is_zero()
    }

    pub fn axial_unit(self) -> Delta {
        if self.is_zero() {
            Delta::ZERO
        } else {
            if !self.is_axial() {
                error!("{:?} is not an axial delta", self);
            }
            if self.dx.abs() > self.dy.abs() {
                Delta { dx: self.dx.signum(), dy: 0 }
            } else {
                Delta { dx: 0, dy: self.dy.signum() }
            }
        }
    }

    pub fn displacement(self) -> usize {
        self.dx.abs() as usize + self.dy.abs() as usize
    }

    #[cfg(test)]
    pub fn perpendicular(self) -> Delta {
        Delta { dx: -self.dy, dy: self.dx }
    }
}

/// Particle types on the board
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

impl Particle {
    pub fn is_vacuum(self) -> bool {
        self == Particle::Vacuum
    }
}

/// Player ID within a single game
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Pid(pub u8);

/// Move submitted by a player
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Move {
    pub who: Pid,
    pub from: Coord,
    pub to: Coord,
}

/// Feedback for coordinate validation
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CoordFeedback {
    Ok,
    Conflict,
    Oob,
    Automaton,
}

/// Feedback for multiple coordinates
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CoordsFeedback {
    pub data: ArrayVec<(Coord, CoordFeedback), 2>,
}

/// Feedback for move proposal
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MoveFeedback {
    Committed,
    SeeCoords(CoordsFeedback),
    MustMove,
    AxisAlignedOnly,
    WaitYourTurn,
    GameOver,
}

/// Result of applying a move
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MoveResult {
    NoSource,
    OccupiedAt(Coord),
    Applied,
}

/// Round state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RoundState {
    Fresh,
    PartiallySubmitted,
    ResolvingConflict,
    GameOver,
}

/// Automaton decision on one axis
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AutomatonDecision {
    UnbalancedPair { pos: bool, att_dist: usize, rep_dist: usize },
    FromRepulsor { pos: bool, rep_dist: usize },
    TowardAttractor { pos: bool, att_dist: usize },
    None,
}

impl AutomatonDecision {
    pub fn priority(&self) -> usize {
        match self {
            AutomatonDecision::None => 0,
            AutomatonDecision::TowardAttractor { .. } => 10,
            AutomatonDecision::FromRepulsor { .. } => 20,
            AutomatonDecision::UnbalancedPair { .. } => 30,
        }
    }

    pub fn delta(&self, axis: Delta) -> Delta {
        fn sgn(&b: &bool) -> isize {
            if b { 1 } else { -1 }
        }
        match self {
            AutomatonDecision::UnbalancedPair { pos, .. }
            | AutomatonDecision::FromRepulsor { pos, .. }
            | AutomatonDecision::TowardAttractor { pos, .. } => axis * sgn(pos),
            AutomatonDecision::None => Delta::ZERO,
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
        self.priority()
            .cmp(&other.priority())
            .then_with(|| match (self, other) {
                (
                    UnbalancedPair { att_dist, rep_dist, .. },
                    UnbalancedPair { att_dist: o_att_dist, rep_dist: o_rep_dist, .. },
                ) => att_dist.cmp(o_att_dist).reverse().then(rep_dist.cmp(o_rep_dist).reverse()),
                (FromRepulsor { rep_dist, .. }, FromRepulsor { rep_dist: o_rep_dist, .. }) => {
                    rep_dist.cmp(o_rep_dist).reverse()
                }
                (
                    TowardAttractor { att_dist, .. },
                    TowardAttractor { att_dist: o_att_dist, .. },
                ) => att_dist.cmp(o_att_dist).reverse(),
                (None, None) => Ordering::Equal,
                _ => unreachable!(),
            })
    }
}

// ============================================================================
// BOARD STATE (BITBOARDS + AUTOMATON)
// ============================================================================

/// Bitboard representation of the game board
///
/// Uses transposed storage: both row[y] and col[x] bitboards maintained
/// for O(1) axial raycasts in any direction. Updates flip bits in both.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BoardBits {
    /// Repulsor positions by row (rep_row[y] has bits set for each repulsor in row y)
    rep_row: [BitRow; H],
    /// Attractor positions by row
    att_row: [BitRow; H],
    /// Repulsor positions by column (rep_col[x] has bits set for each repulsor in col x)
    rep_col: [BitCol; W],
    /// Attractor positions by column
    att_col: [BitCol; W],
    /// Automaton location (only one automaton)
    pub automaton_location: Coord,
    /// Board size (for inbounds checks, always W×H currently)
    pub size: Coord,
}

impl BoardBits {
    /// Create empty board
    pub fn empty() -> Self {
        BoardBits {
            rep_row: [BitRow::EMPTY; H],
            att_row: [BitRow::EMPTY; H],
            rep_col: [BitCol::EMPTY; W],
            att_col: [BitCol::EMPTY; W],
            automaton_location: Coord { x: W as u8 / 2, y: H as u8 / 2 },
            size: Coord { x: W as u8, y: H as u8 },
        }
    }

    /// Standard board layout for a two player game (11×11 Hnefetafl-style)
    pub fn stock_two_player() -> Self {
        let mut board = Self::empty();
        board.automaton_location = Coord { x: 5, y: 5 };

        // Row 0: r r _ _ r r r _ _ r r
        board.place(Coord { x: 0, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 1, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 4, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 5, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 6, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 9, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 10, y: 0 }, Particle::Repulsor);

        // Row 1: _ _ _ a r r r a _ _ _
        board.place(Coord { x: 3, y: 1 }, Particle::Attractor);
        board.place(Coord { x: 4, y: 1 }, Particle::Repulsor);
        board.place(Coord { x: 5, y: 1 }, Particle::Repulsor);
        board.place(Coord { x: 6, y: 1 }, Particle::Repulsor);
        board.place(Coord { x: 7, y: 1 }, Particle::Attractor);

        // Row 4: a a _ _ _ _ _ _ _ a a
        board.place(Coord { x: 0, y: 4 }, Particle::Attractor);
        board.place(Coord { x: 1, y: 4 }, Particle::Attractor);
        board.place(Coord { x: 9, y: 4 }, Particle::Attractor);
        board.place(Coord { x: 10, y: 4 }, Particle::Attractor);

        // Row 5: r r _ _ _ D _ _ _ r r (D = automaton)
        board.place(Coord { x: 0, y: 5 }, Particle::Repulsor);
        board.place(Coord { x: 1, y: 5 }, Particle::Repulsor);
        board.place(Coord { x: 5, y: 5 }, Particle::Automaton);
        board.place(Coord { x: 9, y: 5 }, Particle::Repulsor);
        board.place(Coord { x: 10, y: 5 }, Particle::Repulsor);

        // Row 6: a a _ _ _ _ _ _ _ a a
        board.place(Coord { x: 0, y: 6 }, Particle::Attractor);
        board.place(Coord { x: 1, y: 6 }, Particle::Attractor);
        board.place(Coord { x: 9, y: 6 }, Particle::Attractor);
        board.place(Coord { x: 10, y: 6 }, Particle::Attractor);

        // Row 9: _ _ _ a r r r a _ _ _
        board.place(Coord { x: 3, y: 9 }, Particle::Attractor);
        board.place(Coord { x: 4, y: 9 }, Particle::Repulsor);
        board.place(Coord { x: 5, y: 9 }, Particle::Repulsor);
        board.place(Coord { x: 6, y: 9 }, Particle::Repulsor);
        board.place(Coord { x: 7, y: 9 }, Particle::Attractor);

        // Row 10: r r _ _ r r r _ _ r r
        board.place(Coord { x: 0, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 1, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 4, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 5, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 6, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 9, y: 10 }, Particle::Repulsor);
        board.place(Coord { x: 10, y: 10 }, Particle::Repulsor);

        board
    }

    /// 5×5 board for testing
    pub fn stock_testing() -> Self {
        debug_assert!(W >= 5 && H >= 5);
        let mut board = Self::empty();
        board.automaton_location = Coord { x: 2, y: 2 };

        // Row 0: r _ a _ r
        board.place(Coord { x: 0, y: 0 }, Particle::Repulsor);
        board.place(Coord { x: 2, y: 0 }, Particle::Attractor);
        board.place(Coord { x: 4, y: 0 }, Particle::Repulsor);

        // Row 2: r _ D _ r
        board.place(Coord { x: 0, y: 2 }, Particle::Repulsor);
        board.place(Coord { x: 2, y: 2 }, Particle::Automaton);
        board.place(Coord { x: 4, y: 2 }, Particle::Repulsor);

        // Row 4: r _ a _ r
        board.place(Coord { x: 0, y: 4 }, Particle::Repulsor);
        board.place(Coord { x: 2, y: 4 }, Particle::Attractor);
        board.place(Coord { x: 4, y: 4 }, Particle::Repulsor);

        board
    }

    /// Empty 5×5 board with lonely automaton
    pub fn stock_testing_empty() -> Self {
        debug_assert!(W >= 5 && H >= 5);
        let mut board = Self::empty();
        board.automaton_location = Coord { x: 2, y: 2 };
        board.place(Coord { x: 2, y: 2 }, Particle::Automaton);
        board
    }

    /// Check if coordinate is within board bounds
    #[inline]
    pub fn inbounds(&self, c: Coord) -> bool {
        c.x < self.size.x && c.y < self.size.y
    }

    /// Check if cell is vacuum (no particle)
    #[inline]
    pub fn is_vacuum(&self, c: Coord) -> bool {
        if self.automaton_location == c {
            return false;
        }
        let y = c.y as usize;
        let x = c.x;
        !row_has(&self.rep_row[y], x) && !row_has(&self.att_row[y], x)
    }

    /// Check if cell is the automaton
    #[inline]
    pub fn is_automaton(&self, c: Coord) -> bool {
        self.automaton_location == c
    }

    /// Get particle at coordinate
    pub fn get(&self, c: Coord) -> Particle {
        if self.automaton_location == c {
            return Particle::Automaton;
        }
        let y = c.y as usize;
        let x = c.x;
        if row_has(&self.rep_row[y], x) {
            Particle::Repulsor
        } else if row_has(&self.att_row[y], x) {
            Particle::Attractor
        } else {
            Particle::Vacuum
        }
    }

    /// Place a particle on the board (updates both row and col bitboards)
    pub fn place(&mut self, c: Coord, p: Particle) {
        let x = c.x;
        let y = c.y;
        let xi = x as usize;
        let yi = y as usize;

        // If placing automaton, remove old one
        if p == Particle::Automaton {
            // Clear old automaton position (it becomes vacuum unless something else there)
            // But automaton doesn't set bits, so nothing to clear
            self.automaton_location = c;
            return;
        }

        // Clear existing particle at this location
        set_row(&mut self.rep_row[yi], x, false);
        set_row(&mut self.att_row[yi], x, false);
        set_col(&mut self.rep_col[xi], y, false);
        set_col(&mut self.att_col[xi], y, false);

        // Set new particle
        match p {
            Particle::Repulsor => {
                set_row(&mut self.rep_row[yi], x, true);
                set_col(&mut self.rep_col[xi], y, true);
            }
            Particle::Attractor => {
                set_row(&mut self.att_row[yi], x, true);
                set_col(&mut self.att_col[xi], y, true);
            }
            Particle::Vacuum => {
                // Already cleared
            }
            Particle::Automaton => {
                unreachable!("handled above");
            }
        }
    }

    /// Forcibly swap two positions (conserves particle counts)
    pub fn force_move(&mut self, from: Coord, to: Coord) {
        let from_p = self.get(from);
        let to_p = self.get(to);

        self.place(from, to_p);
        self.place(to, from_p);

        // Handle automaton swap
        if self.automaton_location == from {
            self.automaton_location = to;
        } else if self.automaton_location == to {
            self.automaton_location = from;
        }
    }

    /// Get row occupancy mask (rep | att | aut) for axial movement
    #[inline]
    fn row_occ(&self, y: u8) -> u128 {
        let yi = y as usize;
        let mut occ = self.rep_row[yi].0 | self.att_row[yi].0;
        if self.automaton_location.y == y {
            occ |= bit(self.automaton_location.x);
        }
        occ
    }

    /// Get column occupancy mask (rep | att | aut) for axial movement
    #[inline]
    fn col_occ(&self, x: u8) -> u128 {
        let xi = x as usize;
        let mut occ = self.rep_col[xi].0 | self.att_col[xi].0;
        if self.automaton_location.x == x {
            occ |= bit(self.automaton_location.y);
        }
        occ
    }

    /// Check if axial path is clear (with lifted masks)
    fn path_clear_axial(&self, from: Coord, to: Coord, lifted: &LiftedMask) -> bool {
        if from.y == to.y {
            // Horizontal
            let y = from.y;
            let yi = y as usize;
            let occ = self.row_occ(y) & !(lifted.src_row[yi].0 | lifted.dst_row[yi].0);
            path_clear_bits(occ, from.x, to.x)
        } else {
            // Vertical
            let x = from.x;
            let xi = x as usize;
            let occ = self.col_occ(x) & !(lifted.src_col[xi].0 | lifted.dst_col[xi].0);
            path_clear_bits(occ, from.y, to.y)
        }
    }
}

// ============================================================================
// LIFTED MASKS & MAKE/UNDO
// ============================================================================

/// Ephemeral masks for treating move sources/destinations as passable
/// Computed once per round, never mutates board state
#[derive(Clone)]
struct LiftedMask {
    src_row: [BitRow; H],
    dst_row: [BitRow; H],
    src_col: [BitCol; W],
    dst_col: [BitCol; W],
}

impl LiftedMask {
    fn empty() -> Self {
        LiftedMask {
            src_row: [BitRow::EMPTY; H],
            dst_row: [BitRow::EMPTY; H],
            src_col: [BitCol::EMPTY; W],
            dst_col: [BitCol::EMPTY; W],
        }
    }

    fn from_moves(moves: &[Move]) -> Self {
        let mut mask = Self::empty();
        for &m in moves {
            let fx = m.from.x;
            let fy = m.from.y;
            let tx = m.to.x;
            let ty = m.to.y;

            set_row(&mut mask.src_row[fy as usize], fx, true);
            set_row(&mut mask.dst_row[ty as usize], tx, true);
            set_col(&mut mask.src_col[fx as usize], fy, true);
            set_col(&mut mask.dst_col[tx as usize], ty, true);
        }
        mask
    }
}

/// Minimal undo information for a single move
#[derive(Clone, Copy, Debug)]
struct Diff {
    from: Coord,
    to: Coord,
    was_rep_from: bool,
    was_att_from: bool,
    was_rep_to: bool,
    was_att_to: bool,
    aut_was: Option<Coord>,
}

impl BoardBits {
    /// Apply a move, returning Diff for undo. Checks validity.
    fn apply_move(&mut self, m: Move, lifted: &LiftedMask) -> Result<Diff, MoveResult> {
        let from = m.from;
        let to = m.to;

        // Validity already checked by propose_move, these are debug assertions
        debug_assert!(self.inbounds(from) && self.inbounds(to));
        debug_assert!(from != to);
        debug_assert!(from.x == to.x || from.y == to.y);

        // Check source has particle
        if self.is_vacuum(from) {
            return Err(MoveResult::NoSource);
        }

        // Check path is clear
        if !self.path_clear_axial(from, to, lifted) {
            // Find blocker for error reporting
            let delta = to - from;
            let axis = delta.axial_unit();
            for offset in 1..=delta.displacement() {
                let c = from + axis * (offset as isize);
                let yi = c.y as usize;
                let xi = c.x as usize;
                let occ_here = if from.y == to.y {
                    self.row_occ(c.y) & !(lifted.src_row[yi].0 | lifted.dst_row[yi].0)
                } else {
                    self.col_occ(c.x) & !(lifted.src_col[xi].0 | lifted.dst_col[xi].0)
                };
                let check_bit = if from.y == to.y { c.x } else { c.y };
                if (occ_here & bit(check_bit)) != 0 {
                    return Err(MoveResult::OccupiedAt(c));
                }
            }
            return Err(MoveResult::OccupiedAt(to));
        }

        // Record what's at source and destination for undo
        let was_rep_from = row_has(&self.rep_row[from.y as usize], from.x);
        let was_att_from = row_has(&self.att_row[from.y as usize], from.x);
        let was_rep_to = row_has(&self.rep_row[to.y as usize], to.x);
        let was_att_to = row_has(&self.att_row[to.y as usize], to.x);
        let aut_was = if self.automaton_location == from {
            Some(from)
        } else {
            None
        };

        // Execute swap
        self.force_move(from, to);

        Ok(Diff {
            from,
            to,
            was_rep_from,
            was_att_from,
            was_rep_to,
            was_att_to,
            aut_was,
        })
    }

    /// Undo a move using saved Diff
    fn undo_move(&mut self, d: Diff) {
        // Restore source
        set_row(&mut self.rep_row[d.from.y as usize], d.from.x, d.was_rep_from);
        set_col(&mut self.rep_col[d.from.x as usize], d.from.y, d.was_rep_from);
        set_row(&mut self.att_row[d.from.y as usize], d.from.x, d.was_att_from);
        set_col(&mut self.att_col[d.from.x as usize], d.from.y, d.was_att_from);

        // Restore destination
        set_row(&mut self.rep_row[d.to.y as usize], d.to.x, d.was_rep_to);
        set_col(&mut self.rep_col[d.to.x as usize], d.to.y, d.was_rep_to);
        set_row(&mut self.att_row[d.to.y as usize], d.to.x, d.was_att_to);
        set_col(&mut self.att_col[d.to.x as usize], d.to.y, d.was_att_to);

        // Restore automaton
        if let Some(was) = d.aut_was {
            self.automaton_location = was;
        }
    }
}

// ============================================================================
// RAYCAST FOR AUTOMATON
// ============================================================================

/// Raycast result
struct Raycast {
    what: Particle,
    dist: usize,
}

impl BoardBits {
    /// Raycast along axis from position (automaton movement)
    /// Uses bit scans for O(1) raycast
    fn raycast(&self, from: Coord, axis: Delta) -> Raycast {
        debug_assert!(axis.is_axial() && !axis.is_zero());

        if axis.dy == 0 {
            // Horizontal
            let y = from.y;
            let x = from.x;
            let occ = self.row_occ(y);

            let hit_x = if axis.dx > 0 {
                next_set_bit_right(occ, x)
            } else {
                next_set_bit_left(occ, x)
            };

            if let Some(hx) = hit_x {
                let dist = if hx > x { hx - x } else { x - hx } as usize;
                let hit_coord = Coord { x: hx, y };
                Raycast {
                    what: self.get(hit_coord),
                    dist,
                }
            } else {
                // No hit, measure to edge
                let dist = if axis.dx > 0 {
                    (self.size.x - x) as usize
                } else {
                    (x + 1) as usize
                };
                Raycast {
                    what: Particle::Vacuum,
                    dist,
                }
            }
        } else {
            // Vertical
            let x = from.x;
            let y = from.y;
            let occ = self.col_occ(x);

            let hit_y = if axis.dy > 0 {
                next_set_bit_right(occ, y)
            } else {
                next_set_bit_left(occ, y)
            };

            if let Some(hy) = hit_y {
                let dist = if hy > y { hy - y } else { y - hy } as usize;
                let hit_coord = Coord { x, y: hy };
                Raycast {
                    what: self.get(hit_coord),
                    dist,
                }
            } else {
                // No hit, measure to edge
                let dist = if axis.dy > 0 {
                    (self.size.y - y) as usize
                } else {
                    (y + 1) as usize
                };
                Raycast {
                    what: Particle::Vacuum,
                    dist,
                }
            }
        }
    }
}

// ============================================================================
// GAME STATE & LOGIC
// ============================================================================

/// Reusable scratch space for conflict detection (no allocation)
#[derive(Clone, Debug)]
struct RoundScratch {
    seen_from: [bool; W * H],
    seen_to: [bool; W * H],
    conflicting_from: [bool; W * H],
    conflicting_to: [bool; W * H],
    locked: ArrayVec<Move, MAX_PLAYERS>,
    conflicted: ArrayVec<Move, MAX_PLAYERS>,
}

impl Default for RoundScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl RoundScratch {
    fn new() -> Self {
        RoundScratch {
            seen_from: [false; W * H],
            seen_to: [false; W * H],
            conflicting_from: [false; W * H],
            conflicting_to: [false; W * H],
            locked: ArrayVec::new(),
            conflicted: ArrayVec::new(),
        }
    }

    fn clear(&mut self) {
        self.seen_from.fill(false);
        self.seen_to.fill(false);
        self.conflicting_from.fill(false);
        self.conflicting_to.fill(false);
        self.locked.clear();
        self.conflicted.clear();
    }

    /// O(P) conflict detection using two-pass algorithm
    fn detect_conflicts(
        &mut self,
        pending: &[Move],
    ) -> Result<ArrayVec<Move, MAX_PLAYERS>, ArrayVec<Move, MAX_PLAYERS>> {
        self.clear();

        // Deduplicate identical moves (multiple players can submit the same move)
        let mut seen_pairs = ArrayVec::<(Coord, Coord), MAX_PLAYERS>::new();
        let mut unique_moves = ArrayVec::<Move, MAX_PLAYERS>::new();

        for &m in pending {
            let pair = (m.from, m.to);
            if !seen_pairs.contains(&pair) {
                seen_pairs.push(pair);
                unique_moves.push(m);
            }
        }

        // Pass 1: Identify conflicting coordinates
        // A coordinate is conflicting if it appears in multiple distinct moves
        for &m in &unique_moves {
            let fi = idx(m.from);
            let ti = idx(m.to);

            // If we've seen this 'from' coordinate before, mark it as conflicting
            if core::mem::replace(&mut self.seen_from[fi], true) {
                self.conflicting_from[fi] = true;
            }

            // If we've seen this 'to' coordinate before, mark it as conflicting
            if core::mem::replace(&mut self.seen_to[ti], true) {
                self.conflicting_to[ti] = true;
            }
        }

        // Pass 2: Partition moves based on whether they touch conflicting coordinates
        for &m in &unique_moves {
            let fi = idx(m.from);
            let ti = idx(m.to);

            if self.conflicting_from[fi] || self.conflicting_to[ti] {
                self.conflicted.push(m);
            } else {
                self.locked.push(m);
            }
        }

        if self.conflicted.is_empty() {
            Ok(self.locked.clone())
        } else {
            Err(self.conflicted.clone())
        }
    }
}

/// Main game state
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Game {
    pub winner: Option<Pid>,
    pub locked_players: ArrayVec<Pid, MAX_PLAYERS>,
    pub board: BoardBits,
    pub round: RoundState,
    pub pending_moves: ArrayVec<Move, MAX_PLAYERS>,
    pub goals: ArrayVec<(Coord, Pid), 4>,
    pub player_count: u8,
    pub use_column_rule: bool,

    #[cfg_attr(feature = "serde", serde(skip))]
    scratch: RoundScratch,
}

impl Game {
    pub fn new(board: BoardBits, player_count: u8, use_column_rule: bool) -> Self {
        Game {
            winner: None,
            locked_players: ArrayVec::new(),
            board,
            round: RoundState::Fresh,
            pending_moves: ArrayVec::new(),
            goals: ArrayVec::new(),
            player_count,
            use_column_rule,
            scratch: RoundScratch::new(),
        }
    }

    /// Propose a move, returning feedback and whether round is ready to complete
    #[cfg_attr(feature = "tracing", instrument)]
    pub fn propose_move(&mut self, m: Move) -> (MoveFeedback, bool) {
        use MoveFeedback::*;

        let mut cfs = CoordsFeedback {
            data: ArrayVec::new(),
        };

        fn consider(cfs: &mut CoordsFeedback, b: &BoardBits, c: Coord) -> bool {
            use CoordFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_automaton(c) {
                Automaton
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
            WaitYourTurn
        } else if !consider(&mut cfs, &self.board, m.from) | !consider(&mut cfs, &self.board, m.to)
        {
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

    /// Try to complete the round
    #[cfg_attr(feature = "tracing", instrument)]
    pub fn try_complete_round(&mut self) -> Result<ArrayVec<(Move, MoveResult), MAX_PLAYERS>, ()> {
        if self.pending_moves.len() != self.player_count as usize {
            return Err(());
        }

        match self.scratch.detect_conflicts(&self.pending_moves) {
            Ok(moves_to_apply) => {
                // Build lifted mask
                let lifted = LiftedMask::from_moves(&moves_to_apply);

                let mut results = ArrayVec::new();
                let mut moves_copy = moves_to_apply;

                // Apply moves iteratively (handle cycles)
                while !moves_copy.is_empty() {
                    let mut made_progress = false;
                    moves_copy.retain(|m| {
                        if self.board.is_vacuum(m.from) {
                            true // Skip this move for now
                        } else {
                            let res = self.board.apply_move(*m, &lifted);
                            match res {
                                Ok(_) => results.push((*m, MoveResult::Applied)),
                                Err(e) => results.push((*m, e)),
                            }
                            made_progress = true;
                            false
                        }
                    });

                    if !made_progress {
                        for m in moves_copy.drain(..) {
                            results.push((m, MoveResult::NoSource));
                        }
                    }
                }

                // Update automaton
                self.update_automaton();

                // Check win condition
                if let Some((_, who)) = self.goals.iter().copied().find(|(c, _)| c == &self.board.automaton_location) {
                    self.round = RoundState::GameOver;
                    self.winner = Some(who);
                } else {
                    self.locked_players.clear();
                    self.round = RoundState::Fresh;
                }

                self.pending_moves.clear();
                Ok(results)
            }
            Err(conflicted) => {
                self.round = RoundState::ResolvingConflict;

                // Lock non-conflicted players
                for &m in &self.pending_moves {
                    let is_conflicted = conflicted.iter().any(|cm| cm.who == m.who);
                    if !is_conflicted && !self.locked_players.contains(&m.who) {
                        self.locked_players.push(m.who);
                    }
                }

                // Remove conflicted moves
                self.pending_moves.retain(|m| !conflicted.iter().any(|cm| cm.who == m.who));

                Err(())
            }
        }
    }

    /// Update automaton position
    pub fn update_automaton(&mut self) {
        let new_location = self.automaton_move();
        if new_location != self.board.automaton_location {
            self.board.force_move(self.board.automaton_location, new_location);
        }
    }

    /// Calculate where automaton should move
    #[cfg_attr(feature = "tracing", instrument)]
    pub fn automaton_move(&self) -> Coord {
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
                    rep_dist: core::cmp::min(pos.dist, neg.dist),
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
                    att_dist: core::cmp::min(pos.dist, neg.dist),
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

        let loc = self.board.automaton_location;
        let xp = self.board.raycast(loc, Delta::XP);
        let xn = self.board.raycast(loc, Delta::XN);
        let yp = self.board.raycast(loc, Delta::YP);
        let yn = self.board.raycast(loc, Delta::YN);

        let x_decision = evaluate_axis(&xp, &xn);
        let y_decision = evaluate_axis(&yp, &yn);

        let offset = if x_decision > y_decision {
            x_decision.delta(Delta::XP)
        } else if y_decision > x_decision {
            y_decision.delta(Delta::YP)
        } else {
            if self.use_column_rule {
                x_decision.delta(Delta::XP)
            } else {
                info!("avoided applying column rule - no move");
                Delta::ZERO
            }
        };

        loc + offset
    }
}

// ============================================================================
// OPERATORS & DISPLAY
// ============================================================================

impl core::ops::Sub for Coord {
    type Output = Delta;
    fn sub(self, other: Coord) -> Delta {
        Delta {
            dx: (self.x as i8 - other.x as i8),
            dy: (self.y as i8 - other.y as i8),
        }
    }
}

impl core::ops::Add<Delta> for Coord {
    type Output = Coord;
    fn add(self, other: Delta) -> Coord {
        Coord {
            x: (self.x as i8).saturating_add(other.dx) as u8,
            y: (self.y as i8).saturating_add(other.dy) as u8,
        }
    }
}

impl core::ops::Mul<isize> for Delta {
    type Output = Delta;
    fn mul(self, other: isize) -> Delta {
        Delta {
            dx: (self.dx as isize * other) as i8,
            dy: (self.dy as isize * other) as i8,
        }
    }
}

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {:?} ", coord, feedback)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for MoveFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            MoveFeedback::Committed => write!(f, "committed"),
            MoveFeedback::SeeCoords(cfs) => write!(f, "see coords: {}", cfs),
            MoveFeedback::MustMove => write!(f, "must move"),
            MoveFeedback::AxisAlignedOnly => write!(f, "axis aligned only"),
            MoveFeedback::WaitYourTurn => write!(f, "wait your turn"),
            MoveFeedback::GameOver => write!(f, "game over"),
        }
    }
}

impl core::fmt::Display for MoveResult {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            MoveResult::NoSource => write!(f, "no source"),
            MoveResult::OccupiedAt(c) => write!(f, "occupied at {}", c),
            MoveResult::Applied => write!(f, "applied"),
        }
    }
}

impl core::fmt::Debug for BoardBits {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{}x{} board with automaton at {}",
            self.size.x, self.size.y, self.automaton_location
        )
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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct AutMoveError {
        board: BoardBits,
        expected_move: Delta,
        actual_move: Delta,
    }

    type AutMoveTest = Result<(), AutMoveError>;

    fn expect_automaton_move(game: &Game, by: Delta) -> AutMoveTest {
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
        Game::new(BoardBits::stock_testing_empty(), 2, true)
    }

    #[test]
    fn automaton_stays_put() -> AutMoveTest {
        let board = BoardBits::stock_two_player();
        let game = Game::new(board, 2, true);
        expect_automaton_move(&game, Delta::ZERO)
    }

    #[test]
    fn unbalanced_pair() -> AutMoveTest {
        for &d in Delta::AXIAL_UNITS.iter() {
            let mut game = testing_game();
            let loc = game.board.automaton_location;
            game.board.place(loc + d * 2, Particle::Attractor);
            game.board.place(loc + d * (-2), Particle::Repulsor);
            expect_automaton_move(&game, d)?;

            let clean_board = game.board.clone();
            let perp = d.perpendicular();

            game.board.place(loc + perp * 2, Particle::Attractor);
            expect_automaton_move(&game, d)?;
            game.board.place(loc + perp * (-2), Particle::Attractor);
            expect_automaton_move(&game, d)?;

            game.board = clean_board;

            game.board.place(loc + perp * 2, Particle::Repulsor);
            expect_automaton_move(&game, d)?;
            game.board.place(loc + perp * (-2), Particle::Repulsor);
            expect_automaton_move(&game, d)?;
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
            expect_automaton_move(&game, Delta::ZERO)?;

            game.board = clean_board;

            game.board.place(loc + d * 2, Particle::Attractor);
            game.board.place(loc + d * (-1), Particle::Repulsor);
            expect_automaton_move(&game, d)?;
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
                expect_automaton_move(g, e)?;
                g.board.place(loc + perp * (-2), Particle::Attractor);
                expect_automaton_move(g, e)?;
                Ok(())
            };

            let clean_board = game.board.clone();

            game.board.place(loc + d * (-1), Particle::Repulsor);
            expect_automaton_move(&game, d)?;
            try_with_attractors(&mut game, d)?;

            game.board = clean_board.clone();
            game.board.place(loc + d * (-2), Particle::Repulsor);
            expect_automaton_move(&game, d)?;
            try_with_attractors(&mut game, d)?;

            game.board = clean_board.clone();
            game.board.place(loc + d * (-1), Particle::Repulsor);
            game.board.place(loc + d * 2, Particle::Repulsor);
            expect_automaton_move(&game, d)?;
            try_with_attractors(&mut game, d)?;
        }
        Ok(())
    }

    #[test]
    fn conflict_resolution_basic() {
        let mut game = Game::new(BoardBits::stock_testing(), 2, true);

        game.board.place(Coord { x: 1, y: 2 }, Particle::Attractor);

        let p0 = Pid(0);
        let p1 = Pid(1);

        let move1 = Move {
            who: p0,
            from: Coord { x: 0, y: 2 },
            to: Coord { x: 1, y: 2 },
        };
        let move2 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 2 },
        };

        game.board.place(move1.from, Particle::Attractor);
        game.board.place(move2.from, Particle::Repulsor);

        let (fb1, _) = game.propose_move(move1);
        let (fb2, ready) = game.propose_move(move2);

        assert_eq!(fb1, MoveFeedback::Committed);
        assert_eq!(fb2, MoveFeedback::Committed);
        assert!(ready);

        let result = game.try_complete_round();
        assert!(result.is_err());

        assert_eq!(game.pending_moves.len(), 0);
        assert_eq!(game.locked_players.len(), 0);
    }

    #[test]
    fn conflict_resolution_source_conflict() {
        let mut game = Game::new(BoardBits::stock_testing(), 2, true);

        let p0 = Pid(0);
        let p1 = Pid(1);

        let shared_source = Coord { x: 1, y: 3 };
        let move1 = Move {
            who: p0,
            from: shared_source,
            to: Coord { x: 0, y: 3 },
        };
        let move2 = Move {
            who: p1,
            from: shared_source,
            to: Coord { x: 1, y: 4 },
        };

        game.board.place(shared_source, Particle::Attractor);

        let (fb1, _) = game.propose_move(move1);
        let (fb2, ready) = game.propose_move(move2);

        assert_eq!(fb1, MoveFeedback::Committed);
        assert_eq!(fb2, MoveFeedback::Committed);
        assert!(ready);

        let result = game.try_complete_round();
        assert!(result.is_err());
    }

    #[test]
    fn conflict_resolution_locked_players() {
        let mut game = Game::new(BoardBits::stock_testing(), 3, true);

        let p0 = Pid(0);
        let p1 = Pid(1);
        let p2 = Pid(2);

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);
        game.board.place(Coord { x: 3, y: 1 }, Particle::Attractor);

        let move1 = Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 1, y: 1 },
        };
        let move2 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 1 },
        };
        let move3 = Move {
            who: p2,
            from: Coord { x: 3, y: 1 },
            to: Coord { x: 3, y: 0 },
        };

        game.propose_move(move1);
        game.propose_move(move2);
        let (_, ready) = game.propose_move(move3);

        assert!(ready);

        let result = game.try_complete_round();
        assert!(result.is_err());

        assert_eq!(game.locked_players.len(), 1);
        assert!(game.locked_players.contains(&p2));

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

        let move4 = Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 0, y: 0 },
        };
        let move5 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 4 },
        };

        game.propose_move(move4);
        let (_, ready2) = game.propose_move(move5);

        assert!(ready2);

        let result2 = game.try_complete_round();
        assert!(result2.is_ok());
    }

    #[test]
    fn conflict_resolution_resubmit() {
        let mut game = Game::new(BoardBits::stock_testing(), 2, true);

        let p0 = Pid(0);
        let p1 = Pid(1);

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

        let move1 = Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 1, y: 1 },
        };
        let move2 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 1 },
        };

        game.propose_move(move1);
        let (_, ready) = game.propose_move(move2);
        assert!(ready);

        let result = game.try_complete_round();
        assert!(result.is_err());

        let move3 = Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 0, y: 0 },
        };
        let (fb3, _) = game.propose_move(move3);
        assert_eq!(fb3, MoveFeedback::Committed);

        let move4 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 4 },
        };
        let (fb4, ready2) = game.propose_move(move4);
        assert_eq!(fb4, MoveFeedback::Committed);
        assert!(ready2);

        let result2 = game.try_complete_round();
        assert!(result2.is_ok());
    }

    #[test]
    fn move_cycle_basic() {
        let mut game = Game::new(BoardBits::stock_testing(), 2, true);

        let p0 = Pid(0);
        let p1 = Pid(1);

        game.board.place(Coord { x: 1, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 2, y: 1 }, Particle::Repulsor);

        let move1 = Move {
            who: p0,
            from: Coord { x: 1, y: 1 },
            to: Coord { x: 2, y: 1 },
        };
        let move2 = Move {
            who: p1,
            from: Coord { x: 2, y: 1 },
            to: Coord { x: 1, y: 1 },
        };

        game.propose_move(move1);
        game.propose_move(move2);

        let result = game.try_complete_round();
        assert!(result.is_ok());

        let results = result.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn locked_players_cleared_after_success() {
        let mut game = Game::new(BoardBits::stock_testing(), 3, true);

        let p0 = Pid(0);
        let p1 = Pid(1);
        let p2 = Pid(2);

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);
        game.board.place(Coord { x: 3, y: 1 }, Particle::Attractor);

        game.propose_move(Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 1, y: 1 },
        });
        game.propose_move(Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 1 },
        });
        game.propose_move(Move {
            who: p2,
            from: Coord { x: 3, y: 1 },
            to: Coord { x: 3, y: 0 },
        });

        game.try_complete_round().unwrap_err();
        assert_eq!(game.locked_players.len(), 1);
        assert!(game.locked_players.contains(&p2));

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

        game.propose_move(Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 0, y: 0 },
        });
        game.propose_move(Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 4 },
        });

        let result = game.try_complete_round();
        assert!(result.is_ok());

        assert_eq!(game.locked_players.len(), 0);
    }

    #[test]
    fn pending_moves_cleared_after_successful_round() {
        let mut game = Game::new(BoardBits::stock_testing(), 2, true);

        let p0 = Pid(0);
        let p1 = Pid(1);

        game.board.place(Coord { x: 0, y: 1 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 3 }, Particle::Repulsor);

        let move1 = Move {
            who: p0,
            from: Coord { x: 0, y: 1 },
            to: Coord { x: 0, y: 0 },
        };
        let move2 = Move {
            who: p1,
            from: Coord { x: 1, y: 3 },
            to: Coord { x: 1, y: 4 },
        };

        game.propose_move(move1);
        let (_, ready) = game.propose_move(move2);
        assert!(ready);
        assert_eq!(game.pending_moves.len(), 2);

        let result = game.try_complete_round();
        assert!(result.is_ok());

        assert_eq!(game.pending_moves.len(), 0);
        assert_eq!(game.round, RoundState::Fresh);

        game.board.place(Coord { x: 0, y: 0 }, Particle::Attractor);
        game.board.place(Coord { x: 1, y: 4 }, Particle::Repulsor);

        let move3 = Move {
            who: p0,
            from: Coord { x: 0, y: 0 },
            to: Coord { x: 1, y: 0 },
        };
        let move4 = Move {
            who: p1,
            from: Coord { x: 1, y: 4 },
            to: Coord { x: 2, y: 4 },
        };

        game.propose_move(move3);
        let (_, ready2) = game.propose_move(move4);
        assert!(ready2);
        assert_eq!(game.pending_moves.len(), 2);
    }
}

// ============================================================================
// MAIN (CLI)
// ============================================================================

#[cfg(all(not(test), feature = "std"))]
pub fn main() {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::str::FromStr;

    let mut game = Game::new(BoardBits::stock_two_player(), 2, true);
    let mut line = String::new();
    let stdin = std::io::stdin();

    while game.winner.is_none() {
        println!("game state: {:?}", game.board);

        line.clear();
        stdin.read_line(&mut line).expect("i/o error");

        let mut spl = line.split_whitespace();

        let pid = u8::from_str(spl.next().unwrap_or("0")).unwrap_or(0);
        let srcx = u8::from_str(spl.next().unwrap_or("0")).unwrap_or(0);
        let srcy = u8::from_str(spl.next().unwrap_or("0")).unwrap_or(0);
        let dstx = u8::from_str(spl.next().unwrap_or("0")).unwrap_or(0);
        let dsty = u8::from_str(spl.next().unwrap_or("0")).unwrap_or(0);

        let (fdb, go) = game.propose_move(Move {
            who: Pid(pid),
            from: Coord { x: srcx, y: srcy },
            to: Coord { x: dstx, y: dsty },
        });
        println!("Move feedback: {}", fdb);

        if go {
            match game.try_complete_round() {
                Ok(completed_moves) => {
                    for (m, res) in completed_moves {
                        println!("Player {}: {}", m.who.0, res);
                    }
                }
                Err(()) => {
                    let locked: Vec<String> = game.locked_players.iter().map(|p| format!("{}", p.0)).collect();
                    println!("Players locked: {}", locked.join(", "));
                }
            }
        }
    }

    println!("Player {} wins!", game.winner.unwrap().0);
}

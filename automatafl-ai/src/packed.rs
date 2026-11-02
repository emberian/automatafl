//! Allocation-free `PackedBoard` and `PackedRoundState` representation for high-speed
//! simultaneous-move simulation and AI search.
//!
//! Uses unboxed 128-bit bitboards (`[u64; 2]`) and fixed-capacity stack buffers to execute
//! the canonical `AutomataflRules.lean` semantics (plus R2 variant generalizations) with
//! zero heap allocations in the hot loop.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Vacuum particle code.
pub const VAC: u8 = 0;
/// Repulsor particle code.
pub const REP: u8 = 1;
/// Attractor particle code.
pub const ATT: u8 = 2;
/// Automaton particle code.
pub const AUTO: u8 = 3;

/// Maximum supported board edge (`11 x 11 = 121 <= 128` bits).
pub const MAX_BOARD_SIZE: usize = 11;
/// Maximum number of cells (`121` for 11x11, fits in 128-bit bitboard).
pub const MAX_CELLS: usize = 128;
/// Maximum moves per round (2 in 2P, 4 in 4P).
pub const MAX_PLAYERS: usize = 4;
/// Maximum tactical moves generated on stack for a single player.
pub const MAX_TACTICAL_MOVES: usize = 512;

/// Unboxed 128-bit bitmask (`[u64; 2]`) representing a set of board cell indices `0..128`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BitBoard128(pub [u64; 2]);

impl BitBoard128 {
    pub const EMPTY: Self = Self([0, 0]);

    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        (self.0[0] | self.0[1]) == 0
    }

    #[inline(always)]
    pub const fn get(self, idx: usize) -> bool {
        if idx < 64 {
            ((self.0[0] >> idx) & 1) != 0
        } else if idx < 128 {
            ((self.0[1] >> (idx - 64)) & 1) != 0
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn set(&mut self, idx: usize) {
        if idx < 64 {
            self.0[0] |= 1u64 << idx;
        } else if idx < 128 {
            self.0[1] |= 1u64 << (idx - 64);
        }
    }

    #[inline(always)]
    pub fn clear(&mut self, idx: usize) {
        if idx < 64 {
            self.0[0] &= !(1u64 << idx);
        } else if idx < 128 {
            self.0[1] &= !(1u64 << (idx - 64));
        }
    }

    #[inline(always)]
    pub const fn union(self, other: Self) -> Self {
        Self([self.0[0] | other.0[0], self.0[1] | other.0[1]])
    }

    #[inline(always)]
    pub const fn intersect(self, other: Self) -> Self {
        Self([self.0[0] & other.0[0], self.0[1] & other.0[1]])
    }

    #[inline(always)]
    pub const fn without(self, other: Self) -> Self {
        Self([self.0[0] & !other.0[0], self.0[1] & !other.0[1]])
    }

    #[inline(always)]
    pub const fn count_ones(self) -> u32 {
        self.0[0].count_ones() + self.0[1].count_ones()
    }

    #[inline(always)]
    pub const fn iter_ones(self) -> BitIter128 {
        BitIter128 {
            w0: self.0[0],
            w1: self.0[1],
        }
    }
}

/// Zero-allocation iterator over set bit indices in a [`BitBoard128`].
#[derive(Clone, Copy, Debug)]
pub struct BitIter128 {
    w0: u64,
    w1: u64,
}

impl Iterator for BitIter128 {
    type Item = usize;

    #[inline(always)]
    fn next(&mut self) -> Option<usize> {
        if self.w0 != 0 {
            let tz = self.w0.trailing_zeros() as usize;
            self.w0 &= self.w0 - 1;
            Some(tz)
        } else if self.w1 != 0 {
            let tz = self.w1.trailing_zeros() as usize;
            self.w1 &= self.w1 - 1;
            Some(64 + tz)
        } else {
            None
        }
    }
}

/// Unboxed 2-byte coordinate `(x, y)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackedCoord {
    pub x: u8,
    pub y: u8,
}

impl PackedCoord {
    #[inline(always)]
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    #[inline(always)]
    pub const fn manhattan(self, other: Self) -> u32 {
        let dx = (self.x as i32 - other.x as i32).unsigned_abs();
        let dy = (self.y as i32 - other.y as i32).unsigned_abs();
        dx + dy
    }
}

/// Unboxed 5-byte move `(who, frm, to)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedMove {
    pub who: u8,
    pub frm: PackedCoord,
    pub to: PackedCoord,
}

impl PackedMove {
    #[inline(always)]
    pub const fn new(who: u8, frm: PackedCoord, to: PackedCoord) -> Self {
        Self { who, frm, to }
    }
}

/// Fixed-capacity stack buffer of [`PackedMove`]s for zero-allocation move generation.
#[derive(Clone)]
pub struct MoveList {
    buf: [PackedMove; MAX_TACTICAL_MOVES],
    len: usize,
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveList {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            buf: [PackedMove {
                who: 0,
                frm: PackedCoord { x: 0, y: 0 },
                to: PackedCoord { x: 0, y: 0 },
            }; MAX_TACTICAL_MOVES],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline(always)]
    pub fn push(&mut self, m: PackedMove) {
        if self.len < MAX_TACTICAL_MOVES {
            self.buf[self.len] = m;
            self.len += 1;
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[PackedMove] {
        &self.buf[..self.len]
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [PackedMove] {
        &mut self.buf[..self.len]
    }
}

/// Board size and opening layout preset (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardPreset {
    /// 11x11 Classic (`AutomataflRules.stockTwoPlayer`).
    #[default]
    Classic11x11,
    /// 9x9 Tactical.
    Tactical9x9,
    /// 7x7 Blitz.
    Blitz7x7,
    /// 5x5 Mini testing board.
    Mini5x5,
}

impl BoardPreset {
    #[inline(always)]
    pub const fn size(self) -> u8 {
        match self {
            BoardPreset::Classic11x11 => 11,
            BoardPreset::Tactical9x9 => 9,
            BoardPreset::Blitz7x7 => 7,
            BoardPreset::Mini5x5 => 5,
        }
    }
}

/// Equal-priority automaton tie-break mode (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreakMode {
    /// Prefer the Y axis (`AutomataflRules.TieBreak.column`, default).
    #[default]
    Column,
    /// Prefer the X axis (`AutomataflRules.TieBreak.row`).
    Row,
    /// Freeze on an equal-priority tie (`AutomataflRules.TieBreak.freeze`).
    Freeze,
    /// Prefer continuing on the same (or orthogonal) axis as the previous automaton step.
    Momentum { prefer_same_axis: bool },
}

/// Merge / confluence resolution mode when multiple movers converge on one square (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfluenceMode {
    /// Canonical `AutomataflRules`: mark the contested landing square and re-enter the round.
    #[default]
    Conflict,
    /// Stop 1 square short of the merge point without re-entry (`MERGE_RESOLUTION_DESIGN.md`).
    BunchBefore,
    /// Colliding movers annihilate each other.
    Annihilate,
}

/// Configurable game rules and variant parameters (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameConfig {
    pub preset: BoardPreset,
    pub tie_break: TieBreakMode,
    pub confluence: ConfluenceMode,
    pub two_cycle_swaps: bool,
    pub cooldown_turns: u8,
    pub railgun_fling: bool,
    pub automata_count: u8,
    pub max_turns: u16,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            preset: BoardPreset::Classic11x11,
            tie_break: TieBreakMode::Column,
            confluence: ConfluenceMode::Conflict,
            two_cycle_swaps: false,
            cooldown_turns: 0,
            railgun_fling: false,
            automata_count: 1,
            max_turns: 64,
        }
    }
}

/// Fixed-capacity goal assignment on stack.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedGoals {
    pub entries: [(PackedCoord, u8); 16],
    pub len: u8,
}

impl PackedGoals {
    pub const fn empty() -> Self {
        Self {
            entries: [(PackedCoord { x: 0, y: 0 }, 0); 16],
            len: 0,
        }
    }

    /// Canonical 2-player stock goals (`AutomataflRules.stockGoals2`):
    /// Seat 0 owns `(0, 0)` and `(size - 1, 0)`; Seat 1 owns `(0, size - 1)` and `(size - 1, size - 1)`.
    pub const fn stock_two_player(size: u8) -> Self {
        if size == 0 {
            return Self::empty();
        }
        let last = size - 1;
        let mut entries = [(PackedCoord { x: 0, y: 0 }, 0); 16];
        entries[0] = (PackedCoord { x: 0, y: 0 }, 0);
        entries[1] = (PackedCoord { x: last, y: 0 }, 0);
        entries[2] = (PackedCoord { x: 0, y: last }, 1);
        entries[3] = (PackedCoord { x: last, y: last }, 1);
        Self { entries, len: 4 }
    }

    /// Canonical 4-player stock goals (`AutomataflRules.stockGoals4`):
    /// One corner per seat counter-clockwise.
    pub const fn stock_four_player(size: u8) -> Self {
        if size == 0 {
            return Self::empty();
        }
        let last = size - 1;
        let mut entries = [(PackedCoord { x: 0, y: 0 }, 0); 16];
        entries[0] = (PackedCoord { x: 0, y: 0 }, 0);
        entries[1] = (PackedCoord { x: last, y: 0 }, 1);
        entries[2] = (PackedCoord { x: last, y: last }, 2);
        entries[3] = (PackedCoord { x: 0, y: last }, 3);
        Self { entries, len: 4 }
    }

    #[inline(always)]
    pub fn slice(&self) -> &[(PackedCoord, u8)] {
        &self.entries[..(self.len as usize).min(self.entries.len())]
    }

    #[inline(always)]
    pub fn owner_of(&self, c: PackedCoord) -> Option<u8> {
        for &(gc, seat) in self.slice() {
            if gc == c {
                return Some(seat);
            }
        }
        None
    }

    /// Distinct player seats appearing in `entries` (keeping last occurrence to match
    /// `AutomataflRules.GoalAssignment.seats` in Lean), defaulting to `[0, 1]` when empty.
    #[inline]
    pub fn seats(&self) -> SmallVec<[u8; 8]> {
        let s = self.slice();
        let mut out: SmallVec<[u8; 8]> = SmallVec::new();
        for idx in 0..s.len() {
            let owner = s[idx].1;
            if !s[idx + 1..].iter().any(|&(_, o2)| o2 == owner) {
                out.push(owner);
            }
        }
        if out.is_empty() {
            out.push(0);
            out.push(1);
        }
        out
    }

    /// Bitmask of waiting seats (`0..8`) for a fresh turn with these goals.
    #[inline]
    pub fn waiting_mask(&self) -> u8 {
        let mut mask = 0u8;
        for &(_, owner) in self.slice() {
            if owner < 8 {
                mask |= 1u8 << owner;
            }
        }
        if mask == 0 {
            0b11
        } else {
            mask
        }
    }
}

/// Maximum active cooldown entries tracked on stack (`32` covers all movable pieces).
pub const MAX_COOLDOWNS: usize = 32;

/// Flat, unboxed `#[repr(C)]` board state for allocation-free simulation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedBoard {
    pub size: u8,
    pub repulsors: BitBoard128,
    pub attractors: BitBoard128,
    pub automata: BitBoard128,
    pub auto_x: u8,
    pub auto_y: u8,
    pub col_rule: bool,
    pub last_dx: i8,
    pub last_dy: i8,
    pub cooldown_mask: BitBoard128,
    pub cooldown_cells: [u8; MAX_COOLDOWNS],
    pub cooldown_rem: [u8; MAX_COOLDOWNS],
    pub cooldown_len: u8,
}

impl PackedBoard {
    /// Create an empty `size x size` board with the automaton at `(auto_x, auto_y)`.
    pub fn empty(size: u8, auto_x: u8, auto_y: u8, col_rule: bool) -> Self {
        let mut b = Self {
            size,
            repulsors: BitBoard128::EMPTY,
            attractors: BitBoard128::EMPTY,
            automata: BitBoard128::EMPTY,
            auto_x,
            auto_y,
            col_rule,
            last_dx: 0,
            last_dy: 0,
            cooldown_mask: BitBoard128::EMPTY,
            cooldown_cells: [0; MAX_COOLDOWNS],
            cooldown_rem: [0; MAX_COOLDOWNS],
            cooldown_len: 0,
        };
        if auto_x < size && auto_y < size {
            let idx = b.idx(PackedCoord::new(auto_x, auto_y));
            b.automata.set(idx);
        }
        b
    }

    /// Build a board from a [`BoardPreset`] and [`GameConfig`].
    pub fn from_preset(preset: BoardPreset) -> Self {
        match preset {
            BoardPreset::Classic11x11 => Self::stock_two_player(),
            BoardPreset::Tactical9x9 => Self::stock_tactical_9x9(),
            BoardPreset::Blitz7x7 => Self::stock_blitz_7x7(),
            BoardPreset::Mini5x5 => Self::stock_mini_5x5(),
        }
    }

    /// Build a board configured with `cfg` (`presetBoard` + `useColumnRule`).
    pub fn from_config(cfg: &GameConfig) -> Self {
        let mut b = Self::from_preset(cfg.preset);
        b.col_rule = !matches!(cfg.tie_break, TieBreakMode::Freeze);
        b
    }

    /// Canonical 11x11 two-player opening (`AutomataflRules.stockTwoPlayer`) via `automatafl-lean`.
    pub fn stock_two_player() -> Self {
        let pb = automatafl_lean::PackedBoard::stock(automatafl_lean::BoardPreset::Classic11);
        PackedRoundState::from_packed_bytes(pb.as_bytes())
            .expect("Lean stockClassic11 bytes")
            .0
            .board
    }

    /// 9x9 Tactical preset board (`AutomataflRules.stockTactical9`) via `automatafl-lean`.
    pub fn stock_tactical_9x9() -> Self {
        let pb = automatafl_lean::PackedBoard::stock(automatafl_lean::BoardPreset::Tactical9);
        PackedRoundState::from_packed_bytes(pb.as_bytes())
            .expect("Lean stockTactical9 bytes")
            .0
            .board
    }

    /// 7x7 Blitz preset board (`AutomataflRules.stockBlitz7`) via `automatafl-lean`.
    pub fn stock_blitz_7x7() -> Self {
        let pb = automatafl_lean::PackedBoard::stock(automatafl_lean::BoardPreset::Blitz7);
        PackedRoundState::from_packed_bytes(pb.as_bytes())
            .expect("Lean stockBlitz7 bytes")
            .0
            .board
    }

    /// 5x5 Mini testing board, automaton centred at `(2, 2)`.
    pub fn stock_mini_5x5() -> Self {
        let mut b = Self::empty(5, 2, 2, true);
        b.place(PackedCoord::new(0, 1), ATT);
        b.place(PackedCoord::new(1, 0), REP);
        b.place(PackedCoord::new(4, 3), ATT);
        b.place(PackedCoord::new(3, 4), REP);
        b
    }

    #[inline(always)]
    pub const fn in_bounds(&self, c: PackedCoord) -> bool {
        c.x < self.size && c.y < self.size
    }

    #[inline(always)]
    pub const fn idx(&self, c: PackedCoord) -> usize {
        (c.y as usize) * (self.size as usize) + (c.x as usize)
    }

    #[inline(always)]
    pub const fn coord_of(&self, idx: usize) -> PackedCoord {
        let s = if self.size == 0 {
            1
        } else {
            self.size as usize
        };
        PackedCoord {
            x: (idx % s) as u8,
            y: (idx / s) as u8,
        }
    }

    #[inline(always)]
    pub const fn occupied(&self) -> BitBoard128 {
        self.repulsors.union(self.attractors).union(self.automata)
    }

    #[inline(always)]
    pub const fn movable_pieces(&self) -> BitBoard128 {
        self.repulsors.union(self.attractors)
    }

    #[inline(always)]
    pub const fn cell_at(&self, c: PackedCoord) -> u8 {
        if !self.in_bounds(c) {
            return VAC;
        }
        let i = self.idx(c);
        self.cell_at_idx(i)
    }

    #[inline(always)]
    pub const fn cell_at_idx(&self, i: usize) -> u8 {
        if self.automata.get(i) {
            AUTO
        } else if self.repulsors.get(i) {
            REP
        } else if self.attractors.get(i) {
            ATT
        } else {
            VAC
        }
    }

    #[inline(always)]
    pub const fn is_vacuum(&self, c: PackedCoord) -> bool {
        self.cell_at(c) == VAC
    }

    #[inline(always)]
    pub const fn is_automaton(&self, c: PackedCoord) -> bool {
        self.in_bounds(c) && self.automata.get(self.idx(c))
    }

    #[inline(always)]
    pub fn place(&mut self, c: PackedCoord, code: u8) {
        if !self.in_bounds(c) {
            return;
        }
        let i = self.idx(c);
        self. place_idx(i, code);
        if code == AUTO {
            self.auto_x = c.x;
            self.auto_y = c.y;
        }
    }

    #[inline(always)]
    pub fn place_idx(&mut self, i: usize, code: u8) {
        self.repulsors.clear(i);
        self.attractors.clear(i);
        self.automata.clear(i);
        match code {
            REP => self.repulsors.set(i),
            ATT => self.attractors.set(i),
            AUTO => self.automata.set(i),
            _ => {}
        }
    }

    #[inline(always)]
    pub const fn automaton(&self) -> PackedCoord {
        PackedCoord::new(self.auto_x, self.auto_y)
    }

    /// Encode this board into the 5-token wire format of `AutomataflFFI.lean`:
    /// `size cells autoX autoY colRule`.
    pub fn to_wire(&self) -> String {
        let n = self.size as usize;
        let mut s = String::with_capacity(n * n + 20);
        s.push_str(&n.to_string());
        s.push(' ');
        if n == 0 {
            s.push('-');
        } else {
            for y in 0..self.size {
                for x in 0..self.size {
                    let code = self.cell_at(PackedCoord::new(x, y));
                    s.push(char::from(b'0' + (code & 3)));
                }
            }
        }
        s.push(' ');
        s.push_str(&self.auto_x.to_string());
        s.push(' ');
        s.push_str(&self.auto_y.to_string());
        s.push(' ');
        s.push_str(if self.col_rule { "1" } else { "0" });
        s
    }

    /// Decode a board from the 5-token wire format of `AutomataflFFI.lean`.
    pub fn from_wire(wire: &str) -> Result<Self, String> {
        let toks: SmallVec<[&str; 8]> = wire.split_whitespace().collect();
        if toks.len() != 5 {
            return Err(format!("expected 5 board tokens, got {}", toks.len()));
        }
        let size: u8 = toks[0]
            .parse()
            .map_err(|e| format!("invalid board size: {e}"))?;
        if size as usize > MAX_BOARD_SIZE {
            return Err(format!("board size {size} exceeds MAX_BOARD_SIZE"));
        }
        let ax: u8 = toks[2]
            .parse()
            .map_err(|e| format!("invalid auto_x: {e}"))?;
        let ay: u8 = toks[3]
            .parse()
            .map_err(|e| format!("invalid auto_y: {e}"))?;
        let col_rule = toks[4] != "0";
        let mut b = Self::empty(size, ax, ay, col_rule);
        b.automata = BitBoard128::EMPTY;
        let expected = (size as usize) * (size as usize);
        if toks[1] == "-" {
            if expected != 0 {
                return Err(format!(
                    "expected {expected} cell chars for size {size}, got '-'"
                ));
            }
        } else {
            let bytes = toks[1].as_bytes();
            if bytes.len() != expected {
                return Err(format!(
                    "expected {expected} cell chars, got {}",
                    bytes.len()
                ));
            }
            for (idx, &ch) in bytes.iter().enumerate() {
                let code = ch.wrapping_sub(b'0');
                b.place_idx(idx, code);
            }
        }
        Ok(b)
    }

    /// Step the automaton (`AutomataflRules.automatonStepVariant`) via Lean's `packedStepFFI`,
    /// supporting `railgun_fling` (`flingLoop`) and retaining `last_dx` / `last_dy` when stationary.
    #[inline]
    pub fn automaton_step(&self, cfg: &GameConfig) -> Self {
        let cfg1 = GameConfig {
            automata_count: 1,
            ..*cfg
        };
        let goals = PackedGoals::stock_two_player(self.size);
        self.step_automata(&cfg1, &goals).0
    }

    /// Step the automaton `cfg.automata_count.max(1)` times (`AutomataflRules.stepAutomataCfg`)
    /// via Lean's `packedStepFFI`, stopping early if `winOnEntry` triggers.
    #[inline]
    pub fn step_automata(&self, cfg: &GameConfig, goals: &PackedGoals) -> (Self, Option<u8>) {
        if self.size == 0 || self.auto_x >= self.size || self.auto_y >= self.size {
            return (*self, None);
        }
        let mut base = *self;
        base.cooldown_mask = BitBoard128::EMPTY;
        base.cooldown_cells = [0; MAX_COOLDOWNS];
        base.cooldown_rem = [0; MAX_COOLDOWNS];
        base.cooldown_len = 0;

        let cfg_auto = GameConfig {
            confluence: ConfluenceMode::Conflict,
            two_cycle_swaps: false,
            cooldown_turns: 0,
            ..*cfg
        };
        let rs = PackedRoundState::open(base, 1);
        match rs.round_step(&cfg_auto, goals, &[]) {
            PackedRoundOutcome::Resolved { mut board, win } => {
                board.cooldown_mask = self.cooldown_mask;
                board.cooldown_cells = self.cooldown_cells;
                board.cooldown_rem = self.cooldown_rem;
                board.cooldown_len = self.cooldown_len;
                (board, win)
            }
            PackedRoundOutcome::Again(_) => (*self, None),
        }
    }

    /// Canonical move legality (`AutomataflRules.moveLegalVariantB`):
    /// - `frm != to`
    /// - rook-aligned (`frm.x == to.x || frm.y == to.y`)
    /// - both endpoints in bounds
    /// - `!isAutomaton(frm)` (Ruling D: automaton banned as source only)
    /// - neither endpoint in `marks`
    /// - source not on cooldown
    #[inline]
    pub const fn move_legal(&self, marks: BitBoard128, m: PackedMove) -> bool {
        if m.frm.x == m.to.x && m.frm.y == m.to.y {
            return false;
        }
        if m.frm.x != m.to.x && m.frm.y != m.to.y {
            return false;
        }
        if !self.in_bounds(m.frm) || !self.in_bounds(m.to) {
            return false;
        }
        let frm_idx = self.idx(m.frm);
        let to_idx = self.idx(m.to);
        if self.automata.get(frm_idx) {
            return false;
        }
        if marks.get(frm_idx) || marks.get(to_idx) {
            return false;
        }
        if self.cooldown_mask.get(frm_idx) {
            return false;
        }
        true
    }

    /// Inclusive path-blocked check (`AutomataflRules.blockedB`):
    /// Scans all cells strictly between `m.frm` and `m.to` (`interior`) PLUS `m.to` itself.
    /// Any occupied cell that is not in `moving_sources` blocks `m`.
    #[inline]
    pub const fn blocked_by_mask(&self, moving_sources: BitBoard128, m: PackedMove) -> bool {
        let occ_non_moving = self.occupied().without(moving_sources);
        if m.frm.x == m.to.x {
            let (lo, hi) = if m.frm.y < m.to.y {
                (m.frm.y, m.to.y)
            } else {
                (m.to.y, m.frm.y)
            };
            let mut y = lo.saturating_add(1);
            while y < hi {
                let c = PackedCoord::new(m.frm.x, y);
                if self.in_bounds(c) && occ_non_moving.get(self.idx(c)) {
                    return true;
                }
                y += 1;
            }
        } else {
            let (lo, hi) = if m.frm.x < m.to.x {
                (m.frm.x, m.to.x)
            } else {
                (m.to.x, m.frm.x)
            };
            let mut x = lo.saturating_add(1);
            while x < hi {
                let c = PackedCoord::new(x, m.frm.y);
                if self.in_bounds(c) && occ_non_moving.get(self.idx(c)) {
                    return true;
                }
                x += 1;
            }
        }
        self.in_bounds(m.to) && occ_non_moving.get(self.idx(m.to))
    }

    /// Generate all executable tactical moves for `seat` into `out` with zero heap allocations.
    ///
    /// Includes every unblocked legal move from an occupied movable piece (`repulsor` or `attractor`)
    /// that is neither marked nor on cooldown, accounting for `locked` move sources as passable.
    /// If all movable pieces are blocked along all rays, falls back to any legal move from a
    /// movable piece, and only if no movable piece has any legal move falls back to any legal
    /// move on the board.
    ///
    /// For odd seats (`seat & 1 == 1`), rows and vertical directions are scanned in vertically
    /// reflected order (`y: size-1..=0`, `dy: -1` before `+1`) so candidate ordering commutes
    /// with vertical reflection on symmetric boards.
    pub fn generate_tactical_moves(
        &self,
        marks: BitBoard128,
        locked: &[PackedMove],
        seat: u8,
        _cfg: &GameConfig,
        out: &mut MoveList,
    ) {
        out.clear();

        let mut locked_srcs = BitBoard128::EMPTY;
        for lm in locked {
            if self.in_bounds(lm.frm) {
                locked_srcs.set(self.idx(lm.frm));
            }
        }

        let candidate_sources = self
            .movable_pieces()
            .without(marks)
            .without(self.cooldown_mask)
            .without(self.automata);

        let reflect_y = (seat & 1) == 1;
        let dirs: [(i8, i8); 4] = if reflect_y {
            [(1, 0), (-1, 0), (0, -1), (0, 1)]
        } else {
            [(1, 0), (-1, 0), (0, 1), (0, -1)]
        };
        let size = self.size as i8;

        for ry in 0..self.size {
            let y = if reflect_y { self.size - 1 - ry } else { ry };
            for x in 0..self.size {
                let frm = PackedCoord::new(x, y);
                let src_idx = self.idx(frm);
                if !candidate_sources.get(src_idx) {
                    continue;
                }
                let mut moving_sources = locked_srcs;
                moving_sources.set(src_idx);

                for &(dx, dy) in &dirs {
                    let mut cx = frm.x as i8 + dx;
                    let mut cy = frm.y as i8 + dy;
                    while cx >= 0 && cx < size && cy >= 0 && cy < size {
                        let to = PackedCoord::new(cx as u8, cy as u8);
                        let to_idx = self.idx(to);
                        let cell_occupied =
                            self.occupied().get(to_idx) && !moving_sources.get(to_idx);

                        if !cell_occupied {
                            if !marks.get(to_idx) {
                                out.push(PackedMove::new(seat, frm, to));
                            }
                        } else {
                            break;
                        }
                        cx += dx;
                        cy += dy;
                    }
                }
            }
        }

        // Fallback 1: if no executable (unblocked) move exists, scan any legal (proposable)
        // move from a movable piece not on marks or cooldowns (preferring non-automaton targets first).
        if out.is_empty() {
            for allow_auto_dst in [false, true] {
                for ry in 0..self.size {
                    let y = if reflect_y { self.size - 1 - ry } else { ry };
                    for x in 0..self.size {
                        let frm = PackedCoord::new(x, y);
                        let src_idx = self.idx(frm);
                        if !candidate_sources.get(src_idx) {
                            continue;
                        }
                        for &(dx, dy) in &dirs {
                            let mut cx = x as i8 + dx;
                            let mut cy = y as i8 + dy;
                            while cx >= 0 && cx < size && cy >= 0 && cy < size {
                                let to = PackedCoord::new(cx as u8, cy as u8);
                                if allow_auto_dst || !self.is_automaton(to) {
                                    let m = PackedMove::new(seat, frm, to);
                                    if self.move_legal(marks, m) {
                                        out.push(m);
                                    }
                                }
                                cx += dx;
                                cy += dy;
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    break;
                }
            }
        }

        // Fallback 2: if still empty (e.g. no unmarked/uncooled movable pieces can legally move),
        // scan any legal move on the board (preferring non-automaton targets first).
        if out.is_empty() {
            for allow_auto_dst in [false, true] {
                'any_legal: for ry in 0..self.size {
                    let y = if reflect_y { self.size - 1 - ry } else { ry };
                    for x in 0..self.size {
                        let frm = PackedCoord::new(x, y);
                        for &(dx, dy) in &dirs {
                            let nx = x as i8 + dx;
                            let ny = y as i8 + dy;
                            if nx >= 0 && nx < size && ny >= 0 && ny < size {
                                let to = PackedCoord::new(nx as u8, ny as u8);
                                if !allow_auto_dst && self.is_automaton(to) {
                                    continue;
                                }
                                let m = PackedMove::new(seat, frm, to);
                                if self.move_legal(marks, m) {
                                    out.push(m);
                                    break 'any_legal;
                                }
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    break;
                }
            }
        }
    }

    /// Compute fork and collision clash coordinates under default `.conflict` mode
    /// (`AutomataflRules.clashCoords`).
    #[inline]
    pub fn clash_coords(&self, ms: &[PackedMove]) -> BitBoard128 {
        self.clash_coords_cfg(ms, &GameConfig::default())
    }

    /// Compute pre-move clash coordinates under `cfg` (`AutomataflRules.clashCoordsCfg`)
    /// by delegating directly to `automatafl-lean`.
    pub fn clash_coords_cfg(&self, ms: &[PackedMove], cfg: &GameConfig) -> BitBoard128 {
        if self.size == 0 || ms.is_empty() {
            return BitBoard128::EMPTY;
        }
        let reply = if matches!(cfg.confluence, ConfluenceMode::Conflict) {
            automatafl_lean::rules_wire(&format!(
                "clash {} 0 {}",
                self.to_wire(),
                encode_moves_wire(ms)
            ))
        } else {
            automatafl_lean::rules_wire(&format!(
                "roundcfg {} 0 {} 0 0 {} 0 0 0",
                encode_cfg_wire(cfg),
                self.to_wire(),
                encode_moves_wire(ms)
            ))
        };
        let toks: SmallVec<[&str; 32]> = reply.split_whitespace().collect();
        let coord_toks = if matches!(cfg.confluence, ConfluenceMode::Conflict) {
            if toks.first() != Some(&"1") {
                return BitBoard128::EMPTY;
            }
            &toks[1..]
        } else {
            if toks.len() < 3 || toks[0] != "1" || toks[1] != "A" {
                return BitBoard128::EMPTY;
            }
            &toks[2..]
        };
        parse_coords_mask(self, coord_toks)
    }

    /// Resolve a set of moves on this board (`AutomataflRules.resolveMovesCfg`) via `automatafl-lean`,
    /// returning `(resolved_board, unresolved_merge_coords)`.
    pub fn resolve_moves(&self, ms: &[PackedMove], cfg: &GameConfig) -> (Self, BitBoard128) {
        if self.size == 0 {
            return (*self, BitBoard128::EMPTY);
        }
        let bw = self.to_wire();
        let cfg_w = encode_cfg_wire(cfg);
        let ms_w = encode_moves_wire(ms);
        let cds_w = encode_cooldowns_wire(self);

        let round_reply = automatafl_lean::rules_wire(&format!(
            "roundcfg {cfg_w} 0 {bw} 0 0 {ms_w} 0 {cds_w} 0"
        ));
        let r_toks: SmallVec<[&str; 64]> = round_reply.split_whitespace().collect();
        if r_toks.len() >= 3 && r_toks[0] == "1" && r_toks[1] == "A" {
            let cs = parse_coords_mask(self, &r_toks[2..]);
            if !cs.is_empty() {
                return (*self, cs);
            }
        }

        let mid_reply =
            automatafl_lean::rules_wire(&format!("midcfg {cfg_w} {bw} 0 {ms_w}"));
        let m_toks: SmallVec<[&str; 8]> = mid_reply.split_whitespace().collect();
        if m_toks.len() != 6 || m_toks[0] != "1" {
            return (*self, BitBoard128::EMPTY);
        }
        let mid_wire = m_toks[1..].join(" ");
        let Ok(mut out) = Self::from_wire(&mid_wire) else {
            return (*self, BitBoard128::EMPTY);
        };
        out.last_dx = self.last_dx;
        out.last_dy = self.last_dy;

        // Apply Lean's `recordCooldowns` result from the `roundcfg` reply (`1 R <win> <board:5> <cds>...`)
        if r_toks.len() >= 9 && r_toks[0] == "1" && r_toks[1] == "R" {
            if let Ok(k) = r_toks[8].parse::<usize>() {
                let mut rest = &r_toks[9..];
                for _ in 0..k {
                    if rest.len() < 3 {
                        break;
                    }
                    if let (Ok(x), Ok(y), Ok(rem)) = (
                        rest[0].parse::<u8>(),
                        rest[1].parse::<u8>(),
                        rest[2].parse::<u8>(),
                    ) {
                        let c = PackedCoord::new(x, y);
                        if out.in_bounds(c) && rem > 0 && (out.cooldown_len as usize) < MAX_COOLDOWNS
                        {
                            let idx = out.idx(c);
                            let slot = out.cooldown_len as usize;
                            out.cooldown_cells[slot] = idx as u8;
                            out.cooldown_rem[slot] = rem;
                            out.cooldown_mask.set(idx);
                            out.cooldown_len += 1;
                        }
                    }
                    rest = &rest[3..];
                }
            }
        }
        (out, BitBoard128::EMPTY)
    }

    /// Check `winOnEntry`: returns `Some(seat)` iff an automaton moved into a goal corner owned by `seat`.
    #[inline]
    pub fn win_on_entry(before: &Self, after: &Self, goals: &PackedGoals) -> Option<u8> {
        if after.automata == before.automata {
            return None;
        }
        let newly_entered = after.automata.without(before.automata);
        for idx in newly_entered.iter_ones() {
            let c = after.coord_of(idx);
            if let Some(owner) = goals.owner_of(c) {
                return Some(owner);
            }
        }
        None
    }

    /// Minimum Manhattan distance from the automaton to any goal owned by `seat`
    /// (`AutomataflRules.goalDistance`).
    #[inline]
    pub fn goal_distance(&self, goals: &PackedGoals, seat: u8) -> Option<u32> {
        let auto = self.automaton();
        let mut best: Option<u32> = None;
        for &(gc, owner) in goals.slice() {
            if owner == seat {
                let d = auto.manhattan(gc);
                best = Some(match best {
                    Some(prev) if prev <= d => prev,
                    _ => d,
                });
            }
        }
        best
    }

    /// Capped-match terminal adjudication (`AutomataflRules.adjudicateCapped`).
    #[inline]
    pub fn adjudicate_capped(&self, goals: &PackedGoals) -> Option<u8> {
        let d0 = self.goal_distance(goals, 0);
        let d1 = self.goal_distance(goals, 1);
        match (d0, d1) {
            (Some(a), Some(b)) => {
                if a < b {
                    Some(0)
                } else if b < a {
                    Some(1)
                } else {
                    None
                }
            }
            (Some(_), None) => Some(0),
            (None, Some(_)) => Some(1),
            (None, None) => None,
        }
    }
}

fn encode_cfg_wire(cfg: &GameConfig) -> String {
    let tb = match cfg.tie_break {
        TieBreakMode::Column => "0",
        TieBreakMode::Row => "1",
        TieBreakMode::Freeze => "2",
        TieBreakMode::Momentum {
            prefer_same_axis: true,
        } => "3",
        TieBreakMode::Momentum {
            prefer_same_axis: false,
        } => "4",
    };
    let cf = match cfg.confluence {
        ConfluenceMode::Conflict => "0",
        ConfluenceMode::BunchBefore => "1",
        ConfluenceMode::Annihilate => "2",
    };
    let sw = if cfg.two_cycle_swaps { "1" } else { "0" };
    let fl = if cfg.railgun_fling { "1" } else { "0" };
    format!(
        "{tb} {cf} {sw} {} {fl} {}",
        cfg.cooldown_turns,
        cfg.automata_count.max(1)
    )
}

fn encode_moves_wire(ms: &[PackedMove]) -> String {
    let mut s = ms.len().to_string();
    for m in ms {
        s.push_str(&format!(
            " {} {} {} {} {}",
            m.who, m.frm.x, m.frm.y, m.to.x, m.to.y
        ));
    }
    s
}

fn encode_cooldowns_wire(b: &PackedBoard) -> String {
    let mut s = b.cooldown_len.to_string();
    for i in 0..b.cooldown_len as usize {
        let c = b.coord_of(b.cooldown_cells[i] as usize);
        s.push_str(&format!(" {} {} {}", c.x, c.y, b.cooldown_rem[i]));
    }
    s
}

fn parse_coords_mask(b: &PackedBoard, toks: &[&str]) -> BitBoard128 {
    let Some(&kt) = toks.first() else {
        return BitBoard128::EMPTY;
    };
    let Ok(k) = kt.parse::<usize>() else {
        return BitBoard128::EMPTY;
    };
    let mut rest = &toks[1..];
    let mut mask = BitBoard128::EMPTY;
    for _ in 0..k {
        if rest.len() < 2 {
            break;
        }
        if let (Ok(x), Ok(y)) = (rest[0].parse::<u8>(), rest[1].parse::<u8>()) {
            let c = PackedCoord::new(x, y);
            if b.in_bounds(c) {
                mask.set(b.idx(c));
            }
        }
        rest = &rest[2..];
    }
    mask
}

// ============================================================================
// PackedRoundState & Canonical `roundStep` (`AutomataflRules.lean` §7)
// ============================================================================

/// Error returned when a submission list contains duplicate moves from the same player seat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateSeatSubmissionError {
    pub seat: u8,
}

/// Verify that no player seat appears more than once in `subs`.
#[inline]
pub fn validate_no_duplicate_seats(
    subs: &[PackedMove],
) -> Result<(), DuplicateSeatSubmissionError> {
    let mut seen_mask = 0u8;
    for m in subs {
        if m.who >= 8 {
            return Err(DuplicateSeatSubmissionError { seat: m.who });
        }
        let bit = 1u8 << m.who;
        if (seen_mask & bit) != 0 {
            return Err(DuplicateSeatSubmissionError { seat: m.who });
        }
        seen_mask |= bit;
    }
    Ok(())
}

/// Unboxed public state of a round within a turn (`AutomataflRules.RoundState`).
///
/// Contains ONLY public information:
/// - `board`: the turn-start board (frozen across clash re-entry rounds)
/// - `marks`: accumulated conflict markers from prior clash rounds of the current turn
/// - `locked`: moves from non-conflicted seats that locked in prior clash rounds
/// - `waiting_mask`: bitmask of seats that owe a move in the current round
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackedRoundState {
    pub board: PackedBoard,
    pub marks: BitBoard128,
    pub locked: [PackedMove; MAX_PLAYERS],
    pub locked_len: u8,
    pub waiting_mask: u8,
}

/// Outcome of a single round (`AutomataflRules.RoundOutcome`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedRoundOutcome {
    /// The round clashed; contested coordinates are marked, non-conflicted moves lock,
    /// and conflicted seats re-enter.
    Again(PackedRoundState),
    /// The round resolved cleanly; returns the post-automaton-step board and optional winning seat.
    Resolved {
        board: PackedBoard,
        win: Option<u8>,
    },
}

impl PackedRoundState {
    /// Open a fresh turn on `board` for `num_players` seats (`AutomataflRules.openRound`).
    #[inline(always)]
    pub const fn open(board: PackedBoard, num_players: u8) -> Self {
        let waiting_mask = if num_players >= 8 {
            0xFF
        } else {
            (1u8 << num_players) - 1
        };
        Self::open_with_mask(board, waiting_mask)
    }

    /// Open a fresh turn on `board` with an explicit `waiting_mask` of active seats.
    #[inline(always)]
    pub const fn open_with_mask(board: PackedBoard, waiting_mask: u8) -> Self {
        Self {
            board,
            marks: BitBoard128::EMPTY,
            locked: [PackedMove {
                who: 0,
                frm: PackedCoord { x: 0, y: 0 },
                to: PackedCoord { x: 0, y: 0 },
            }; MAX_PLAYERS],
            locked_len: 0,
            waiting_mask,
        }
    }

    #[inline(always)]
    pub const fn is_waiting(&self, seat: u8) -> bool {
        seat < 8 && (self.waiting_mask & (1u8 << seat)) != 0
    }

    #[inline(always)]
    pub fn locked_slice(&self) -> &[PackedMove] {
        &self.locked[..self.locked_len as usize]
    }

    /// Bitmask of active seats in this match (combining `waiting_mask`, `locked` move owners,
    /// and any goal owners in `goals`).
    #[inline]
    pub fn active_player_mask(&self, goals: &PackedGoals) -> u8 {
        let mut mask = self.waiting_mask;
        for i in 0..self.locked_len as usize {
            if self.locked[i].who < 8 {
                mask |= 1u8 << self.locked[i].who;
            }
        }
        for &(_, owner) in goals.slice() {
            if owner < 8 {
                mask |= 1u8 << owner;
            }
        }
        if mask == 0 {
            0b11
        } else {
            mask
        }
    }

    /// Total number of active seats in this match (combining `waiting_mask` and `locked`).
    #[inline]
    pub fn active_player_count(&self) -> u8 {
        let mut mask = self.waiting_mask;
        for i in 0..self.locked_len as usize {
            if self.locked[i].who < 8 {
                mask |= 1u8 << self.locked[i].who;
            }
        }
        if mask == 0 {
            2
        } else {
            let highest = 8 - mask.leading_zeros() as u8;
            highest.max(2).min(MAX_PLAYERS as u8)
        }
    }

    /// Execute one canonical or variant round (`AutomataflRules.roundStepVariant`), allocation-free.
    #[inline]
    pub fn round_step(
        &self,
        cfg: &GameConfig,
        goals: &PackedGoals,
        subs: &[PackedMove],
    ) -> PackedRoundOutcome {
        self.round_step_with_waiting(cfg, goals, subs).0
    }

    /// Execute one canonical or variant round (`AutomataflRules.roundStepVariant`) via
    /// `automatafl_lean::packed_step_raw` (`lean/Automatafl/PackedBoard.lean::packedStepFFI`),
    /// returning both the [`PackedRoundOutcome`] and the Lean-ordered `waiting` seat list.
    pub fn round_step_with_waiting(
        &self,
        cfg: &GameConfig,
        goals: &PackedGoals,
        subs: &[PackedMove],
    ) -> (PackedRoundOutcome, SmallVec<[u8; 8]>) {
        let mut rs_in = *self;
        let mut clean_locked = [PackedMove::default(); MAX_PLAYERS];
        let mut clean_len = 0usize;
        for i in 0..rs_in.locked_len as usize {
            let lm = rs_in.locked[i];
            if rs_in.board.move_legal(BitBoard128::EMPTY, lm) {
                clean_locked[clean_len] = lm;
                clean_len += 1;
            }
        }
        rs_in.locked = clean_locked;
        rs_in.locked_len = clean_len as u8;
        if rs_in.waiting_mask == 0 && rs_in.locked_len == 0 {
            rs_in.waiting_mask = 1;
        }
        let state_bytes = rs_in.to_packed_bytes_with_goals(cfg, goals);
        let mut moves_bytes: SmallVec<[u8; 64]> = SmallVec::with_capacity(subs.len().min(25) * 5);
        if self.waiting_mask != 0 {
            for m in subs {
                if self.is_waiting(m.who)
                    && self.board.move_legal(self.marks, *m)
                    && moves_bytes.len() + 5 <= 125
                {
                    moves_bytes.push(m.who);
                    moves_bytes.push(m.frm.x);
                    moves_bytes.push(m.frm.y);
                    moves_bytes.push(m.to.x);
                    moves_bytes.push(m.to.y);
                }
            }
        }
        let outcome = automatafl_lean::packed_step_raw(&state_bytes, &moves_bytes)
            .expect("automatafl_lean::packed_step_raw failed");
        match outcome {
            automatafl_lean::PackedOutcome::Again(next_pb) => {
                let payload = next_pb.as_bytes();
                let n_wait = payload[57] as usize;
                let mut waiting_order: SmallVec<[u8; 8]> = SmallVec::new();
                for &w in &payload[58..58 + n_wait] {
                    waiting_order.push(w);
                }
                let (next_rs, _, _) = Self::from_packed_bytes(payload)
                    .expect("Lean packed_step_raw emitted valid packed state bytes");
                (PackedRoundOutcome::Again(next_rs), waiting_order)
            }
            automatafl_lean::PackedOutcome::Resolved { board: next_pb, win } => {
                let payload = next_pb.as_bytes();
                let n_wait = payload[57] as usize;
                let mut waiting_order: SmallVec<[u8; 8]> = SmallVec::new();
                for &w in &payload[58..58 + n_wait] {
                    waiting_order.push(w);
                }
                let (next_rs, _, _) = Self::from_packed_bytes(payload)
                    .expect("Lean packed_step_raw emitted valid packed state bytes");
                (
                    PackedRoundOutcome::Resolved {
                        board: next_rs.board,
                        win: win.map(|w| w as u8),
                    },
                    waiting_order,
                )
            }
        }
    }

    /// Compute both the `proposable` (`moveLegalVariantB`) and `executable`
    /// (`carAt && moveLegalVariantB && !blockedB`) target bitmasks from `frm` via
    /// `automatafl_lean::packed_legal_targets_raw` (`lean/Automatafl/PackedBoard.lean::packedLegalTargetsFFI`).
    #[inline]
    pub fn legal_target_masks(&self, seat: u8, frm: PackedCoord) -> (BitBoard128, BitBoard128) {
        if !self.board.in_bounds(frm) {
            return (BitBoard128::EMPTY, BitBoard128::EMPTY);
        }
        let mut rs_in = *self;
        if rs_in.waiting_mask == 0 && rs_in.locked_len == 0 {
            rs_in.waiting_mask = 1;
        }
        let state_bytes = rs_in.to_packed_bytes(&GameConfig::default());
        let prop = automatafl_lean::packed_legal_targets_raw(&state_bytes, seat, frm.x, frm.y, false);
        let exec = automatafl_lean::packed_legal_targets_raw(&state_bytes, seat, frm.x, frm.y, true);
        (BitBoard128([prop.lo, prop.hi]), BitBoard128([exec.lo, exec.hi]))
    }

    /// Serialize this `PackedRoundState` + `GameConfig` (with stock 2-player or 4-player goals
    /// matching `active_player_count`) into the flat binary `ByteArray` format of
    /// `lean/Automatafl/PackedBoard.lean::PackedBoard.toBytes`.
    pub fn to_packed_bytes(&self, cfg: &GameConfig) -> Vec<u8> {
        let goals = if self.active_player_count() > 2 {
            PackedGoals::stock_four_player(self.board.size)
        } else {
            PackedGoals::stock_two_player(self.board.size)
        };
        self.to_packed_bytes_with_goals(cfg, &goals)
    }

    /// Serialize this `PackedRoundState` + `GameConfig` + `PackedGoals` into the flat binary
    /// `ByteArray` format of `lean/Automatafl/PackedBoard.lean::PackedBoard.toBytes`.
    pub fn to_packed_bytes_with_goals(&self, cfg: &GameConfig, goals: &PackedGoals) -> Vec<u8> {
        self.to_packed_bytes_with_goals_and_waiting(cfg, goals, None)
    }

    /// Serialize this `PackedRoundState` + `GameConfig` + `PackedGoals` into the flat binary
    /// `ByteArray` format of `lean/Automatafl/PackedBoard.lean::PackedBoard.toBytes`, using
    /// `waiting_order` when provided (or ascending `0..8` bit order from `waiting_mask`).
    pub fn to_packed_bytes_with_goals_and_waiting(
        &self,
        cfg: &GameConfig,
        goals: &PackedGoals,
        waiting_order: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut ba = Vec::with_capacity(
            64 + (self.locked_len as usize) * 5 + (goals.len as usize) * 3 + 4,
        );
        ba.extend_from_slice(&self.board.repulsors.0[0].to_le_bytes());
        ba.extend_from_slice(&self.board.repulsors.0[1].to_le_bytes());
        ba.extend_from_slice(&self.board.attractors.0[0].to_le_bytes());
        ba.extend_from_slice(&self.board.attractors.0[1].to_le_bytes());
        ba.extend_from_slice(&self.marks.0[0].to_le_bytes());
        ba.extend_from_slice(&self.marks.0[1].to_le_bytes());
        ba.push(self.board.size);
        ba.push(self.board.auto_x);
        ba.push(self.board.auto_y);
        let tie_byte = match cfg.tie_break {
            TieBreakMode::Column => 0,
            TieBreakMode::Row => 1,
            TieBreakMode::Freeze => 2,
            TieBreakMode::Momentum {
                prefer_same_axis: true,
            } => 3,
            TieBreakMode::Momentum {
                prefer_same_axis: false,
            } => 4,
        };
        ba.push(tie_byte);
        let conf_byte = match cfg.confluence {
            ConfluenceMode::Conflict => 0,
            ConfluenceMode::BunchBefore => 1,
            ConfluenceMode::Annihilate => 2,
        };
        ba.push(conf_byte);
        let flags = (if self.board.col_rule { 1u8 } else { 0u8 })
            | (if cfg.two_cycle_swaps { 2u8 } else { 0u8 })
            | (if cfg.railgun_fling { 4u8 } else { 0u8 });
        ba.push(flags);
        ba.push(cfg.cooldown_turns);
        ba.push(cfg.automata_count.max(1));
        let dir_byte = match (self.board.last_dx, self.board.last_dy) {
            (1, 0) => 1u8,
            (-1, 0) => 2u8,
            (0, 1) => 3u8,
            (0, -1) => 4u8,
            _ => 0u8,
        };
        ba.push(dir_byte);

        // Waiting seats
        if let Some(order) = waiting_order {
            let n_wait = order.len().min(255) as u8;
            ba.push(n_wait);
            ba.extend_from_slice(&order[..n_wait as usize]);
        } else {
            let n_wait = self.waiting_mask.count_ones() as u8;
            ba.push(n_wait);
            for s in 0..8u8 {
                if self.is_waiting(s) {
                    ba.push(s);
                }
            }
        }

        // Locked moves
        ba.push(self.locked_len);
        for m in self.locked_slice() {
            ba.push(m.who);
            ba.push(m.frm.x);
            ba.push(m.frm.y);
            ba.push(m.to.x);
            ba.push(m.to.y);
        }

        // Cooldowns
        ba.push(self.board.cooldown_len);
        for i in 0..self.board.cooldown_len as usize {
            let c = self.board.coord_of(self.board.cooldown_cells[i] as usize);
            ba.push(c.x);
            ba.push(c.y);
            ba.push(self.board.cooldown_rem[i]);
        }

        // Goals (`goals.len` byte followed by `[x, y, owner]` 3-byte triples)
        let gs = goals.slice();
        ba.push(gs.len() as u8);
        for &(c, owner) in gs {
            ba.push(c.x);
            ba.push(c.y);
            ba.push(owner);
        }
        ba
    }

    /// Deserialize a `(PackedRoundState, GameConfig, PackedGoals)` tuple from the flat binary
    /// `ByteArray` format of `lean/Automatafl/PackedBoard.lean::PackedBoard.fromBytes?`.
    pub fn from_packed_bytes(
        ba: &[u8],
    ) -> Result<(Self, GameConfig, PackedGoals), String> {
        if ba.len() < 60 {
            return Err(format!("packed byte buffer too short: {} < 60", ba.len()));
        }
        let read_u64 = |off: usize| -> u64 {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&ba[off..off + 8]);
            u64::from_le_bytes(arr)
        };
        let mut rep = BitBoard128([read_u64(0), read_u64(8)]);
        let mut att = BitBoard128([read_u64(16), read_u64(24)]);
        let mk = BitBoard128([read_u64(32), read_u64(40)]);
        let sz = ba[48];
        if sz == 0 || sz as usize > MAX_BOARD_SIZE {
            return Err(format!("invalid board size in packed bytes: {sz}"));
        }
        let ax = ba[49];
        let ay = ba[50];
        if ax >= sz || ay >= sz {
            return Err(format!(
                "automaton out of bounds in packed bytes: ({ax}, {ay}) >= {sz}"
            ));
        }
        let n_cells = (sz as usize) * (sz as usize);
        let (low_mask, high_mask) = if n_cells <= 64 {
            let lo = if n_cells == 64 {
                u64::MAX
            } else {
                (1u64 << n_cells) - 1
            };
            (lo, 0u64)
        } else {
            (u64::MAX, (1u64 << (n_cells - 64)) - 1)
        };
        if (rep.0[0] & !low_mask) != 0
            || (rep.0[1] & !high_mask) != 0
            || (att.0[0] & !low_mask) != 0
            || (att.0[1] & !high_mask) != 0
            || (mk.0[0] & !low_mask) != 0
            || (mk.0[1] & !high_mask) != 0
        {
            return Err("out-of-bounds bit set in packed bitboard mask".to_string());
        }
        let auto_idx = (ay as usize) * (sz as usize) + (ax as usize);
        rep.clear(auto_idx);
        att.clear(auto_idx);
        att = att.without(rep);

        let tie_break = match ba[51] {
            0 => TieBreakMode::Column,
            1 => TieBreakMode::Row,
            2 => TieBreakMode::Freeze,
            3 => TieBreakMode::Momentum {
                prefer_same_axis: true,
            },
            4 => TieBreakMode::Momentum {
                prefer_same_axis: false,
            },
            other => return Err(format!("invalid tie_break byte in packed bytes: {other}")),
        };
        let confluence = match ba[52] {
            0 => ConfluenceMode::Conflict,
            1 => ConfluenceMode::BunchBefore,
            2 => ConfluenceMode::Annihilate,
            other => return Err(format!("invalid confluence byte in packed bytes: {other}")),
        };
        let flg = ba[53];
        if (flg & !0b111) != 0 {
            return Err(format!("invalid flags byte in packed bytes: {flg:#x}"));
        }
        let col_rule = (flg & 1) != 0;
        let two_cycle_swaps = (flg & 2) != 0;
        let railgun_fling = (flg & 4) != 0;
        let cooldown_turns = ba[54];
        if ba[55] == 0 {
            return Err("invalid automata_count 0 in packed bytes".to_string());
        }
        let automata_count = ba[55];
        let (last_dx, last_dy) = match ba[56] {
            0 => (0i8, 0i8),
            1 => (1i8, 0i8),
            2 => (-1i8, 0i8),
            3 => (0i8, 1i8),
            4 => (0i8, -1i8),
            other => return Err(format!("invalid last_dir byte in packed bytes: {other}")),
        };
        let n_wait = ba[57] as usize;
        if n_wait > 8 || 58 + n_wait >= ba.len() {
            return Err("invalid or truncated waiting list in packed bytes".to_string());
        }
        let mut waiting_mask = 0u8;
        for i in 0..n_wait {
            let pid = ba[58 + i];
            if pid >= 8 {
                return Err(format!("invalid waiting seat pid in packed bytes: {pid}"));
            }
            waiting_mask |= 1u8 << pid;
        }
        let lock_off = 58 + n_wait;
        let n_lock = ba[lock_off] as usize;
        let cd_off = lock_off + 1 + n_lock * 5;
        if n_lock > MAX_PLAYERS || cd_off >= ba.len() {
            return Err("invalid or truncated locked list in packed bytes".to_string());
        }
        let mut locked = [PackedMove::default(); MAX_PLAYERS];
        let mut locked_len = 0usize;
        for i in 0..n_lock {
            let base = lock_off + 1 + i * 5;
            if locked_len < MAX_PLAYERS {
                locked[locked_len] = PackedMove::new(
                    ba[base],
                    PackedCoord::new(ba[base + 1], ba[base + 2]),
                    PackedCoord::new(ba[base + 3], ba[base + 4]),
                );
                locked_len += 1;
            }
        }
        if waiting_mask == 0 && locked_len == 0 {
            return Err("no active or locked seats in packed bytes".to_string());
        }
        let n_cd = ba[cd_off] as usize;
        let goal_off = cd_off + 1 + n_cd * 3;
        if n_cd > MAX_COOLDOWNS || goal_off > ba.len() {
            return Err("invalid or truncated cooldown list in packed bytes".to_string());
        }
        let mut board = PackedBoard::empty(sz, ax, ay, col_rule);
        board.repulsors = rep;
        board.attractors = att;
        board.last_dx = last_dx;
        board.last_dy = last_dy;
        for i in 0..n_cd {
            let base = cd_off + 1 + i * 3;
            let c = PackedCoord::new(ba[base], ba[base + 1]);
            let rem = ba[base + 2];
            if !board.in_bounds(c) || rem == 0 {
                return Err("invalid cooldown entry in packed bytes".to_string());
            }
            if (board.cooldown_len as usize) < MAX_COOLDOWNS {
                let idx = board.idx(c);
                let k = board.cooldown_len as usize;
                board.cooldown_cells[k] = idx as u8;
                board.cooldown_rem[k] = rem;
                board.cooldown_mask.set(idx);
                board.cooldown_len += 1;
            }
        }
        let goals = if goal_off == ba.len() {
            PackedGoals::stock_two_player(sz)
        } else {
            let n_goals = ba[goal_off] as usize;
            if n_goals > 16 || goal_off + 1 + n_goals * 3 != ba.len() {
                return Err("malformed goals list or trailing garbage in packed bytes".to_string());
            }
            if n_goals == 0 {
                PackedGoals::stock_two_player(sz)
            } else {
                let mut pg = PackedGoals::empty();
                for i in 0..n_goals.min(pg.entries.len()) {
                    let base = goal_off + 1 + i * 3;
                    let gc = PackedCoord::new(ba[base], ba[base + 1]);
                    let owner = ba[base + 2];
                    if !board.in_bounds(gc) || owner >= 8 {
                        return Err("invalid goal entry in packed bytes".to_string());
                    }
                    pg.entries[i] = (gc, owner);
                    pg.len += 1;
                }
                pg
            }
        };
        let preset = match sz {
            5 => BoardPreset::Mini5x5,
            7 => BoardPreset::Blitz7x7,
            9 => BoardPreset::Tactical9x9,
            _ => BoardPreset::Classic11x11,
        };
        let cfg = GameConfig {
            preset,
            tie_break,
            confluence,
            two_cycle_swaps,
            cooldown_turns,
            railgun_fling,
            automata_count,
            max_turns: 64,
        };
        let rs = PackedRoundState {
            board,
            marks: mk,
            locked,
            locked_len: locked_len as u8,
            waiting_mask,
        };
        Ok((rs, cfg, goals))
    }
}

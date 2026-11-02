//! R1.2 Allocation-free `PackedBoard` backed by the formally verified Lean 4 core.
//!
//! `PackedBoard` stores the compact binary representation defined in `lean/Automatafl/PackedBoard.lean`
//! (proven equivalent to `Board.cellAt` and `roundStep` / `roundStepVariant` via
//! `boardOfMasks_cellAt_eq`, `Board.reifyMasks_cellAt_eq`, `PackedBoard.unpack_pack`, and
//! `PackedBoard.unpack_pack_cellAt_eq`) inside a fixed-capacity stack buffer (`Copy`, zero Rust heap
//! allocation during search rollouts).

use crate::board::{ATT, AUTO, Board, Coord, Move, REP, VAC};
use crate::config::{BoardPreset, ConfluenceMode, CooldownEntry, Dir, GameConfig, TieBreak};
use crate::{
    ffi_packed_adjudicate, ffi_packed_legal_targets, ffi_packed_step, ffi_packed_stock,
};

/// 128-bit board bitmask (`lo`: bits 0..63, `hi`: bits 64..127) covering up to 11×11 = 121 cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitmask128 {
    pub lo: u64,
    pub hi: u64,
}

impl Bitmask128 {
    pub const ZERO: Self = Self { lo: 0, hi: 0 };

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.lo == 0 && self.hi == 0
    }

    #[inline]
    pub const fn count_ones(self) -> u32 {
        self.lo.count_ones() + self.hi.count_ones()
    }

    #[inline]
    pub const fn test_bit(self, idx: usize) -> bool {
        if idx < 64 {
            ((self.lo >> idx) & 1) != 0
        } else if idx < 128 {
            ((self.hi >> (idx - 64)) & 1) != 0
        } else {
            false
        }
    }

    #[inline]
    pub fn set_bit(&mut self, idx: usize) {
        if idx < 64 {
            self.lo |= 1u64 << idx;
        } else if idx < 128 {
            self.hi |= 1u64 << (idx - 64);
        }
    }

    #[inline]
    pub fn clear_bit(&mut self, idx: usize) {
        if idx < 64 {
            self.lo &= !(1u64 << idx);
        } else if idx < 128 {
            self.hi &= !(1u64 << (idx - 64));
        }
    }

    #[inline]
    pub const fn bitor(self, rhs: Self) -> Self {
        Self {
            lo: self.lo | rhs.lo,
            hi: self.hi | rhs.hi,
        }
    }

    #[inline]
    pub const fn bitand(self, rhs: Self) -> Self {
        Self {
            lo: self.lo & rhs.lo,
            hi: self.hi & rhs.hi,
        }
    }

    /// Decode all set bits within `size × size` into `(x, y)` coordinates in row-major order.
    pub fn to_coords(self, size: usize) -> Vec<Coord> {
        let mut out = Vec::with_capacity(self.count_ones() as usize);
        let total = size * size;
        for idx in 0..total.min(128) {
            if self.test_bit(idx) {
                out.push(((idx % size) as i32, (idx / size) as i32));
            }
        }
        out
    }
}

/// Maximum serialized byte size of a `PackedBoard` stack buffer.
pub const MAX_PACKED_BYTES: usize = 382;

/// Allocation-free `Copy` packed board state (`AutomataflRules.PackedBoard`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedBoard {
    buf: [u8; MAX_PACKED_BYTES],
    len: u16,
}

impl std::fmt::Debug for PackedBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackedBoard")
            .field("size", &self.size())
            .field("auto_coord", &self.auto_coord())
            .field("rep_mask", &self.rep_mask())
            .field("att_mask", &self.att_mask())
            .field("mark_mask", &self.mark_mask())
            .field("waiting", &self.waiting())
            .field("goals", &self.goals())
            .field("len", &self.len)
            .finish()
    }
}

/// Outcome of stepping a [`PackedBoard`] via [`PackedBoard::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedOutcome {
    /// Conflicted round: contested squares marked, offending moves dropped, waiting seats re-enter.
    Again(PackedBoard),
    /// Clean round resolved: post-turn board and optional winning seat.
    Resolved {
        board: PackedBoard,
        win: Option<u32>,
    },
}

#[inline]
fn read_u64_le(bytes: &[u8], off: usize) -> u64 {
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[off..off + 8]);
    u64::from_le_bytes(arr)
}

#[inline]
fn write_u64_le(bytes: &mut [u8], off: usize, val: u64) {
    bytes[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

impl PackedBoard {
    /// Default 2-player corner goals (`AutomataflRules.stockGoals2`) for a `size × size` board.
    pub fn stock_goals2_for_size(size: usize) -> Vec<(Coord, u32)> {
        let last = size.saturating_sub(1) as i32;
        vec![
            ((0, 0), 0),
            ((last, 0), 0),
            ((0, last), 1),
            ((last, last), 1),
        ]
    }

    /// Default 4-player corner goals (`AutomataflRules.stockGoals4`) for a `size × size` board.
    pub fn stock_goals4_for_size(size: usize) -> Vec<(Coord, u32)> {
        let last = size.saturating_sub(1) as i32;
        vec![
            ((0, 0), 0),
            ((last, 0), 1),
            ((last, last), 2),
            ((0, last), 3),
        ]
    }

    /// Return the stock opening `PackedBoard` for `preset` (`Blitz7`, `Tactical9`, `Classic11`),
    /// computed directly by `@[export] automatafl_packed_stock` in Lean.
    pub fn stock(preset: BoardPreset) -> Self {
        let mut buf = [0u8; MAX_PACKED_BYTES];
        let n = ffi_packed_stock(preset.to_byte(), &mut buf);
        assert!(
            n >= 60 && n <= MAX_PACKED_BYTES,
            "automatafl_packed_stock returned invalid byte length {n}"
        );
        Self {
            buf,
            len: n as u16,
        }
    }

    /// Construct a `PackedBoard` from raw serialized bytes (`PackedBoard.fromBytes?` format).
    /// Fail-closed on malformed lengths, invalid board sizes (`sz == 0 || sz > 11`), or
    /// out-of-bounds coordinates. Canonicalizes legacy buffers (missing the trailing `goals`
    /// section), `n_goals == 0` buffers, header flags, and `rep`/`att`/`mark` bitmasks
    /// (masking to `sz × sz`, clearing the automaton bit, and ensuring `rep` and `att` are
    /// disjoint, matching `sanitizeBoardMasks` in Lean) so `PartialEq` and `Hash` agree.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 60 || bytes.len() > MAX_PACKED_BYTES {
            return None;
        }
        let sz = bytes[48] as usize;
        if sz == 0 || sz > 11 {
            return None;
        }
        let ax = bytes[49] as usize;
        let ay = bytes[50] as usize;
        if ax >= sz || ay >= sz {
            return None;
        }
        let n_wait = bytes[57] as usize;
        let lock_off = 58 + n_wait;
        if lock_off >= bytes.len() {
            return None;
        }
        let n_lock = bytes[lock_off] as usize;
        let cd_off = lock_off + 1 + n_lock * 5;
        if cd_off >= bytes.len() {
            return None;
        }
        for i in 0..n_lock {
            let base = lock_off + 1 + i * 5;
            let fx = bytes[base + 1] as usize;
            let fy = bytes[base + 2] as usize;
            let tx = bytes[base + 3] as usize;
            let ty = bytes[base + 4] as usize;
            if fx >= sz || fy >= sz || tx >= sz || ty >= sz {
                return None;
            }
        }
        let n_cd = bytes[cd_off] as usize;
        let goal_off = cd_off + 1 + n_cd * 3;
        if goal_off > bytes.len() {
            return None;
        }
        for i in 0..n_cd {
            let base = cd_off + 1 + i * 3;
            let cx = bytes[base] as usize;
            let cy = bytes[base + 1] as usize;
            let turns = bytes[base + 2];
            if cx >= sz || cy >= sz || turns == 0 {
                return None;
            }
        }
        let mut buf = [0u8; MAX_PACKED_BYTES];
        let final_len =
            if goal_off == bytes.len() || (goal_off + 1 == bytes.len() && bytes[goal_off] == 0) {
                if goal_off + 13 > MAX_PACKED_BYTES {
                    return None;
                }
                buf[..goal_off].copy_from_slice(&bytes[..goal_off]);
                let fallback = Self::stock_goals2_for_size(sz);
                buf[goal_off] = fallback.len() as u8;
                let mut pos = goal_off + 1;
                for &(c, pid) in &fallback {
                    buf[pos] = u8::try_from(c.0).unwrap_or(255);
                    buf[pos + 1] = u8::try_from(c.1).unwrap_or(255);
                    buf[pos + 2] = pid as u8;
                    pos += 3;
                }
                pos
            } else {
                let n_goals = bytes[goal_off] as usize;
                if goal_off + 1 + n_goals * 3 != bytes.len() {
                    return None;
                }
                for i in 0..n_goals {
                    let base = goal_off + 1 + i * 3;
                    let gx = bytes[base] as usize;
                    let gy = bytes[base + 1] as usize;
                    if gx >= sz || gy >= sz {
                        return None;
                    }
                }
                buf[..bytes.len()].copy_from_slice(bytes);
                bytes.len()
            };
        let sanitize_mask = |mask: Bitmask128| -> Bitmask128 {
            let total = sz * sz;
            if total == 0 {
                Bitmask128::ZERO
            } else if total < 64 {
                Bitmask128 {
                    lo: mask.lo & ((1u64 << total) - 1),
                    hi: 0,
                }
            } else if total == 64 {
                Bitmask128 {
                    lo: mask.lo,
                    hi: 0,
                }
            } else if total < 128 {
                Bitmask128 {
                    lo: mask.lo,
                    hi: mask.hi & ((1u64 << (total - 64)) - 1),
                }
            } else {
                mask
            }
        };
        let mut rep = sanitize_mask(Bitmask128 {
            lo: read_u64_le(&buf, 0),
            hi: read_u64_le(&buf, 8),
        });
        let mut att = sanitize_mask(Bitmask128 {
            lo: read_u64_le(&buf, 16),
            hi: read_u64_le(&buf, 24),
        });
        let mk = sanitize_mask(Bitmask128 {
            lo: read_u64_le(&buf, 32),
            hi: read_u64_le(&buf, 40),
        });
        let auto_idx = ay * sz + ax;
        rep.clear_bit(auto_idx);
        att.clear_bit(auto_idx);
        att.lo &= !rep.lo;
        att.hi &= !rep.hi;
        write_u64_le(&mut buf, 0, rep.lo);
        write_u64_le(&mut buf, 8, rep.hi);
        write_u64_le(&mut buf, 16, att.lo);
        write_u64_le(&mut buf, 24, att.hi);
        write_u64_le(&mut buf, 32, mk.lo);
        write_u64_le(&mut buf, 40, mk.hi);
        buf[51] = TieBreak::from_byte(buf[51]).to_byte();
        buf[52] = ConfluenceMode::from_byte(buf[52]).to_byte();
        buf[53] &= 0b0000_0111;
        buf[55] = buf[55].max(1);
        buf[56] = Dir::opt_to_byte(Dir::opt_from_byte(buf[56]));
        Some(Self {
            buf,
            len: final_len as u16,
        })
    }

    /// Pack a [`Board`] and [`GameConfig`] (plus round state) with default 2-player stock goals.
    pub fn pack(
        board: &Board,
        cfg: &GameConfig,
        waiting: &[u32],
        marks: &[Coord],
        locked: &[Move],
        cooldowns: &[CooldownEntry],
        last_dir: Option<Dir>,
    ) -> Self {
        let goals = Self::stock_goals2_for_size(board.n);
        Self::pack_with_goals(
            board, cfg, &goals, waiting, marks, locked, cooldowns, last_dir,
        )
    }

    /// Pack a [`Board`], [`GameConfig`], custom `goals`, and round state into a `PackedBoard`.
    #[allow(clippy::too_many_arguments)]
    pub fn pack_with_goals(
        board: &Board,
        cfg: &GameConfig,
        goals: &[(Coord, u32)],
        waiting: &[u32],
        marks: &[Coord],
        locked: &[Move],
        cooldowns: &[CooldownEntry],
        last_dir: Option<Dir>,
    ) -> Self {
        let n = board.n;
        let fallback_goals;
        let eff_goals: &[(Coord, u32)] = if goals.is_empty() {
            fallback_goals = Self::stock_goals2_for_size(n);
            &fallback_goals
        } else {
            goals
        };

        let mut rep = Bitmask128::ZERO;
        let mut att = Bitmask128::ZERO;
        let mut mk = Bitmask128::ZERO;

        for y in 0..n {
            for x in 0..n {
                if (x as i32, y as i32) == board.auto {
                    continue;
                }
                let idx = y * n + x;
                match board.cell_at((x as i32, y as i32)) {
                    REP => rep.set_bit(idx),
                    ATT => att.set_bit(idx),
                    _ => {}
                }
            }
        }
        for &(mx, my) in marks {
            if mx >= 0 && (mx as usize) < n && my >= 0 && (my as usize) < n {
                mk.set_bit((my as usize) * n + (mx as usize));
            }
        }

        let mut buf = [0u8; MAX_PACKED_BYTES];
        write_u64_le(&mut buf, 0, rep.lo);
        write_u64_le(&mut buf, 8, rep.hi);
        write_u64_le(&mut buf, 16, att.lo);
        write_u64_le(&mut buf, 24, att.hi);
        write_u64_le(&mut buf, 32, mk.lo);
        write_u64_le(&mut buf, 40, mk.hi);
        buf[48] = n as u8;
        buf[49] = u8::try_from(board.auto.0).unwrap_or(255);
        buf[50] = u8::try_from(board.auto.1).unwrap_or(255);
        buf[51] = cfg.tie_break.to_byte();
        buf[52] = cfg.confluence.to_byte();
        let flags = (if board.col_rule { 1u8 } else { 0u8 })
            | (if cfg.two_cycle_swaps { 2u8 } else { 0u8 })
            | (if cfg.railgun_fling { 4u8 } else { 0u8 });
        buf[53] = flags;
        buf[54] = cfg.cooldown_turns;
        buf[55] = cfg.automata_count.max(1);
        buf[56] = Dir::opt_to_byte(last_dir);

        let mut pos = 57usize;
        let n_wait = waiting.len().min(u8::MAX as usize);
        assert!(
            pos + 1 + n_wait + 3 <= MAX_PACKED_BYTES,
            "PackedBoard waiting list exceeds MAX_PACKED_BYTES"
        );
        buf[pos] = n_wait as u8;
        pos += 1;
        for &p in &waiting[..n_wait] {
            buf[pos] = p as u8;
            pos += 1;
        }
        let n_lock = locked.len().min(u8::MAX as usize);
        assert!(
            pos + 1 + n_lock * 5 + 2 <= MAX_PACKED_BYTES,
            "PackedBoard locked list exceeds MAX_PACKED_BYTES"
        );
        buf[pos] = n_lock as u8;
        pos += 1;
        for m in &locked[..n_lock] {
            buf[pos] = m.who as u8;
            buf[pos + 1] = u8::try_from(m.frm.0).unwrap_or(255);
            buf[pos + 2] = u8::try_from(m.frm.1).unwrap_or(255);
            buf[pos + 3] = u8::try_from(m.to.0).unwrap_or(255);
            buf[pos + 4] = u8::try_from(m.to.1).unwrap_or(255);
            pos += 5;
        }
        let n_cd = cooldowns.len().min(u8::MAX as usize);
        assert!(
            pos + 1 + n_cd * 3 + 1 <= MAX_PACKED_BYTES,
            "PackedBoard cooldowns list exceeds MAX_PACKED_BYTES"
        );
        buf[pos] = n_cd as u8;
        pos += 1;
        for &(c, t) in &cooldowns[..n_cd] {
            buf[pos] = u8::try_from(c.0).unwrap_or(255);
            buf[pos + 1] = u8::try_from(c.1).unwrap_or(255);
            buf[pos + 2] = t as u8;
            pos += 3;
        }
        let n_goals = eff_goals.len().min(u8::MAX as usize);
        assert!(
            pos + 1 + n_goals * 3 <= MAX_PACKED_BYTES,
            "PackedBoard goals list exceeds MAX_PACKED_BYTES"
        );
        buf[pos] = n_goals as u8;
        pos += 1;
        for &(c, pid) in &eff_goals[..n_goals] {
            buf[pos] = u8::try_from(c.0).unwrap_or(255);
            buf[pos + 1] = u8::try_from(c.1).unwrap_or(255);
            buf[pos + 2] = pid as u8;
            pos += 3;
        }

        Self {
            buf,
            len: pos as u16,
        }
    }

    /// Borrow the serialized bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len as usize]
    }

    /// Return a `Vec<u8>` copy of the serialized bytes.
    #[inline]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Repulsor bitmask (`Bitmask128`).
    #[inline]
    pub fn rep_mask(&self) -> Bitmask128 {
        Bitmask128 {
            lo: read_u64_le(&self.buf, 0),
            hi: read_u64_le(&self.buf, 8),
        }
    }

    /// Attractor bitmask (`Bitmask128`).
    #[inline]
    pub fn att_mask(&self) -> Bitmask128 {
        Bitmask128 {
            lo: read_u64_le(&self.buf, 16),
            hi: read_u64_le(&self.buf, 24),
        }
    }

    /// Conflict marker bitmask (`Bitmask128`).
    #[inline]
    pub fn mark_mask(&self) -> Bitmask128 {
        Bitmask128 {
            lo: read_u64_le(&self.buf, 32),
            hi: read_u64_le(&self.buf, 40),
        }
    }

    /// Board edge size (`7`, `9`, or `11`).
    #[inline]
    pub fn size(&self) -> usize {
        self.buf[48] as usize
    }

    /// Automaton coordinate `(x, y)`.
    #[inline]
    pub fn auto_coord(&self) -> Coord {
        (self.buf[49] as i32, self.buf[50] as i32)
    }

    /// Whether `c` is in bounds.
    #[inline]
    pub fn in_bounds(&self, c: Coord) -> bool {
        let n = self.size() as i32;
        c.0 >= 0 && c.0 < n && c.1 >= 0 && c.1 < n
    }

    /// O(1) particle lookup directly from the bitmasks.
    #[inline]
    pub fn cell_at(&self, c: Coord) -> u8 {
        if !self.in_bounds(c) {
            return VAC;
        }
        if c == self.auto_coord() {
            return AUTO;
        }
        let idx = (c.1 as usize) * self.size() + (c.0 as usize);
        if self.rep_mask().test_bit(idx) {
            REP
        } else if self.att_mask().test_bit(idx) {
            ATT
        } else {
            VAC
        }
    }

    /// Extract the embedded [`GameConfig`].
    pub fn config(&self) -> GameConfig {
        let preset = match self.size() {
            7 => BoardPreset::Blitz7,
            9 => BoardPreset::Tactical9,
            _ => BoardPreset::Classic11,
        };
        let flg = self.buf[53];
        GameConfig {
            tie_break: TieBreak::from_byte(self.buf[51]),
            confluence: ConfluenceMode::from_byte(self.buf[52]),
            preset,
            two_cycle_swaps: (flg & 2) != 0,
            cooldown_turns: self.buf[54],
            railgun_fling: (flg & 4) != 0,
            automata_count: self.buf[55].max(1),
        }
    }

    /// Last automaton step direction (`Option<Dir>`).
    #[inline]
    pub fn last_dir(&self) -> Option<Dir> {
        Dir::opt_from_byte(self.buf[56])
    }

    /// Unpack to a [`Board`].
    pub fn to_board(&self) -> Board {
        let n = self.size();
        let rep = self.rep_mask();
        let att = self.att_mask();
        let auto = self.auto_coord();
        let mut cells = vec![VAC; n * n];
        for y in 0..n {
            for x in 0..n {
                let idx = y * n + x;
                if (x as i32, y as i32) == auto {
                    cells[idx] = AUTO;
                } else if rep.test_bit(idx) {
                    cells[idx] = REP;
                } else if att.test_bit(idx) {
                    cells[idx] = ATT;
                }
            }
        }
        Board {
            n,
            cells,
            auto,
            col_rule: (self.buf[53] & 1) != 0,
        }
    }

    /// Waiting seats in the current round.
    pub fn waiting(&self) -> Vec<u32> {
        let n_wait = self.buf[57] as usize;
        self.buf[58..58 + n_wait]
            .iter()
            .map(|&b| b as u32)
            .collect()
    }

    /// Marked coordinates in the current round.
    pub fn marks(&self) -> Vec<Coord> {
        self.mark_mask().to_coords(self.size())
    }

    /// Locked moves in the current round.
    pub fn locked(&self) -> Vec<Move> {
        let n_wait = self.buf[57] as usize;
        let lock_off = 58 + n_wait;
        let n_lock = self.buf[lock_off] as usize;
        let mut out = Vec::with_capacity(n_lock);
        for i in 0..n_lock {
            let base = lock_off + 1 + i * 5;
            out.push(Move {
                who: self.buf[base] as u32,
                frm: (self.buf[base + 1] as i32, self.buf[base + 2] as i32),
                to: (self.buf[base + 3] as i32, self.buf[base + 4] as i32),
            });
        }
        out
    }

    /// Active piece cooldowns.
    pub fn cooldowns(&self) -> Vec<CooldownEntry> {
        let n_wait = self.buf[57] as usize;
        let lock_off = 58 + n_wait;
        let n_lock = self.buf[lock_off] as usize;
        let cd_off = lock_off + 1 + n_lock * 5;
        let n_cd = self.buf[cd_off] as usize;
        let mut out = Vec::with_capacity(n_cd);
        for i in 0..n_cd {
            let base = cd_off + 1 + i * 3;
            out.push((
                (self.buf[base] as i32, self.buf[base + 1] as i32),
                self.buf[base + 2] as u32,
            ));
        }
        out
    }

    /// Goal assignment `((x, y), seat)` embedded in this `PackedBoard`.
    /// Falls back to [`PackedBoard::stock_goals2_for_size`] if deserialized from a legacy buffer without a goals trailer.
    pub fn goals(&self) -> Vec<(Coord, u32)> {
        let n_wait = self.buf[57] as usize;
        let lock_off = 58 + n_wait;
        let n_lock = self.buf[lock_off] as usize;
        let cd_off = lock_off + 1 + n_lock * 5;
        let n_cd = self.buf[cd_off] as usize;
        let goal_off = cd_off + 1 + n_cd * 3;
        if goal_off >= self.len as usize {
            return Self::stock_goals2_for_size(self.size());
        }
        let n_goals = self.buf[goal_off] as usize;
        if n_goals == 0 {
            return Self::stock_goals2_for_size(self.size());
        }
        let mut out = Vec::with_capacity(n_goals);
        for i in 0..n_goals {
            let base = goal_off + 1 + i * 3;
            if base + 2 < self.len as usize {
                out.push((
                    (self.buf[base] as i32, self.buf[base + 1] as i32),
                    self.buf[base + 2] as u32,
                ));
            }
        }
        out
    }

    /// Update the embedded `goals` in place (and if at the start of a fresh un-conflicted round,
    /// synchronize `waiting` seats with the deduplicated seats of `goals`).
    pub fn set_goals(&mut self, goals: &[(Coord, u32)]) {
        *self = self.with_goals(goals);
    }

    /// Return a copy of this `PackedBoard` with `goals` set to `goals`.
    /// If the round is fresh (`marks` and `locked` are empty), `waiting` is also updated to the
    /// deduplicated seats in `goals` (matching `openRound b g.seats` in Lean, where `List.dedup`
    /// keeps the last occurrence of each seat).
    pub fn with_goals(self, goals: &[(Coord, u32)]) -> Self {
        let board = self.to_board();
        let cfg = self.config();
        let marks = self.marks();
        let locked = self.locked();
        let cooldowns = self.cooldowns();
        let last_dir = self.last_dir();
        let waiting = if marks.is_empty() && locked.is_empty() {
            let mut seats = Vec::new();
            for (idx, &(_, pid)) in goals.iter().enumerate() {
                if !goals[idx + 1..].iter().any(|&(_, p)| p == pid) {
                    seats.push(pid);
                }
            }
            if seats.is_empty() {
                vec![0, 1]
            } else {
                seats
            }
        } else {
            self.waiting()
        };
        Self::pack_with_goals(
            &board, &cfg, goals, &waiting, &marks, &locked, &cooldowns, last_dir,
        )
    }

    /// Return a copy of this `PackedBoard` configured with the 4-player stock corner goals
    /// (`AutomataflRules.stockGoals4`: `(0,0)->0`, `(N-1,0)->1`, `(N-1,N-1)->2`, `(0,N-1)->3`)
    /// and waiting seats `[0, 1, 2, 3]` if at the start of a round.
    pub fn with_four_player_goals(self) -> Self {
        let goals = Self::stock_goals4_for_size(self.size());
        self.with_goals(&goals)
    }

    /// Return a copy of this `PackedBoard` with `waiting` seats overridden.
    pub fn with_waiting(self, waiting: &[u32]) -> Self {
        let board = self.to_board();
        let cfg = self.config();
        let goals = self.goals();
        let marks = self.marks();
        let locked = self.locked();
        let cooldowns = self.cooldowns();
        let last_dir = self.last_dir();
        Self::pack_with_goals(
            &board, &cfg, &goals, waiting, &marks, &locked, &cooldowns, last_dir,
        )
    }

    /// **Execute one round step on `PackedBoard`** via `@[export] automatafl_packed_step`
    /// (`PackedBoard.stepVariant` in Lean, using direct `Bitmask128` mutation via
    /// `resolveMoveMasksCfg` / `applyMoveMasks` and O(1)-depth `boardOfMasks` reification).
    ///
    /// Uses stack buffers on the Rust side (zero Rust heap allocations).
    pub fn step(&self, moves: &[Move]) -> Result<PackedOutcome, String> {
        let mut move_buf = [0u8; 128];
        if moves.len() * 5 > move_buf.len() {
            return Err(format!("too many moves ({}) for stack buffer", moves.len()));
        }
        for (i, m) in moves.iter().enumerate() {
            let base = i * 5;
            move_buf[base] = m.who as u8;
            move_buf[base + 1] = u8::try_from(m.frm.0).unwrap_or(255);
            move_buf[base + 2] = u8::try_from(m.frm.1).unwrap_or(255);
            move_buf[base + 3] = u8::try_from(m.to.0).unwrap_or(255);
            move_buf[base + 4] = u8::try_from(m.to.1).unwrap_or(255);
        }
        packed_step_raw(self.as_bytes(), &move_buf[..moves.len() * 5])
    }

    /// **Compute the 128-bit bitmask of legal destinations** from `frm` for seat `who`.
    ///
    /// Invokes `@[export] automatafl_packed_legal_targets` directly on the packed byte buffer.
    /// When `live_only == true`, filters out occluded moves and non-carrying sources in addition
    /// to marks and cooldowns; when `live_only == false`, returns all proposable rook-line
    /// destinations respecting marks and cooldowns.
    pub fn legal_targets_mask(&self, who: u32, frm: Coord, live_only: bool) -> Bitmask128 {
        if !self.in_bounds(frm) {
            return Bitmask128::ZERO;
        }
        let (lo, hi) = ffi_packed_legal_targets(
            self.as_bytes(),
            who as u8,
            frm.0 as u8,
            frm.1 as u8,
            u8::from(live_only),
        );
        Bitmask128 { lo, hi }
    }

    /// **Adjudicate a capped match** on this `PackedBoard` via `@[export] automatafl_packed_adjudicate`
    /// (`AutomataflRules.adjudicateCapped`).
    pub fn adjudicate(&self) -> Option<u32> {
        let w = ffi_packed_adjudicate(self.as_bytes());
        if w == 255 { None } else { Some(w) }
    }
}

/// Fail-closed raw byte entrypoint for `@[export] automatafl_packed_step`.
/// Validates `state` via [`PackedBoard::from_bytes`] and `moves` (`5 * k` bytes) before stepping.
/// When `state.len() < 60`, also exercises the underlying C/Lean `fromBytes?` fail-closed path.
pub fn packed_step_raw(state: &[u8], moves: &[u8]) -> Result<PackedOutcome, String> {
    let Some(pb) = PackedBoard::from_bytes(state) else {
        if state.len() < 60 {
            let mut out_buf = [0u8; 8];
            let _ = ffi_packed_step(state, &[], &mut out_buf);
        }
        return Err("packed_step_raw: malformed state bytes".to_string());
    };
    if moves.len() % 5 != 0 || moves.len() > 128 {
        return Err(format!(
            "packed_step_raw: invalid moves byte length {}",
            moves.len()
        ));
    }
    let mut out_buf = [0u8; MAX_PACKED_BYTES + 4];
    let out_len = ffi_packed_step(pb.as_bytes(), moves, &mut out_buf);
    if out_len < 3 || out_len > out_buf.len() {
        return Err(format!(
            "automatafl_packed_step returned malformed reply (len={out_len})"
        ));
    }
    match out_buf[0] {
        1 => {
            let next_pb = PackedBoard::from_bytes(&out_buf[2..out_len])
                .ok_or_else(|| "automatafl_packed_step Again payload malformed".to_string())?;
            Ok(PackedOutcome::Again(next_pb))
        }
        2 => {
            let win_byte = out_buf[1];
            let win = if win_byte == 255 {
                None
            } else {
                Some(win_byte as u32)
            };
            let next_pb = PackedBoard::from_bytes(&out_buf[2..out_len])
                .ok_or_else(|| "automatafl_packed_step Resolved payload malformed".to_string())?;
            Ok(PackedOutcome::Resolved {
                board: next_pb,
                win,
            })
        }
        tag => Err(format!("automatafl_packed_step unknown outcome tag {tag}")),
    }
}

/// Fail-closed raw byte entrypoint for `@[export] automatafl_packed_legal_targets`.
pub fn packed_legal_targets_raw(
    state: &[u8],
    who: u8,
    fx: u8,
    fy: u8,
    live_only: bool,
) -> Bitmask128 {
    let Some(pb) = PackedBoard::from_bytes(state) else {
        if state.len() < 60 {
            let _ = ffi_packed_legal_targets(state, who, fx, fy, u8::from(live_only));
        }
        return Bitmask128::ZERO;
    };
    pb.legal_targets_mask(who as u32, (fx as i32, fy as i32), live_only)
}

/// Fail-closed raw byte entrypoint for `@[export] automatafl_packed_adjudicate`.
pub fn packed_adjudicate_raw(state: &[u8]) -> Result<Option<u32>, String> {
    let Some(pb) = PackedBoard::from_bytes(state) else {
        if state.len() < 60 {
            let _ = ffi_packed_adjudicate(state);
        }
        return Err("packed_adjudicate_raw: malformed state bytes".to_string());
    };
    let w = ffi_packed_adjudicate(pb.as_bytes());
    match w {
        254 => Err("automatafl_packed_adjudicate rejected state bytes".to_string()),
        255 => Ok(None),
        seat => Ok(Some(seat)),
    }
}


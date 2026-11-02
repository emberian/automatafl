//! Core domain types, wire marshallers, and configurable variant settings (`GameConfig`)
//! for Automatafl across the server and multi-target clients.

use serde::{Deserialize, Serialize};

/// Vacuum — the only pass-through particle.
pub const VAC: u8 = 0;
/// Repulsor.
pub const REP: u8 = 1;
/// Attractor.
pub const ATT: u8 = 2;
/// The Automaton. Occupies a cell, occludes, and is the raycast origin.
pub const AUTO: u8 = 3;

/// Standard 11x11 Classic board edge.
pub const N11: usize = 11;
/// 9x9 Tactical board edge.
pub const N9: usize = 9;
/// 7x7 Blitz board edge.
pub const N7: usize = 7;
/// 5x5 Mini/testing board edge.
pub const N5: usize = 5;

/// A board coordinate `(x, y)`. Signed so callers can name off-board squares which the oracle rejects.
pub type Coord = (i32, i32);

/// Cardinal directions `(dx, dy)` in the order the Lean `sense` verb reports (`xp, xn, yp, yn`).
pub const XP: Coord = (1, 0);
pub const XN: Coord = (-1, 0);
pub const YP: Coord = (0, 1);
pub const YN: Coord = (0, -1);
pub const DIRS: [Coord; 4] = [XP, XN, YP, YN];

/// A player seat in the two-player game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Seat {
    /// Seat A (player 0): owns the `y = 0` corners `(0, 0)` and `(n - 1, 0)`.
    A,
    /// Seat B (player 1): owns the `y = n - 1` corners `(0, n - 1)` and `(n - 1, n - 1)`.
    B,
}

impl Seat {
    pub fn idx(self) -> usize {
        match self {
            Seat::A => 0,
            Seat::B => 1,
        }
    }

    pub fn pid(self) -> u32 {
        self.idx() as u32
    }

    pub fn other(self) -> Seat {
        match self {
            Seat::A => Seat::B,
            Seat::B => Seat::A,
        }
    }

    pub fn from_idx(who: u32) -> Seat {
        if who == 0 {
            Seat::A
        } else {
            Seat::B
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Seat::A => "A",
            Seat::B => "B",
        }
    }
}

/// Board size and layout preset (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoardPreset {
    /// 11x11 Classic two-player board.
    #[default]
    Classic11x11,
    /// 9x9 Tactical board.
    Tactical9x9,
    /// 7x7 Blitz board.
    Blitz7x7,
    /// 5x5 Mini testing board.
    Mini5x5,
}

impl BoardPreset {
    pub fn size(self) -> usize {
        match self {
            BoardPreset::Classic11x11 => N11,
            BoardPreset::Tactical9x9 => N9,
            BoardPreset::Blitz7x7 => N7,
            BoardPreset::Mini5x5 => N5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BoardPreset::Classic11x11 => "11×11 Classic",
            BoardPreset::Tactical9x9 => "9×9 Tactical",
            BoardPreset::Blitz7x7 => "7×7 Blitz",
            BoardPreset::Mini5x5 => "5×5 Mini",
        }
    }

    pub fn to_ai(self) -> automatafl_ai::BoardPreset {
        match self {
            BoardPreset::Classic11x11 => automatafl_ai::BoardPreset::Classic11x11,
            BoardPreset::Tactical9x9 => automatafl_ai::BoardPreset::Tactical9x9,
            BoardPreset::Blitz7x7 => automatafl_ai::BoardPreset::Blitz7x7,
            BoardPreset::Mini5x5 => automatafl_ai::BoardPreset::Mini5x5,
        }
    }
}

/// Automaton equal-priority tie-break mode (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreakMode {
    /// Prefer the Y axis (column rule, default).
    #[default]
    Column,
    /// Prefer the X axis (row rule).
    Row,
    /// Do not move on an equal-priority tie.
    Freeze,
    /// Momentum tie-break: prefer continuing along the same axis (or orthogonal axis if false) as the last step.
    Momentum {
        #[serde(default = "default_true")]
        prefer_same_axis: bool,
    },
}

fn default_true() -> bool {
    true
}

impl TieBreakMode {
    pub fn wire_token(self) -> &'static str {
        match self {
            TieBreakMode::Column => "0",
            TieBreakMode::Row => "1",
            TieBreakMode::Freeze => "2",
            TieBreakMode::Momentum {
                prefer_same_axis: true,
            } => "3",
            TieBreakMode::Momentum {
                prefer_same_axis: false,
            } => "4",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TieBreakMode::Column => "Column (prefer Y)",
            TieBreakMode::Row => "Row (prefer X)",
            TieBreakMode::Freeze => "Freeze (hold on tie)",
            TieBreakMode::Momentum {
                prefer_same_axis: true,
            } => "Momentum (same axis)",
            TieBreakMode::Momentum {
                prefer_same_axis: false,
            } => "Momentum (switch axis)",
        }
    }

    pub fn to_ai(self) -> automatafl_ai::TieBreakMode {
        match self {
            TieBreakMode::Column => automatafl_ai::TieBreakMode::Column,
            TieBreakMode::Row => automatafl_ai::TieBreakMode::Row,
            TieBreakMode::Freeze => automatafl_ai::TieBreakMode::Freeze,
            TieBreakMode::Momentum { prefer_same_axis } => {
                automatafl_ai::TieBreakMode::Momentum { prefer_same_axis }
            }
        }
    }
}

/// Confluence / merge resolution mode when multiple movers converge on the same square (R2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfluenceMode {
    /// Canonical conflict: mark the contested coordinate and re-open the round for colliding seats.
    #[default]
    Conflict,
    /// Stop 1 cell short of the collision point without re-entry (`MERGE_RESOLUTION_DESIGN.md`).
    BunchBefore,
    /// Colliding movers destroy each other.
    Annihilate,
}

impl ConfluenceMode {
    pub fn wire_token(self) -> &'static str {
        match self {
            ConfluenceMode::Conflict => "0",
            ConfluenceMode::BunchBefore => "1",
            ConfluenceMode::Annihilate => "2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConfluenceMode::Conflict => "Conflict (Mark & Re-enter)",
            ConfluenceMode::BunchBefore => "Bunch Before Merge",
            ConfluenceMode::Annihilate => "Annihilate",
        }
    }

    pub fn to_ai(self) -> automatafl_ai::ConfluenceMode {
        match self {
            ConfluenceMode::Conflict => automatafl_ai::ConfluenceMode::Conflict,
            ConfluenceMode::BunchBefore => automatafl_ai::ConfluenceMode::BunchBefore,
            ConfluenceMode::Annihilate => automatafl_ai::ConfluenceMode::Annihilate,
        }
    }
}

/// Commit-phase workflow mode for simultaneous moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommitMode {
    /// Players seal moves in secret; once all waiting seats have sealed, the round automatically reveals & resolves.
    /// (While waiting for the other seat, only the commitment hash is visible; coordinates are never leaked.)
    #[default]
    AutoRevealOnBothSealed,
    /// Strict three-step ladder (`Commit` -> `Reveal` -> `Resolve`): each player explicitly opens their seal
    /// after both have committed, and then triggers `Resolve`.
    ExplicitCommitReveal,
}

/// Full configurable game rules and variant parameters (R2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameConfig {
    #[serde(default)]
    pub preset: BoardPreset,
    #[serde(default)]
    pub tie_break: TieBreakMode,
    #[serde(default)]
    pub confluence: ConfluenceMode,
    #[serde(default)]
    pub two_cycle_swaps: bool,
    #[serde(default)]
    pub cooldown_turns: u32,
    #[serde(default)]
    pub railgun_fling: bool,
    #[serde(default = "default_one")]
    pub automata_count: u32,
    #[serde(default = "default_max_turns")]
    pub max_turns: u64,
    #[serde(default)]
    pub commit_mode: CommitMode,
}

fn default_one() -> u32 {
    1
}

fn default_max_turns() -> u64 {
    256
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
            max_turns: 256,
            commit_mode: CommitMode::AutoRevealOnBothSealed,
        }
    }
}

impl GameConfig {
    /// Encode `GameConfig` as the 6-token `AutomataflFFI.parseGameConfig` wire prefix:
    /// `tie conf twoCycleSwaps cooldownTurns railgunFling automataCount`.
    pub fn to_wire(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.tie_break.wire_token(),
            self.confluence.wire_token(),
            if self.two_cycle_swaps { 1 } else { 0 },
            self.cooldown_turns,
            if self.railgun_fling { 1 } else { 0 },
            self.automata_count.max(1)
        )
    }

    pub fn to_ai(&self) -> automatafl_ai::GameConfig {
        automatafl_ai::GameConfig {
            preset: self.preset.to_ai(),
            tie_break: self.tie_break.to_ai(),
            confluence: self.confluence.to_ai(),
            two_cycle_swaps: self.two_cycle_swaps,
            cooldown_turns: self.cooldown_turns.min(255) as u8,
            railgun_fling: self.railgun_fling,
            automata_count: self.automata_count.max(1).min(255) as u8,
            max_turns: self.max_turns.min(u16::MAX as u64) as u16,
        }
    }
}

/// A board position (`Board` from `dregg-automatafl`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    /// Grid dimension (`n x n`).
    pub n: usize,
    /// Row-major cells (`cells[y * n + x]` in `{0, 1, 2, 3}`).
    pub cells: Vec<u8>,
    /// Primary automaton coordinate `(x, y)`.
    pub auto: Coord,
    /// Column-rule flag (`true` for column tie-break).
    pub col_rule: bool,
}

/// A move: `who` moves a piece from `frm` to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Move {
    pub who: u32,
    pub frm: Coord,
    pub to: Coord,
}

/// One raycast result along a cardinal axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raycast {
    pub what: u8,
    pub dist: usize,
}

/// One axis decision from `evaluateAxis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// `0 = none, 1 = towardAttractor, 2 = fromRepulsor, 3 = unbalancedPair`.
    pub variant: u8,
    /// True if the decision points in the positive axis direction.
    pub pos: bool,
    pub att_dist: usize,
    pub rep_dist: usize,
}

impl Decision {
    pub const NONE: Decision = Decision {
        variant: 0,
        pos: false,
        att_dist: 0,
        rep_dist: 0,
    };

    pub fn priority(&self) -> usize {
        (self.variant as usize) * 10
    }
}

/// Complete automaton sense telemetry on a board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomatonSense {
    /// `[xp, xn, yp, yn]`.
    pub rays: [Raycast; 4],
    pub x_dec: Decision,
    pub y_dec: Decision,
    pub offset: Coord,
}

/// Goal distances and capped-match adjudication verdict (`Standing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Each seat's Manhattan distance to its nearest owned goal corner.
    pub dist: [Option<u32>; 2],
    /// Which seat would win if adjudicated right now (`None` = dead heat).
    pub verdict: Option<u32>,
}

/// Outcome of a single round (`AutomataflRules.roundStep`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundOutcome {
    /// The round clashed: contested squares are marked, non-clashing moves lock, clashing seats re-enter.
    Again {
        marks: Vec<Coord>,
        locked: Vec<Move>,
        waiting: Vec<u32>,
    },
    /// The round resolved cleanly: moves applied, automaton stepped, win checked on entry.
    Resolved {
        board: Board,
        win: Option<u32>,
    },
}

/// One of the automaton's four sightlines for UI rendering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sightline {
    pub dir: String,
    pub step: Coord,
    pub dist: usize,
    pub what: u8,
    pub end: Coord,
}

impl Sightline {
    pub fn caps_a_piece(&self) -> bool {
        self.what == ATT || self.what == REP
    }
}

impl Board {
    pub fn cell_at(&self, c: Coord) -> u8 {
        if self.in_bounds(c) {
            self.cells[(c.1 as usize) * self.n + (c.0 as usize)]
        } else {
            VAC
        }
    }

    pub fn in_bounds(&self, c: Coord) -> bool {
        let (x, y) = c;
        x >= 0 && (x as usize) < self.n && y >= 0 && (y as usize) < self.n
    }

    pub fn idx(&self, c: Coord) -> usize {
        (c.1 as usize) * self.n + (c.0 as usize)
    }

    pub fn coord_of(&self, idx: usize) -> Coord {
        ((idx % self.n) as i32, (idx / self.n) as i32)
    }

    pub fn tie_token(&self) -> &'static str {
        if self.col_rule {
            "0"
        } else {
            "1"
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.cells.len() != self.n * self.n {
            return Err(format!(
                "board carries {} cells for size {}",
                self.cells.len(),
                self.n
            ));
        }
        for (i, &code) in self.cells.iter().enumerate() {
            if code > AUTO {
                return Err(format!(
                    "board cell {i} holds {code}, outside the particle alphabet {{0,1,2,3}}"
                ));
            }
        }
        if !self.in_bounds(self.auto) {
            return Err(format!("the automaton at {:?} is off the board", self.auto));
        }
        if self.cell_at(self.auto) != AUTO {
            return Err(format!(
                "the automaton at {:?} does not carry its own code (cell = {})",
                self.auto,
                self.cell_at(self.auto)
            ));
        }
        Ok(())
    }

    pub fn to_wire(&self) -> String {
        let mut s = String::with_capacity(self.cells.len() + 24);
        s.push_str(&self.n.to_string());
        s.push(' ');
        if self.cells.is_empty() {
            s.push('-');
        } else {
            for c in &self.cells {
                s.push(char::from(b'0' + (c & 3)));
            }
        }
        s.push(' ');
        s.push_str(&self.auto.0.to_string());
        s.push(' ');
        s.push_str(&self.auto.1.to_string());
        s.push(' ');
        s.push_str(if self.col_rule { "1" } else { "0" });
        s
    }

    pub fn wire_coord(c: Coord) -> String {
        format!("{} {}", c.0, c.1)
    }

    pub fn from_wire<'a, 'b>(toks: &'a [&'b str]) -> Result<(Board, &'a [&'b str]), String> {
        if toks.len() < 5 {
            return Err(format!("board wire needs 5 tokens, got {}", toks.len()));
        }
        let n: usize = toks[0]
            .parse()
            .map_err(|e| format!("board size {:?}: {e}", toks[0]))?;
        let cells: Vec<u8> = if toks[1] == "-" {
            Vec::new()
        } else {
            toks[1]
                .bytes()
                .map(|b| {
                    if b.is_ascii_digit() && b - b'0' < 4 {
                        Ok(b - b'0')
                    } else {
                        Err(format!("cell digit {:?} outside {{0,1,2,3}}", b as char))
                    }
                })
                .collect::<Result<_, _>>()?
        };
        if cells.len() != n * n {
            return Err(format!(
                "board wire carries {} cells for size {n}",
                cells.len()
            ));
        }
        let ax: usize = toks[2]
            .parse()
            .map_err(|e| format!("auto x {:?}: {e}", toks[2]))?;
        let ay: usize = toks[3]
            .parse()
            .map_err(|e| format!("auto y {:?}: {e}", toks[3]))?;
        let col_num: usize = toks[4]
            .parse()
            .map_err(|e| format!("colRule {:?}: {e}", toks[4]))?;
        let col = col_num != 0;
        Ok((
            Board {
                n,
                cells,
                auto: (ax as i32, ay as i32),
                col_rule: col,
            },
            &toks[5..],
        ))
    }
}

pub fn coords_wire(cs: &[Coord]) -> String {
    let mut s = cs.len().to_string();
    for c in cs {
        s.push(' ');
        s.push_str(&Board::wire_coord(*c));
    }
    s
}

pub fn moves_wire(ms: &[Move]) -> String {
    let mut s = ms.len().to_string();
    for m in ms {
        s.push(' ');
        s.push_str(&m.who.to_string());
        s.push(' ');
        s.push_str(&Board::wire_coord(m.frm));
        s.push(' ');
        s.push_str(&Board::wire_coord(m.to));
    }
    s
}

pub fn goals_wire(g: &[(Coord, u32)]) -> String {
    let mut s = g.len().to_string();
    for (c, seat) in g {
        s.push(' ');
        s.push_str(&Board::wire_coord(*c));
        s.push(' ');
        s.push_str(&seat.to_string());
    }
    s
}

pub fn pids_wire(ps: &[u32]) -> String {
    let mut s = ps.len().to_string();
    for p in ps {
        s.push(' ');
        s.push_str(&p.to_string());
    }
    s
}

pub fn coords_from_wire<'a, 'b>(
    toks: &'a [&'b str],
) -> Result<(Vec<Coord>, &'a [&'b str]), String> {
    let (k, mut rest) = match toks.split_first() {
        Some((kt, rest)) => (
            kt.parse::<usize>()
                .map_err(|e| format!("coord count {kt:?}: {e}"))?,
            rest,
        ),
        None => return Err("coord list wire is empty".into()),
    };
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        if rest.len() < 2 {
            return Err("coord list wire is truncated".into());
        }
        let x: i32 = rest[0]
            .parse()
            .map_err(|e| format!("coord x {:?}: {e}", rest[0]))?;
        let y: i32 = rest[1]
            .parse()
            .map_err(|e| format!("coord y {:?}: {e}", rest[1]))?;
        out.push((x, y));
        rest = &rest[2..];
    }
    Ok((out, rest))
}

pub fn moves_from_wire<'a, 'b>(toks: &'a [&'b str]) -> Result<(Vec<Move>, &'a [&'b str]), String> {
    let (k, mut rest) = match toks.split_first() {
        Some((kt, rest)) => (
            kt.parse::<usize>()
                .map_err(|e| format!("move count {kt:?}: {e}"))?,
            rest,
        ),
        None => return Err("move list wire is empty".into()),
    };
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        if rest.len() < 5 {
            return Err("move list wire is truncated".into());
        }
        let num = |t: &str| -> Result<i32, String> {
            t.parse::<i32>()
                .map_err(|e| format!("move field {t:?}: {e}"))
        };
        out.push(Move {
            who: rest[0]
                .parse()
                .map_err(|e| format!("move seat {:?}: {e}", rest[0]))?,
            frm: (num(rest[1])?, num(rest[2])?),
            to: (num(rest[3])?, num(rest[4])?),
        });
        rest = &rest[5..];
    }
    Ok((out, rest))
}

pub fn pids_from_wire<'a, 'b>(toks: &'a [&'b str]) -> Result<(Vec<u32>, &'a [&'b str]), String> {
    let (k, mut rest) = match toks.split_first() {
        Some((kt, rest)) => (
            kt.parse::<usize>()
                .map_err(|e| format!("seat count {kt:?}: {e}"))?,
            rest,
        ),
        None => return Err("seat list wire is empty".into()),
    };
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        match rest.split_first() {
            Some((t, tail)) => {
                out.push(t.parse().map_err(|e| format!("seat {t:?}: {e}"))?);
                rest = tail;
            }
            None => return Err("seat list wire is truncated".into()),
        }
    }
    Ok((out, rest))
}

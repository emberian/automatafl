//! R2 GameConfig generalizations & variants (`TieBreak`, `ConfluenceMode`, `BoardPreset`, `GameConfig`).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::board::{Board, Coord};

/// Automaton equal-priority tie-break mode (`AutomataflRules.TieBreak`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TieBreak {
    /// Prefer Y axis (`0` on the wire) — canonical deployed default.
    #[default]
    Column,
    /// Prefer X axis (`1` on the wire).
    Row,
    /// Freeze (`(0, 0)`) on equal priority (`2` on the wire).
    Freeze,
    /// Momentum tie-break: prefer same axis as `last_dir` when `prefer_same_axis == true` (`3`),
    /// or perpendicular axis when `false` (`4`). Falls back to `Column` when `last_dir == None`.
    Momentum { prefer_same_axis: bool },
}

impl TieBreak {
    /// Wire token (`"0"`..`"4"`).
    pub fn to_wire_token(self) -> &'static str {
        match self {
            TieBreak::Column => "0",
            TieBreak::Row => "1",
            TieBreak::Freeze => "2",
            TieBreak::Momentum {
                prefer_same_axis: true,
            } => "3",
            TieBreak::Momentum {
                prefer_same_axis: false,
            } => "4",
        }
    }

    /// Packed byte representation (`0`..`4`).
    pub fn to_byte(self) -> u8 {
        match self {
            TieBreak::Column => 0,
            TieBreak::Row => 1,
            TieBreak::Freeze => 2,
            TieBreak::Momentum {
                prefer_same_axis: true,
            } => 3,
            TieBreak::Momentum {
                prefer_same_axis: false,
            } => 4,
        }
    }

    /// Decode from packed byte (`0`..`4`).
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => TieBreak::Column,
            1 => TieBreak::Row,
            2 => TieBreak::Freeze,
            3 => TieBreak::Momentum {
                prefer_same_axis: true,
            },
            4 => TieBreak::Momentum {
                prefer_same_axis: false,
            },
            _ => TieBreak::Column,
        }
    }
}

/// Move confluence & collision resolution mode (`AutomataflRules.ConfluenceMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConfluenceMode {
    /// Canonical mark-and-re-enter on conflicts (`0` on the wire).
    #[default]
    Conflict,
    /// Stop 1 cell short of contested merge point `M` without re-entry per `MERGE_RESOLUTION_DESIGN.md` (`1` on the wire).
    BunchBefore,
    /// Colliding movers annihilate each other without re-entry (`2` on the wire).
    Annihilate,
}

impl ConfluenceMode {
    /// Wire token (`"0"`..`"2"`).
    pub fn to_wire_token(self) -> &'static str {
        match self {
            ConfluenceMode::Conflict => "0",
            ConfluenceMode::BunchBefore => "1",
            ConfluenceMode::Annihilate => "2",
        }
    }

    /// Packed byte representation (`0`..`2`).
    pub fn to_byte(self) -> u8 {
        match self {
            ConfluenceMode::Conflict => 0,
            ConfluenceMode::BunchBefore => 1,
            ConfluenceMode::Annihilate => 2,
        }
    }

    /// Decode from packed byte (`0`..`2`).
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => ConfluenceMode::Conflict,
            1 => ConfluenceMode::BunchBefore,
            2 => ConfluenceMode::Annihilate,
            _ => ConfluenceMode::Conflict,
        }
    }
}

/// Board size preset (`AutomataflRules.BoardPreset`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BoardPreset {
    /// 7×7 Blitz board (`0` / `"7"`).
    Blitz7,
    /// 9×9 Tactical board (`1` / `"9"`).
    Tactical9,
    /// 11×11 Classic board (`2` / `"11"`).
    #[default]
    Classic11,
}

impl BoardPreset {
    /// Board edge size (`7`, `9`, or `11`).
    pub fn size(self) -> usize {
        match self {
            BoardPreset::Blitz7 => 7,
            BoardPreset::Tactical9 => 9,
            BoardPreset::Classic11 => 11,
        }
    }

    /// Packed preset byte (`0` = Blitz7, `1` = Tactical9, `2` = Classic11).
    pub fn to_byte(self) -> u8 {
        match self {
            BoardPreset::Blitz7 => 0,
            BoardPreset::Tactical9 => 1,
            BoardPreset::Classic11 => 2,
        }
    }

    /// Wire token (`"7"`, `"9"`, `"11"`).
    pub fn to_wire_token(self) -> &'static str {
        match self {
            BoardPreset::Blitz7 => "7",
            BoardPreset::Tactical9 => "9",
            BoardPreset::Classic11 => "11",
        }
    }
}

/// Cardinal direction (`Automatafl.Dir`), used for momentum tracking (`last_dir`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Dir {
    /// `+x` (`1`).
    Xp,
    /// `-x` (`2`).
    Xn,
    /// `+y` (`3`).
    Yp,
    /// `-y` (`4`).
    Yn,
}

impl Dir {
    /// Encode `Option<Dir>` as a wire token (`"0"`..`"4"`).
    pub fn opt_to_wire(d: Option<Dir>) -> &'static str {
        match d {
            None => "0",
            Some(Dir::Xp) => "1",
            Some(Dir::Xn) => "2",
            Some(Dir::Yp) => "3",
            Some(Dir::Yn) => "4",
        }
    }

    /// Encode `Option<Dir>` as a byte (`0`..`4`).
    pub fn opt_to_byte(d: Option<Dir>) -> u8 {
        match d {
            None => 0,
            Some(Dir::Xp) => 1,
            Some(Dir::Xn) => 2,
            Some(Dir::Yp) => 3,
            Some(Dir::Yn) => 4,
        }
    }

    /// Decode `Option<Dir>` from a byte (`0`..`4`).
    pub fn opt_from_byte(b: u8) -> Option<Dir> {
        match b {
            1 => Some(Dir::Xp),
            2 => Some(Dir::Xn),
            3 => Some(Dir::Yp),
            4 => Some(Dir::Yn),
            _ => None,
        }
    }

    /// Decode `Option<Dir>` from a wire token (`"0"`..`"4"`).
    pub fn opt_from_wire(tok: &str) -> Result<Option<Dir>, String> {
        match tok {
            "0" => Ok(None),
            "1" => Ok(Some(Dir::Xp)),
            "2" => Ok(Some(Dir::Xn)),
            "3" => Ok(Some(Dir::Yp)),
            "4" => Ok(Some(Dir::Yn)),
            other => Err(format!("invalid OptDir token {other:?}")),
        }
    }
}

/// Complete game configuration (`AutomataflRules.GameConfig`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GameConfig {
    /// Equal-priority tie-break mode.
    pub tie_break: TieBreak,
    /// Move confluence resolution mode (`Conflict`, `BunchBefore`, `Annihilate`).
    pub confluence: ConfluenceMode,
    /// Board preset (`Blitz7`, `Tactical9`, `Classic11`).
    pub preset: BoardPreset,
    /// Whether 2-cycles (`A -> B` and `B -> A`) swap instead of staying put.
    pub two_cycle_swaps: bool,
    /// Number of cooldown turns imposed on a piece after moving (`0` = disabled).
    pub cooldown_turns: u8,
    /// Whether the automaton flings across consecutive empty cells in its step direction.
    pub railgun_fling: bool,
    /// Number of automaton steps per turn (`>= 1`).
    pub automata_count: u8,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            tie_break: TieBreak::Column,
            confluence: ConfluenceMode::Conflict,
            preset: BoardPreset::Classic11,
            two_cycle_swaps: false,
            cooldown_turns: 0,
            railgun_fling: false,
            automata_count: 1,
        }
    }
}

impl GameConfig {
    /// Encode `GameConfig` as the 6-token wire format expected by `parseGameConfig` in `FFI.lean`:
    /// `tie conf twoCycleSwaps cooldownTurns railgunFling automataCount`.
    pub fn to_wire(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.tie_break.to_wire_token(),
            self.confluence.to_wire_token(),
            if self.two_cycle_swaps { 1 } else { 0 },
            self.cooldown_turns,
            if self.railgun_fling { 1 } else { 0 },
            self.automata_count.max(1)
        )
    }
}

/// Active piece cooldown entry `(coord, remaining_turns)`.
pub type CooldownEntry = (Coord, u32);

/// Encode a cooldown list: `k` followed by `k × [x y turns]`.
pub fn cooldowns_wire(cds: &[CooldownEntry]) -> String {
    let mut s = cds.len().to_string();
    for (c, t) in cds {
        s.push(' ');
        s.push_str(&Board::wire_coord(*c));
        s.push(' ');
        s.push_str(&t.to_string());
    }
    s
}

/// Decode a length-prefixed cooldown list from `toks`.
pub fn cooldowns_from_wire<'a, 'b>(
    toks: &'a [&'b str],
) -> Result<(Vec<CooldownEntry>, &'a [&'b str]), String> {
    let (k, mut rest) = match toks.split_first() {
        Some((kt, rest)) => (
            kt.parse::<usize>()
                .map_err(|e| format!("cooldown count {kt:?}: {e}"))?,
            rest,
        ),
        None => return Err("cooldown list wire is empty".into()),
    };
    let mut out = Vec::with_capacity(k);
    for _ in 0..k {
        if rest.len() < 3 {
            return Err("cooldown list wire is truncated".into());
        }
        let x: i32 = rest[0]
            .parse()
            .map_err(|e| format!("cooldown x {:?}: {e}", rest[0]))?;
        let y: i32 = rest[1]
            .parse()
            .map_err(|e| format!("cooldown y {:?}: {e}", rest[1]))?;
        let t: u32 = rest[2]
            .parse()
            .map_err(|e| format!("cooldown turns {:?}: {e}", rest[2]))?;
        out.push(((x, y), t));
        rest = &rest[3..];
    }
    Ok((out, rest))
}

//! The Automatafl board as data, and the wire marshalling to the verified Lean 4 core.
//!
//! This module holds no game rules: it defines the state shape (`Board`, `Move`, `Coord`,
//! particle codes), pure coordinate indexing (`cell_at`, `in_bounds`, `idx`), and the wire
//! marshallers for `@[export] dregg_automatafl_rules`.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Vacuum — the only pass-through particle.
pub const VAC: u8 = 0;
/// Repulsor.
pub const REP: u8 = 1;
/// Attractor.
pub const ATT: u8 = 2;
/// The Automaton. Occupies a cell, occludes, and can be raycast from.
pub const AUTO: u8 = 3;

/// 7×7 Blitz board edge.
pub const N7: usize = 7;
/// 9×9 Tactical board edge.
pub const N9: usize = 9;
/// 11×11 Classic board edge.
pub const N11: usize = 11;

/// A board coordinate `(x, y)`. Signed so callers can name off-board squares; the Lean
/// oracle answers `illegal` for negative coordinates via its out-of-bounds sentinel.
pub type Coord = (i32, i32);

/// A board position.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Board {
    /// Board edge `n`; the grid is `n × n`.
    pub n: usize,
    /// `cells[y * n + x]` in `{0, 1, 2, 3}`. The automaton's cell also holds [`AUTO`].
    pub cells: Vec<u8>,
    /// The automaton's coordinate.
    pub auto: Coord,
    /// Equal-priority tie-break flag on the board wire (`true` = column rule, `false` = freeze).
    pub col_rule: bool,
}

/// A move: seat `who` moves a piece from `frm` to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Move {
    /// The seat that submitted the move.
    pub who: u32,
    /// The source square.
    pub frm: Coord,
    /// The destination square.
    pub to: Coord,
}

/// Cardinal directions `(dx, dy)` in the order reported by the Lean `sense` verb.
pub const XP: Coord = (1, 0);
/// `-x` direction.
pub const XN: Coord = (-1, 0);
/// `+y` direction.
pub const YP: Coord = (0, 1);
/// `-y` direction.
pub const YN: Coord = (0, -1);
/// `[XP, XN, YP, YN]`.
pub const DIRS: [Coord; 4] = [XP, XN, YP, YN];

/// One raycast result as reported by Lean: the first non-vacuum particle hit (or [`VAC`] at
/// the wall) and the step distance at which the scan terminated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Raycast {
    /// Particle code hit, or [`VAC`] for the wall.
    pub what: u8,
    /// Step index at which the scan terminated.
    pub dist: usize,
}

/// One axis decision as reported by Lean (`0 = none`, `1 = towardAttractor`,
/// `2 = fromRepulsor`, `3 = unbalancedPair`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Decision {
    /// `0 none · 1 towardAttractor · 2 fromRepulsor · 3 unbalancedPair`.
    pub variant: u8,
    /// Whether the decision points along the axis-positive direction.
    pub pos: bool,
    /// Attractor distance (`0` if the variant carries none).
    pub att_dist: usize,
    /// Repulsor distance (`0` if the variant carries none).
    pub rep_dist: usize,
}

impl Decision {
    /// All-zero `none` decision.
    pub const NONE: Decision = Decision {
        variant: 0,
        pos: false,
        att_dist: 0,
        rep_dist: 0,
    };

    /// Priority number (`Decision.priority`): `variant × 10`.
    pub fn priority(&self) -> usize {
        (self.variant as usize) * 10
    }
}

/// The automaton's whole decision on a board (`sense` verb): four raycasts, two axis
/// decisions, and the chosen one-step offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AutomatonSense {
    /// `[xp, xn, yp, yn]`.
    pub rays: [Raycast; 4],
    /// X-axis decision.
    pub x_dec: Decision,
    /// Y-axis decision.
    pub y_dec: Decision,
    /// Chosen offset `(dx, dy)`.
    pub offset: Coord,
}

impl Board {
    /// Construct an empty `n × n` testing board with the automaton at `(n / 2, n / 2)`.
    pub fn empty_with_size(n: usize, col_rule: bool) -> Self {
        let mut cells = vec![VAC; n * n];
        let c = (n / 2) as i32;
        if n > 0 {
            cells[(c as usize) * n + (c as usize)] = AUTO;
        }
        Self {
            n,
            cells,
            auto: (c, c),
            col_rule,
        }
    }

    /// The particle at `c`; returns [`VAC`] outside the board (`Board.cellAt`).
    pub fn cell_at(&self, c: Coord) -> u8 {
        if self.in_bounds(c) {
            self.cells[(c.1 as usize) * self.n + (c.0 as usize)]
        } else {
            VAC
        }
    }

    /// Place a particle `p` at in-bounds coordinate `c`, updating `self.auto` if `p == AUTO`.
    pub fn set_cell(&mut self, c: Coord, p: u8) {
        if self.in_bounds(c) {
            let i = self.idx(c);
            self.cells[i] = p;
            if p == AUTO {
                self.auto = c;
            }
        }
    }

    /// Is `c` within `[0, n) × [0, n)`?
    pub fn in_bounds(&self, c: Coord) -> bool {
        let (x, y) = c;
        x >= 0 && (x as usize) < self.n && y >= 0 && (y as usize) < self.n
    }

    /// Row-major index `y * n + x` of an in-bounds coordinate.
    pub fn idx(&self, c: Coord) -> usize {
        (c.1 as usize) * self.n + (c.0 as usize)
    }

    /// Tie-break token denoted by `col_rule`: `"0"` (`TieBreak.column`) or `"2"` (`TieBreak.freeze`).
    pub fn tie_token(&self) -> &'static str {
        if self.col_rule { "0" } else { "2" }
    }

    /// Data-shape check ensuring the board can be encoded faithfully on the wire.
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

    /// Five-token board wire: `size cells autoX autoY colRule`.
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

    /// Format a coordinate as two space-separated wire tokens.
    pub fn wire_coord(c: Coord) -> String {
        format!("{} {}", c.0, c.1)
    }

    /// Decode a board from `toks`, returning the decoded board and remaining tokens.
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
        let ax: i32 = toks[2]
            .parse()
            .map_err(|e| format!("auto x {:?}: {e}", toks[2]))?;
        let ay: i32 = toks[3]
            .parse()
            .map_err(|e| format!("auto y {:?}: {e}", toks[3]))?;
        let col = toks[4] != "0";
        Ok((
            Board {
                n,
                cells,
                auto: (ax, ay),
                col_rule: col,
            },
            &toks[5..],
        ))
    }
}

/// Encode a length-prefixed coordinate list: `k` then `k × [x y]`.
pub fn coords_wire(cs: &[Coord]) -> String {
    let mut s = cs.len().to_string();
    for c in cs {
        s.push(' ');
        s.push_str(&Board::wire_coord(*c));
    }
    s
}

/// Encode a length-prefixed move list: `k` then `k × [who fromX fromY toX toY]`.
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

/// Encode a length-prefixed goal assignment: `k` then `k × [x y seat]`.
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

/// Encode a length-prefixed seat list: `k` then `k × [pid]`.
pub fn pids_wire(ps: &[u32]) -> String {
    let mut s = ps.len().to_string();
    for p in ps {
        s.push(' ');
        s.push_str(&p.to_string());
    }
    s
}

/// Decode a length-prefixed coordinate list from `toks`.
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

/// Decode a length-prefixed move list from `toks`.
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

/// Decode a length-prefixed seat list from `toks`.
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

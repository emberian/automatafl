//! Lean-backed game state and board adapter for `automatafl-v8n`.
//!
//! Replaces the legacy `automatafl-logic` crate by delegating all rule evaluations
//! (stock board setup, raycasting, automaton decision & stepping, move legality,
//! conflict detection, and simultaneous move resolution) directly to the formally
//! verified Lean 4 core via `automatafl_lean::rules_wire`.

use ndarray::Array2 as Grid;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub enum CoordFeedback {
    Ok,
    Conflict,
    Oob,
    Automaton,
}

impl core::fmt::Display for CoordFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            CoordFeedback::Ok => write!(f, "is OK"),
            CoordFeedback::Conflict => write!(f, "is conflicted"),
            CoordFeedback::Oob => write!(f, "is not on the board"),
            CoordFeedback::Automaton => write!(f, "is the automaton, which is off-limits"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveFeedback {
    SeeCoords(CoordsFeedback),
    MustMove,
    AxisAlignedOnly,
    WaitYourTurn,
    GameOver,
}

impl core::fmt::Display for MoveFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            MoveFeedback::SeeCoords(cfs) => write!(f, "had some problems: {cfs}"),
            MoveFeedback::MustMove => write!(f, "must have different source and destination squares"),
            MoveFeedback::AxisAlignedOnly => {
                write!(f, "must move the piece only along a row or column (like a chess Rook)")
            }
            MoveFeedback::WaitYourTurn => {
                write!(f, "cannot be performed while other players are resolving conflicts")
            }
            MoveFeedback::GameOver => write!(f, "doesn't matter once the game is over"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposeFeedback {
    Accepted,
    AcceptedAndReady,
    Rejected(MoveFeedback),
}

impl core::fmt::Display for ProposeFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            ProposeFeedback::Accepted => write!(f, "is now pending waiting for the other player"),
            ProposeFeedback::AcceptedAndReady => {
                write!(f, "is now pending and all players have submitted moves")
            }
            ProposeFeedback::Rejected(fb) => write!(f, "is rejected: {fb}, try again"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundState {
    Fresh,
    PartiallySubmitted,
    ResolvingConflict,
    GameOver,
}

impl core::fmt::Display for RoundState {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            RoundState::Fresh => write!(f, "not yet started"),
            RoundState::PartiallySubmitted => write!(f, "has players waiting"),
            RoundState::ResolvingConflict => write!(f, "is resolving conflicts"),
            RoundState::GameOver => write!(f, "is over"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveResult {
    NoSource,
    OccupiedAt(Coord),
    Applied,
}

impl core::fmt::Display for MoveResult {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            MoveResult::NoSource => {
                write!(f, "failed because there was never a piece to move at the source.")
            }
            MoveResult::OccupiedAt(c) => write!(
                f,
                "failed because the move is occluded between source and destination by a piece at {c}."
            ),
            MoveResult::Applied => write!(f, "applied!"),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeResolutionMode {
    DetectAndConflict,
    Annihilate,
    BunchBeforeMerge,
    BunchedStacking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleBehaviorMode {
    RotatePieces,
    NoMovement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pid(pub u8);

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct CoordsFeedback {
    pub data: SmallVec<[(Coord, CoordFeedback); 2]>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Particle {
    Repulsor,
    Attractor,
    Automaton,
    Vacuum,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub what: Particle,
    pub conflict: bool,
    pub passable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Raycast {
    pub what: Particle,
    pub hit: Option<Coord>,
    pub dist: usize,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub particles: Grid<Cell>,
    pub size: Coord,
    pub automaton_location: Coord,
    pub conflict_list: SmallVec<[Coord; 16]>,
    pub passable_list: SmallVec<[Coord; 16]>,
}

impl core::fmt::Display for CoordsFeedback {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for (coord, feedback) in &self.data {
            write!(f, "{} {}", coord, feedback)?;
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
            dx: self.x as i16 - other.x as i16,
            dy: self.y as i16 - other.y as i16,
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
        self.dx == 0 || (self.dy == 0 && !self.is_zero())
    }

    pub fn axial_unit(self) -> Delta {
        if self.is_zero() {
            Delta::ZERO
        } else if self.dx.abs() > self.dy.abs() {
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
        !(self.what.is_vacuum() || self.passable)
    }
}

impl core::fmt::Debug for Board {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
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
        for y in (0..self.size.y).rev() {
            write!(f, "{:2}│", y)?;
            for x in 0..self.size.x {
                let cell = self.particles[Coord { x, y }.ix()];
                let conflict_char = if cell.conflict { '!' } else { ' ' };
                let passable_char = if cell.passable { '~' } else { ' ' };
                let particle_char = match cell.what {
                    Particle::Repulsor => 'R',
                    Particle::Attractor => 'A',
                    Particle::Automaton => 'D',
                    Particle::Vacuum => '.',
                };
                write!(f, "{}{}{}", conflict_char, particle_char, passable_char)?;
            }
            writeln!(f, "│")?;
        }
        write!(f, "  └")?;
        for _ in 0..self.size.x {
            write!(f, "───")?;
        }
        writeln!(f, "┘")?;
        write!(f, "    ")?;
        for x in 0..self.size.x {
            write!(f, "{:<3}", x)?;
        }
        writeln!(f)
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
// Lean Wire Encoding / Decoding Helpers
// ============================================================================

fn encode_board_for_lean(b: &Board, use_column_rule: bool, auto_override: Option<Coord>) -> String {
    let n = b.size.x.max(b.size.y) as usize;
    let auto = auto_override.unwrap_or(b.automaton_location);
    let mut cells = String::with_capacity(n * n);
    if n == 0 {
        cells.push('-');
    } else {
        for y in 0..n {
            for x in 0..n {
                let ch = if x < b.size.x as usize && y < b.size.y as usize {
                    let c = Coord {
                        x: x as u8,
                        y: y as u8,
                    };
                    if c == auto {
                        '3'
                    } else {
                        match b.particles[c.ix()].what {
                            Particle::Vacuum | Particle::Automaton => '0',
                            Particle::Repulsor => '1',
                            Particle::Attractor => '2',
                        }
                    }
                } else {
                    '0'
                };
                cells.push(ch);
            }
        }
    }
    let col_tok = if use_column_rule { "1" } else { "0" };
    format!("{} {} {} {} {}", n, cells, auto.x, auto.y, col_tok)
}

fn decode_board_from_lean_tokens(
    toks: &[&str],
    orig_size: Coord,
) -> Option<(Board, usize)> {
    if toks.len() < 5 {
        return None;
    }
    let n: u8 = toks[0].parse().ok()?;
    let cells_str = toks[1];
    let ax: u8 = toks[2].parse().ok()?;
    let ay: u8 = toks[3].parse().ok()?;
    let _col: u8 = toks[4].parse().ok()?;

    let vacuum_cell = Cell {
        what: Particle::Vacuum,
        conflict: false,
        passable: false,
    };
    let width = orig_size.x;
    let height = orig_size.y;
    let mut particles = Grid::from_elem((height as usize, width as usize), vacuum_cell);
    let bytes = cells_str.as_bytes();
    for y in 0..height {
        for x in 0..width {
            let idx = (y as usize) * (n as usize) + (x as usize);
            let what = match bytes.get(idx).copied().unwrap_or(b'0') {
                b'1' => Particle::Repulsor,
                b'2' => Particle::Attractor,
                b'3' => Particle::Automaton,
                _ => Particle::Vacuum,
            };
            particles[Coord { x, y }.ix()] = Cell {
                what,
                conflict: false,
                passable: false,
            };
        }
    }
    let automaton_location = Coord { x: ax, y: ay };
    if automaton_location.x < width && automaton_location.y < height {
        particles[automaton_location.ix()].what = Particle::Automaton;
    }
    Some((
        Board {
            particles,
            size: orig_size,
            automaton_location,
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        },
        5,
    ))
}

fn encode_coords_for_lean(cs: &[Coord]) -> String {
    let mut s = format!("{}", cs.len());
    for c in cs {
        s.push_str(&format!(" {} {}", c.x, c.y));
    }
    s
}

fn encode_moves_for_lean(ms: &[Move]) -> String {
    let mut s = format!("{}", ms.len());
    for m in ms {
        s.push_str(&format!(
            " {} {} {} {} {}",
            m.who.0, m.from.x, m.from.y, m.to.x, m.to.y
        ));
    }
    s
}

fn particle_from_code(code: u8) -> Particle {
    match code {
        1 => Particle::Repulsor,
        2 => Particle::Attractor,
        3 => Particle::Automaton,
        _ => Particle::Vacuum,
    }
}

impl Board {
    /// Standard 11x11 board layout for a two-player game, queried directly from Lean (`stock`).
    pub fn stock_two_player() -> Board {
        let resp = automatafl_lean::rules_wire("stock");
        let toks: Vec<&str> = resp.split_whitespace().collect();
        if toks.first().copied() == Some("1") {
            if let Some((board, _)) =
                decode_board_from_lean_tokens(&toks[1..], Coord { x: 11, y: 11 })
            {
                return board;
            }
        }
        panic!("Lean `stock` query failed: {resp}");
    }

    /// 5x5 board with an automaton and some particles.
    pub fn stock_testing() -> Board {
        let mut b = Self::stock_testing_empty();
        for (x, y, p) in [
            (0, 0, Particle::Repulsor),
            (2, 0, Particle::Attractor),
            (4, 0, Particle::Repulsor),
            (0, 2, Particle::Repulsor),
            (4, 2, Particle::Repulsor),
            (0, 4, Particle::Repulsor),
            (2, 4, Particle::Attractor),
            (4, 4, Particle::Repulsor),
        ] {
            b.place(Coord { x, y }, p);
        }
        b
    }

    /// Empty 5x5 board containing a lonely automaton.
    pub fn stock_testing_empty() -> Board {
        Self::empty_square(5, Coord { x: 2, y: 2 })
    }

    /// Empty 6x6 board containing a lonely automaton at (2, 2).
    pub fn stock_testing_empty_6() -> Board {
        Self::empty_square(6, Coord { x: 2, y: 2 })
    }

    /// Empty 7x7 board containing a lonely automaton at (3, 3).
    pub fn stock_testing_empty_7() -> Board {
        Self::empty_square(7, Coord { x: 3, y: 3 })
    }

    fn empty_square(size: u8, automaton_location: Coord) -> Board {
        let o = Cell {
            what: Particle::Vacuum,
            conflict: false,
            passable: false,
        };
        let mut particles = Grid::from_elem((size as usize, size as usize), o);
        particles[automaton_location.ix()] = Cell {
            what: Particle::Automaton,
            ..o
        };
        Board {
            particles,
            size: Coord { x: size, y: size },
            automaton_location,
            conflict_list: SmallVec::new(),
            passable_list: SmallVec::new(),
        }
    }

    pub fn place(&mut self, c: Coord, w: Particle) {
        if w == Particle::Automaton {
            self.particles[self.automaton_location.ix()].what = Particle::Vacuum;
            self.automaton_location = c;
        }
        self.particles[c.ix()].what = w;
    }

    pub fn mark_passable(&mut self, c: Coord) {
        self.particles[c.ix()].passable = true;
        self.passable_list.push(c);
    }

    pub fn force_move(&mut self, from: Coord, to: Coord) {
        let is_automaton_move = self.particles[from.ix()].what == Particle::Automaton;
        self.particles.swap(from.ix(), to.ix());

        if is_automaton_move {
            debug_assert_eq!(self.automaton_location, from);
            self.automaton_location = to;
        } else if self.automaton_location == to {
            self.automaton_location = from;
        }
    }

    /// Raycast on the board down an axis from a coordinate, delegating axial unit raycasts
    /// on square boards directly to Lean (`sense`).
    pub fn raycast(&self, from: Coord, axis: Delta) -> Raycast {
        debug_assert_ne!(axis, Delta::ZERO);

        if self.size.x == self.size.y
            && (axis == Delta::XP || axis == Delta::XN || axis == Delta::YP || axis == Delta::YN)
        {
            let wire = format!("sense 0 {}", encode_board_for_lean(self, true, Some(from)));
            let resp = automatafl_lean::rules_wire(&wire);
            let toks: Vec<&str> = resp.split_whitespace().collect();
            if toks.first().copied() == Some("1") && toks.len() >= 9 {
                let ray_idx = if axis == Delta::XP {
                    1
                } else if axis == Delta::XN {
                    3
                } else if axis == Delta::YP {
                    5
                } else {
                    7
                };
                if let (Ok(what_code), Ok(dist)) = (
                    toks[ray_idx].parse::<u8>(),
                    toks[ray_idx + 1].parse::<usize>(),
                ) {
                    let what = particle_from_code(what_code);
                    let hit = if what.is_vacuum() {
                        None
                    } else {
                        Some(from + axis.scale(dist as isize))
                    };
                    return Raycast { what, hit, dist };
                }
            }
        }

        for i in 1isize.. {
            let co = from + axis.scale(i);
            if !self.inbounds(co) {
                return Raycast {
                    what: Particle::Vacuum,
                    hit: None,
                    dist: i as usize,
                };
            }
            let c = self.particles[co.ix()];
            if !c.what.is_vacuum() {
                return Raycast {
                    what: c.what,
                    hit: Some(co),
                    dist: i as usize,
                };
            }
        }
        unreachable!()
    }

    pub fn mark_conflict(&mut self, c: Coord) {
        self.particles[c.ix()].conflict = true;
        self.conflict_list.push(c);
    }

    pub fn clear_marks(&mut self) {
        for c in self.conflict_list.drain(..) {
            self.particles[c.ix()].conflict = false;
        }
        for c in self.passable_list.drain(..) {
            self.particles[c.ix()].passable = false;
        }
    }

    pub fn is_conflict(&self, c: Coord) -> bool {
        self.particles[c.ix()].conflict
    }

    pub fn is_vacuum(&self, c: Coord) -> bool {
        self.particles[c.ix()].what.is_vacuum()
    }

    pub fn is_automaton(&self, c: Coord) -> bool {
        self.automaton_location == c
    }

    pub fn inbounds(&self, c: Coord) -> bool {
        c.x < self.size.x && c.y < self.size.y
    }

    pub fn do_move(&mut self, from: Coord, to: Coord) -> MoveResult {
        use MoveResult::*;
        debug_assert!(self.inbounds(from) && self.inbounds(to));
        let delta = to - from;
        debug_assert!(!delta.is_zero());
        debug_assert!(delta.is_axial());

        let axis = delta.axial_unit();
        for offset in 1..=delta.displacement() {
            let c = from + axis.scale(offset as isize);
            if self.particles[c.ix()].occludes() {
                return OccupiedAt(c);
            }
        }
        self.force_move(from, to);
        Applied
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Game {
    pub winner: Option<Pid>,
    pub locked_players: SmallVec<[Pid; 2]>,
    pub board: Board,
    pub round: RoundState,
    pub pending_moves: SmallVec<[Move; 2]>,
    pub goals: SmallVec<[(Coord, Pid); 4]>,
    pub player_count: u8,
    pub use_column_rule: bool,
    pub merge_resolution_mode: MergeResolutionMode,
    pub cycle_behavior_mode: CycleBehaviorMode,
}

impl Game {
    /// Stock two-player goal assignment queried from Lean (`goals <size>`).
    pub fn stock_goals(size: Coord, player_count: u8) -> SmallVec<[(Coord, Pid); 4]> {
        let mut goals = SmallVec::new();
        if player_count == 2 && size.x > 0 && size.y > 0 {
            if size.x == size.y {
                let resp = automatafl_lean::rules_wire(&format!("goals {}", size.x));
                let toks: Vec<&str> = resp.split_whitespace().collect();
                if toks.first().copied() == Some("1") && toks.len() >= 2 {
                    if let Ok(k) = toks[1].parse::<usize>() {
                        for i in 0..k {
                            let base = 2 + i * 3;
                            if base + 2 < toks.len() {
                                if let (Ok(x), Ok(y), Ok(pid)) = (
                                    toks[base].parse::<u8>(),
                                    toks[base + 1].parse::<u8>(),
                                    toks[base + 2].parse::<u8>(),
                                ) {
                                    goals.push((Coord { x, y }, Pid(pid)));
                                }
                            }
                        }
                        if !goals.is_empty() {
                            return goals;
                        }
                    }
                }
            }
            let (right, top) = (size.x - 1, size.y - 1);
            goals.push((Coord { x: 0, y: 0 }, Pid(0)));
            goals.push((Coord { x: right, y: 0 }, Pid(0)));
            goals.push((Coord { x: 0, y: top }, Pid(1)));
            goals.push((Coord { x: right, y: top }, Pid(1)));
        }
        goals
    }

    pub fn new(
        board: Board,
        player_count: u8,
        use_column_rule: bool,
        merge_resolution_mode: MergeResolutionMode,
        cycle_behavior_mode: CycleBehaviorMode,
    ) -> Game {
        let goals = Game::stock_goals(board.size, player_count);
        Game {
            winner: None,
            locked_players: SmallVec::new(),
            board,
            round: RoundState::Fresh,
            pending_moves: SmallVec::new(),
            goals,
            player_count,
            use_column_rule,
            merge_resolution_mode,
            cycle_behavior_mode,
        }
    }

    pub fn new_default_modes(board: Board, player_count: u8, use_column_rule: bool) -> Game {
        Game::new(
            board,
            player_count,
            use_column_rule,
            MergeResolutionMode::DetectAndConflict,
            CycleBehaviorMode::RotatePieces,
        )
    }

    fn lean_game_config_wire(&self) -> String {
        let tb = if self.use_column_rule { 0 } else { 2 };
        let conf = match self.merge_resolution_mode {
            MergeResolutionMode::DetectAndConflict => 0,
            MergeResolutionMode::BunchBeforeMerge | MergeResolutionMode::BunchedStacking => 1,
            MergeResolutionMode::Annihilate => 2,
        };
        format!("{tb} {conf} 0 0 0 1")
    }

    pub fn propose_move(&mut self, m: Move) -> ProposeFeedback {
        use MoveFeedback::*;

        fn consider(cfs: &mut CoordsFeedback, b: &Board, c: Coord) -> bool {
            use CoordFeedback::*;
            let feedback = if !b.inbounds(c) {
                Oob
            } else if b.is_automaton(c) {
                Automaton
            } else if b.is_conflict(c) {
                Conflict
            } else {
                Ok
            };
            let res = feedback == Ok;
            cfs.data.push((c, feedback));
            res
        }

        ProposeFeedback::Rejected(if self.round == RoundState::GameOver {
            GameOver
        } else if self.locked_players.contains(&m.who) {
            WaitYourTurn
        } else if m.from == m.to {
            MustMove
        } else if !(m.from.x == m.to.x || m.from.y == m.to.y) {
            AxisAlignedOnly
        } else {
            let mut cfs = CoordsFeedback {
                data: SmallVec::new(),
            };
            let from_ok = consider(&mut cfs, &self.board, m.from);
            let to_ok = consider(&mut cfs, &self.board, m.to);

            if from_ok && to_ok {
                // Verify move legality via Lean `legal` oracle when board is square
                if self.board.size.x == self.board.size.y {
                    let wire = format!(
                        "legal {} {} {} {} {} {} {}",
                        encode_board_for_lean(&self.board, self.use_column_rule, None),
                        encode_coords_for_lean(&self.board.conflict_list),
                        m.who.0,
                        m.from.x,
                        m.from.y,
                        m.to.x,
                        m.to.y
                    );
                    let resp = automatafl_lean::rules_wire(&wire);
                    debug_assert_eq!(resp, "1 1", "Lean `legal` disagreed on accepted move");
                }
                self.pending_moves.push(m);
                if self.pending_moves.len() == self.player_count as usize {
                    return ProposeFeedback::AcceptedAndReady;
                } else {
                    return ProposeFeedback::Accepted;
                }
            }

            SeeCoords(cfs)
        })
    }

    /// Calculate the coordinate to which the automaton would move right now,
    /// delegated directly to Lean (`step`).
    pub fn automaton_move(&self) -> Coord {
        let tb = if self.use_column_rule { "0" } else { "2" };
        let wire = format!(
            "step {} {}",
            tb,
            encode_board_for_lean(&self.board, self.use_column_rule, None)
        );
        let resp = automatafl_lean::rules_wire(&wire);
        let toks: Vec<&str> = resp.split_whitespace().collect();
        if toks.first().copied() == Some("1") && toks.len() >= 6 {
            if let (Ok(ax), Ok(ay)) = (toks[3].parse::<u8>(), toks[4].parse::<u8>()) {
                return Coord { x: ax, y: ay };
            }
        }
        self.board.automaton_location
    }

    /// Perform the automaton's step via [`Self::automaton_move`].
    pub fn update_automaton(&mut self) {
        let new_location = self.automaton_move();
        if new_location != self.board.automaton_location {
            let result = self
                .board
                .do_move(self.board.automaton_location, new_location);
            debug_assert_eq!(result, MoveResult::Applied);
        }
    }

    fn resolve_conflicts(&mut self) -> Result<SmallVec<[Move; 2]>, SmallVec<[Move; 2]>> {
        // Query Lean `clash` when in DetectAndConflict mode
        let mut conflicted_coords = HashSet::new();
        if self.merge_resolution_mode == MergeResolutionMode::DetectAndConflict {
            let wire = format!(
                "clash {} {} {}",
                encode_board_for_lean(&self.board, self.use_column_rule, None),
                encode_coords_for_lean(&self.board.conflict_list),
                encode_moves_for_lean(&self.pending_moves)
            );
            let resp = automatafl_lean::rules_wire(&wire);
            let toks: Vec<&str> = resp.split_whitespace().collect();
            if toks.first().copied() == Some("1") && toks.len() >= 2 {
                if let Ok(k) = toks[1].parse::<usize>() {
                    for i in 0..k {
                        let base = 2 + i * 2;
                        if base + 1 < toks.len() {
                            if let (Ok(x), Ok(y)) =
                                (toks[base].parse::<u8>(), toks[base + 1].parse::<u8>())
                            {
                                conflicted_coords.insert(Coord { x, y });
                            }
                        }
                    }
                }
            }
        } else {
            // In Annihilate / BunchBefore modes, only source forks are pre-move conflicts
            for m1 in &self.pending_moves {
                for m2 in &self.pending_moves {
                    if m1.from == m2.from && m1.to != m2.to {
                        conflicted_coords.insert(m1.from);
                    }
                }
            }
        }

        if conflicted_coords.is_empty() {
            return Ok(self.pending_moves.clone());
        }

        let mut conflict_moves = SmallVec::new();
        for m in &self.pending_moves {
            if conflicted_coords.contains(&m.from) || conflicted_coords.contains(&m.to) {
                conflict_moves.push(*m);
            }
        }

        for coord in conflicted_coords {
            self.board.mark_conflict(coord);
        }

        Err(conflict_moves)
    }

    /// Resolve a batch of moves on the board by delegating the board transformation
    /// directly to Lean (`midcfg`).
    pub fn apply_moves(
        &mut self,
        moves_to_apply: SmallVec<[Move; 2]>,
    ) -> SmallVec<[(Move, MoveResult); 2]> {
        let mut results = SmallVec::<[(Move, MoveResult); 2]>::new();
        self.board.clear_marks();

        // Compute per-move status diagnostics before replacing the board with Lean's `midcfg`
        for m in &moves_to_apply {
            self.board.mark_passable(m.from);
        }
        for &m in &moves_to_apply {
            let delta = m.to - m.from;
            let axis = delta.axial_unit();
            let mut occluder = None;
            for offset in 1..delta.displacement() {
                let c = m.from + axis.scale(offset as isize);
                if self.board.particles[c.ix()].occludes() {
                    occluder = Some(c);
                    break;
                }
            }
            if let Some(c) = occluder {
                results.push((m, MoveResult::OccupiedAt(c)));
            } else if !self.board.is_vacuum(m.from) {
                results.push((m, MoveResult::Applied));
            } else {
                results.push((m, MoveResult::NoSource));
            }
        }
        self.board.clear_marks();

        // Delegate actual board state transition to Lean `midcfg`
        let wire = format!(
            "midcfg {} {} 0 {}",
            self.lean_game_config_wire(),
            encode_board_for_lean(&self.board, self.use_column_rule, None),
            encode_moves_for_lean(&moves_to_apply)
        );
        let resp = automatafl_lean::rules_wire(&wire);
        let toks: Vec<&str> = resp.split_whitespace().collect();
        if toks.first().copied() == Some("1") {
            if let Some((next_board, _)) =
                decode_board_from_lean_tokens(&toks[1..], self.board.size)
            {
                self.board = next_board;
            }
        }

        results
    }

    pub fn try_complete_round(&mut self) -> CompleteRoundFeedback {
        let needed_moves = (self.player_count as usize).saturating_sub(self.pending_moves.len());
        if needed_moves != 0 {
            if self.round == RoundState::Fresh && !self.pending_moves.is_empty() {
                self.round = RoundState::PartiallySubmitted;
            }
            return CompleteRoundFeedback::WaitingForPlayers(needed_moves);
        }

        match self.resolve_conflicts() {
            Ok(moves_to_apply) => {
                let results = self.apply_moves(moves_to_apply);
                self.update_automaton();

                match self
                    .goals
                    .iter()
                    .copied()
                    .find(|(c, _)| c == &self.board.automaton_location)
                {
                    Some((_, who)) => {
                        self.round = RoundState::GameOver;
                        self.winner = Some(who);
                    }
                    None => {
                        self.board.clear_marks();
                        self.locked_players.clear();
                        self.pending_moves.clear();
                        self.round = RoundState::Fresh;
                    }
                }
                CompleteRoundFeedback::CompletedMoves(results)
            }
            Err(moves_conflicted) => {
                self.round = RoundState::ResolvingConflict;

                self.locked_players.clear();
                for m in &self.pending_moves {
                    if !moves_conflicted.contains(m) {
                        self.locked_players.push(m.who);
                    }
                }

                self.pending_moves.retain(|m| !moves_conflicted.contains(m));

                CompleteRoundFeedback::Conflict(ConflictStatus {
                    conflicted_moves: moves_conflicted,
                    locked_players: self.locked_players.clone(),
                })
            }
        }
    }
}

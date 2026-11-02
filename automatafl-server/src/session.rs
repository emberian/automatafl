//! Fog-of-war simultaneous-move commit-phase session state machine (`GameSession`)
//! and viewer-scoped state projection (`GameView`).
//!
//! ## Security & Fog-of-War Invariants
//! 1. **Secret Move Protection**: While a round is in `Phase::Commit` or `Phase::Resubmit` (or
//!    unopened in `Phase::Reveal`), a player's submitted move coordinates (`frm`, `to`) are stored
//!    exclusively in `secret_committed[seat]`. [`GameSession::view_for`] strips `secret_committed`
//!    and `sel` for every seat other than `viewer`, exposing only `has_committed: bool` and the
//!    cryptographic `commitment_seal` (`blake3` digest).
//! 2. **Cryptographic Commit-Reveal + Server-Sealed Commit**: Supports both direct secret move
//!    submission (sealed by the server with a per-round `blake3` nonce) and zero-knowledge-to-host
//!    client-side hash commitments (`commit_blind_hash` -> `reveal_blind_move`).
//! 3. **Zero Bot Move Leakage**: When playing against an AI bot, the bot computes its move strictly
//!    from public state (`board`, `goals`, `marks`, `locked`) without ever reading the human seat's
//!    `secret_committed` slot.
//! 4. **Conflict Re-Entry Termination**: On `RoundOutcome::Again`, `marks` must strictly grow
//!    (`marks.len() > prev_marks.len()` and `marks.len() <= n * n`). Non-clashing moves are locked
//!    and clashing seats re-enter with fresh secret seals.

use std::borrow::Cow;

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::bot::{BotStrategy, choose_bot_move_with_config};
use crate::oracle::{
    adjudicate_capped, compute_landed_squares, executable_targets, legal_targets, move_legal,
    own_move_forecast_with_config, round_with_config, sense_reading_with_config, sightline_squares,
    sightlines_of, standing, stock_board_for_config, stock_goals2,
};
use crate::protocol::{
    AUTO, Board, CommitMode, Coord, GameConfig, Move, RoundOutcome, Seat, Sightline, Standing, VAC,
};

/// The simultaneous-move turn phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Both seats are sealing a move.
    Commit,
    /// Previous round clashed: contested squares are marked, non-clashing moves lock, waiting seats re-seal.
    Resubmit,
    /// All waiting seats have sealed; waiting seats now open their commitments.
    Reveal,
    /// All waiting seats have opened; ready to resolve the round.
    Resolve,
    /// Match has ended.
    Over,
}

impl Phase {
    pub fn is_sealing(self) -> bool {
        matches!(self, Phase::Commit | Phase::Resubmit)
    }
}

/// How a completed game ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinishReason {
    /// Automaton entered a seat's goal corner.
    GoalReached { winner: Seat },
    /// Turn cap reached and adjudicated by nearest goal distance (`adjudicateCapped`).
    TurnCapAdjudicated { winner: Seat },
    /// Turn cap reached with exact distance parity (`adjudicateCapped == None`).
    DrawByTurnCap,
    /// A player resigned.
    Resignation { winner: Seat, resigned: Seat },
}

/// Record of what the automaton did on a resolved turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoStepRecord {
    pub turn_no: u64,
    pub from: Coord,
    pub to: Coord,
    pub why: String,
}

/// Public disclosure of a clash that forced round re-entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClashDisclosure {
    pub round_no: usize,
    pub new_marks: Vec<Coord>,
    pub all_marks: Vec<Coord>,
    pub submitted_moves: Vec<Move>,
    pub locked_moves: Vec<Move>,
    pub waiting_seats: Vec<u32>,
    pub summary: String,
}

/// Complete record of a resolved turn (`PlayedTurn`) for replay and verification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayedTurn {
    pub turn_no: u64,
    pub start: Board,
    pub conflict_subs: Vec<Vec<Move>>,
    pub clean_subs: Vec<Move>,
    #[serde(default)]
    pub landed_squares: Vec<Coord>,
    pub post_board: Board,
    pub auto_step: AutoStepRecord,
    pub winner: Option<Seat>,
}

/// Compact summary of a resolved turn (without full board clones) for client history logs & highlights.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnSummary {
    pub turn_no: u64,
    pub moves: Vec<Move>,
    pub conflict_rounds: usize,
    #[serde(default)]
    pub landed_squares: Vec<Coord>,
    pub auto_step: AutoStepRecord,
    pub winner: Option<Seat>,
}

/// Complete live session state stored on the server (including private secret vaults).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameSession {
    pub id: String,
    pub name: String,
    pub is_public: bool,
    pub config: GameConfig,
    pub seed: u64,
    pub created_at_ms: u64,
    #[serde(default)]
    pub last_active_ms: u64,

    /// Server-minted CSPRNG secret capability tokens for `[Seat::A, Seat::B]`.
    pub seat_tokens: [String; 2],
    /// Server-only CSPRNG secret salt for server-sealed commitments so commitment digests
    /// cannot be dictionary-attacked over the ~200 legal moves.
    #[serde(default = "generate_seal_secret")]
    pub seal_secret: String,
    /// Display names for `[Seat::A, Seat::B]`.
    pub player_names: [Option<String>; 2],
    /// Optional AI bot controller for `[Seat::A, Seat::B]`.
    pub bot_seats: [Option<BotStrategy>; 2],

    /// Genesis board at turn 0.
    pub start_board: Board,
    /// Current committed board (`RoundState.board`).
    pub board: Board,
    /// Goal assignment `[(coord, seat_pid)]`.
    pub goals: Vec<(Coord, u32)>,

    /// Accumulated conflict markers for the current turn (`RoundState.marks`).
    pub marks: Vec<Coord>,
    /// Locked moves that survived a clash in the current turn (`RoundState.locked`).
    pub locked: Vec<Move>,
    /// Seats that owe a fresh move in the current round (`RoundState.waiting`).
    pub waiting: Vec<u32>,
    /// Conflict submissions from earlier rounds of the current turn.
    pub conflict_subs: Vec<Vec<Move>>,
    /// Most recent clash disclosure in the current turn.
    pub last_clash: Option<ClashDisclosure>,

    /// Selected source square per seat (private to each seat).
    pub sel: [Option<Coord>; 2],
    /// PRIVATE VAULT: each seat's sealed move plaintext (hidden from opponent & spectators until reveal/resolve).
    pub secret_committed: [Option<Move>; 2],
    /// Cryptographic commitment seal per seat (`blake3` hex).
    pub seal: [Option<String>; 2],
    /// True if the commitment was submitted as a blind client-side hash (plaintext not yet sent to server).
    pub is_blind_hash: [bool; 2],
    /// Whether each seat has opened its seal in the current round.
    pub revealed: [bool; 2],

    /// Resolved turn counter.
    pub turn_no: u64,
    /// Monotonic total sealed commitments counter.
    pub commits_count: u64,
    /// Monotonic total opened commitments counter.
    pub reveals_count: u64,
    /// Monotonic state revision number (increments on public state transitions).
    pub revision: u64,

    /// Most recent automaton step telemetry.
    pub last_step: Option<AutoStepRecord>,
    /// Completed turn history for replay verification.
    pub turns_played: Vec<PlayedTurn>,

    /// Winner if decided.
    pub winner: Option<Seat>,
    /// Terminal reason if ended.
    pub finish_reason: Option<FinishReason>,
    /// Whether the match is over.
    pub ended: bool,
}

/// Viewer-scoped projection of a single seat's status (safe to send over the network).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatView {
    pub seat: Seat,
    pub label: Cow<'static, str>,
    pub player_name: Option<String>,
    pub is_claimed: bool,
    pub is_you: bool,
    pub is_bot: bool,
    pub bot_strategy: Option<BotStrategy>,
    pub is_waiting: bool,
    pub is_locked: bool,
    pub has_committed: bool,
    pub has_revealed: bool,
    pub commitment_seal: Option<String>,
    /// ONLY populated when `is_you == true`.
    pub own_selection: Option<Coord>,
    /// ONLY populated when `is_you == true` OR `has_revealed == true` (after reveal/lock).
    /// NEVER populated for an opponent's or spectator's view of an unrevealed move!
    pub visible_move: Option<Move>,
    /// ONLY populated when `is_you == true` and a move is sealed.
    pub own_move_forecast: Option<String>,
    /// Proposable rook-line targets for `own_selection` (only when `is_you == true`).
    pub legal_targets: Vec<Coord>,
    /// Unobstructed executable rook-line targets for `own_selection` (only when `is_you == true`).
    pub executable_targets: Vec<Coord>,
}

/// Lightweight viewer-scoped projection of a cell on the board (zero heap `String` allocations).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellView {
    pub idx: usize,
    pub x: i32,
    pub y: i32,
    pub particle: u8,
    pub glyph: char,
    pub tag: Cow<'static, str>,
    pub highlight: bool,
    pub is_marked: bool,
    #[serde(default)]
    pub is_on_cooldown: bool,
    pub is_goal_of: Option<Seat>,
    pub is_sightline: bool,
    pub is_selectable: bool,
    pub is_proposable_target: bool,
    pub is_executable_target: bool,
}

/// Viewer-scoped projection of the entire game state (`GameView`).
/// Strictly enforces fog-of-war: never contains the other player's unrevealed move coordinates.
/// Omits redundant `cells` over the wire while automatically deriving `cells` on deserialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GameView {
    pub id: String,
    pub name: String,
    pub is_public: bool,
    pub config: GameConfig,
    pub revision: u64,
    pub viewer_seat: Option<Seat>,
    pub phase: Phase,
    pub turn_no: u64,
    pub conflict_round_no: usize,
    pub max_turns: u64,
    pub ended: bool,
    pub winner: Option<Seat>,
    pub finish_reason: Option<FinishReason>,

    pub board: Board,
    pub goals: Vec<(Coord, u32)>,
    pub marks: Vec<Coord>,
    #[serde(default)]
    pub cooldown_squares: Vec<Coord>,
    pub locked_moves: Vec<Move>,
    pub waiting_seats: Vec<u32>,

    pub seats: [SeatView; 2],
    #[serde(skip_serializing, default)]
    pub cells: Vec<CellView>,
    pub sightlines: Vec<Sightline>,
    pub sightline_loop_squares: Vec<Coord>,
    pub automaton_next_target: Coord,
    pub automaton_next_why: String,
    pub standing: Option<Standing>,
    pub last_step: Option<AutoStepRecord>,
    pub last_clash: Option<ClashDisclosure>,
    #[serde(default)]
    pub turn_history: Vec<TurnSummary>,
    pub status_headline: String,
    pub next_step_hint: String,
    pub turns_played_count: usize,
}

impl<'de> Deserialize<'de> for GameView {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct GameViewWire {
            id: String,
            name: String,
            is_public: bool,
            config: GameConfig,
            revision: u64,
            viewer_seat: Option<Seat>,
            phase: Phase,
            turn_no: u64,
            conflict_round_no: usize,
            max_turns: u64,
            ended: bool,
            winner: Option<Seat>,
            finish_reason: Option<FinishReason>,
            board: Board,
            goals: Vec<(Coord, u32)>,
            marks: Vec<Coord>,
            #[serde(default)]
            cooldown_squares: Vec<Coord>,
            locked_moves: Vec<Move>,
            waiting_seats: Vec<u32>,
            seats: [SeatView; 2],
            #[serde(default)]
            cells: Vec<CellView>,
            sightlines: Vec<Sightline>,
            sightline_loop_squares: Vec<Coord>,
            automaton_next_target: Coord,
            automaton_next_why: String,
            standing: Option<Standing>,
            last_step: Option<AutoStepRecord>,
            last_clash: Option<ClashDisclosure>,
            #[serde(default)]
            turn_history: Vec<TurnSummary>,
            status_headline: String,
            next_step_hint: String,
            turns_played_count: usize,
        }

        let w = GameViewWire::deserialize(deserializer)?;
        let mut view = GameView {
            id: w.id,
            name: w.name,
            is_public: w.is_public,
            config: w.config,
            revision: w.revision,
            viewer_seat: w.viewer_seat,
            phase: w.phase,
            turn_no: w.turn_no,
            conflict_round_no: w.conflict_round_no,
            max_turns: w.max_turns,
            ended: w.ended,
            winner: w.winner,
            finish_reason: w.finish_reason,
            board: w.board,
            goals: w.goals,
            marks: w.marks,
            cooldown_squares: w.cooldown_squares,
            locked_moves: w.locked_moves,
            waiting_seats: w.waiting_seats,
            seats: w.seats,
            cells: w.cells,
            sightlines: w.sightlines,
            sightline_loop_squares: w.sightline_loop_squares,
            automaton_next_target: w.automaton_next_target,
            automaton_next_why: w.automaton_next_why,
            standing: w.standing,
            last_step: w.last_step,
            last_clash: w.last_clash,
            turn_history: w.turn_history,
            status_headline: w.status_headline,
            next_step_hint: w.next_step_hint,
            turns_played_count: w.turns_played_count,
        };
        if view.cells.is_empty() {
            view.cells = view.derive_cells();
        }
        Ok(view)
    }
}

impl GameView {
    /// Derive the board `CellView` list from `board`, `marks`, `goals`, `cooldown_squares`,
    /// `sightline_loop_squares`, and viewer seat state with zero heap `String` allocations.
    pub fn derive_cells(&self) -> Vec<CellView> {
        let viewer_view = self.viewer_seat.map(|s| &self.seats[s.idx()]);
        let viewer_sel = viewer_view.and_then(|v| v.own_selection);
        let viewer_sealed = viewer_view
            .filter(|v| v.has_committed && !v.has_revealed)
            .and_then(|v| v.visible_move);
        let viewer_legals = viewer_view
            .map(|v| v.legal_targets.as_slice())
            .unwrap_or(&[]);
        let viewer_live = viewer_view
            .map(|v| v.executable_targets.as_slice())
            .unwrap_or(&[]);
        let viewer_can_play = !self.ended
            && self.phase.is_sealing()
            && viewer_view
                .map(|v| v.is_waiting && !v.has_committed)
                .unwrap_or(false);

        let total_cells = self.board.n * self.board.n;
        let mut cells = Vec::with_capacity(total_cells);
        for idx in 0..total_cells {
            let c = self.board.coord_of(idx);
            let p = self.board.cell_at(c);
            let is_auto = c == self.board.auto;
            let is_marked = self.marks.contains(&c);
            let is_on_cooldown = self.cooldown_squares.contains(&c);
            let is_goal_of = self
                .goals
                .iter()
                .find(|&&(gc, _)| gc == c)
                .map(|&(_, pid)| Seat::from_idx(pid));
            let is_sightline = self.sightline_loop_squares.contains(&c);
            let is_selected = viewer_sel == Some(c);
            let is_proposable = viewer_legals.contains(&c);
            let is_executable = viewer_live.contains(&c);
            let is_movable = p != VAC && p != AUTO && !is_marked && !is_on_cooldown;

            let (tag, highlight): (&'static str, bool) = if is_marked {
                ("mark", false)
            } else if is_auto {
                ("accent", true)
            } else if viewer_sealed.map(|m| m.to) == Some(c) {
                ("sealed", true)
            } else if viewer_sealed.map(|m| m.frm) == Some(c) || is_selected {
                ("warn", true)
            } else if is_executable {
                ("good", true)
            } else if is_proposable {
                ("blocked", false)
            } else if is_on_cooldown {
                ("cooldown", false)
            } else if is_goal_of.is_some() {
                ("goal", false)
            } else if p == VAC {
                ("muted", false)
            } else {
                ("", false)
            };

            let glyph = if is_marked {
                '×'
            } else if p == VAC && is_goal_of.is_some() {
                match is_goal_of.unwrap() {
                    Seat::A => 'a',
                    Seat::B => 'b',
                }
            } else if p == VAC && is_sightline {
                if c.1 == self.board.auto.1 {
                    '─'
                } else {
                    '│'
                }
            } else {
                match p {
                    1 => 'R',
                    2 => 'A',
                    3 => '@',
                    _ => '·',
                }
            };

            cells.push(CellView {
                idx,
                x: c.0,
                y: c.1,
                particle: p,
                glyph,
                tag: Cow::Borrowed(tag),
                highlight,
                is_marked,
                is_on_cooldown,
                is_goal_of,
                is_sightline,
                is_selectable: viewer_can_play && is_movable,
                is_proposable_target: viewer_can_play && is_proposable,
                is_executable_target: viewer_can_play && is_executable,
            });
        }
        cells
    }
}

/// Public lobby summary for a game session (contains zero secret moves or tokens).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobbyGameSummary {
    pub id: String,
    pub name: String,
    pub is_public: bool,
    pub config: GameConfig,
    pub phase: Phase,
    pub turn_no: u64,
    pub max_turns: u64,
    pub seat_a_name: Option<String>,
    pub seat_b_name: Option<String>,
    pub has_open_seat: bool,
    pub ended: bool,
    pub winner: Option<Seat>,
}

impl GameSession {
    /// Mint a new game session with CSPRNG per-seat capability tokens and server seal secret.
    pub fn new(
        id: String,
        name: String,
        is_public: bool,
        config: GameConfig,
        seed: u64,
        created_at_ms: u64,
    ) -> Result<Self, String> {
        let board = stock_board_for_config(&config)?;
        let goals = stock_goals2(board.n)?;
        let token_a = generate_seat_token(&id, seed, Seat::A);
        let token_b = generate_seat_token(&id, seed, Seat::B);
        let seal_secret = generate_seal_secret();

        Ok(Self {
            id,
            name,
            is_public,
            config,
            seed,
            created_at_ms,
            last_active_ms: created_at_ms,
            seat_tokens: [token_a, token_b],
            seal_secret,
            player_names: [None, None],
            bot_seats: [None, None],
            start_board: board.clone(),
            board,
            goals,
            marks: Vec::new(),
            locked: Vec::new(),
            waiting: vec![0, 1],
            conflict_subs: Vec::new(),
            last_clash: None,
            sel: [None, None],
            secret_committed: [None, None],
            seal: [None, None],
            is_blind_hash: [false, false],
            revealed: [false, false],
            turn_no: 0,
            commits_count: 0,
            reveals_count: 0,
            revision: 1,
            last_step: None,
            turns_played: Vec::new(),
            winner: None,
            finish_reason: None,
            ended: false,
        })
    }

    pub fn lobby_summary(&self) -> LobbyGameSummary {
        let a_taken = self.player_names[0].is_some() || self.bot_seats[0].is_some();
        let b_taken = self.player_names[1].is_some() || self.bot_seats[1].is_some();
        LobbyGameSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            is_public: self.is_public,
            config: self.config.clone(),
            phase: self.phase(),
            turn_no: self.turn_no,
            max_turns: self.config.max_turns,
            seat_a_name: self.player_names[0].clone(),
            seat_b_name: self.player_names[1].clone(),
            has_open_seat: !self.ended && (!a_taken || !b_taken),
            ended: self.ended,
            winner: self.winner,
        }
    }

    /// Authenticate a seat capability token, returning the corresponding `Seat` (`None` for spectators).
    pub fn authenticate(&self, token: Option<&str>) -> Option<Seat> {
        let tok = token?.trim();
        if tok.is_empty() {
            return None;
        }
        if constant_time_eq(tok.as_bytes(), self.seat_tokens[0].as_bytes()) {
            Some(Seat::A)
        } else if constant_time_eq(tok.as_bytes(), self.seat_tokens[1].as_bytes()) {
            Some(Seat::B)
        } else {
            None
        }
    }

    /// Claim an open human seat, rotating its capability token to a freshly minted CSPRNG token
    /// so any pre-join invite token held by the game creator cannot spy on or impersonate the joiner.
    pub fn join_seat(
        &mut self,
        preferred: Option<Seat>,
        player_name: String,
    ) -> Result<(Seat, String), String> {
        let target = match preferred {
            Some(s) => {
                if self.player_names[s.idx()].is_some() || self.bot_seats[s.idx()].is_some() {
                    return Err(format!("Seat {} is already taken", s.label()));
                }
                s
            }
            None => {
                if self.player_names[0].is_none() && self.bot_seats[0].is_none() {
                    Seat::A
                } else if self.player_names[1].is_none() && self.bot_seats[1].is_none() {
                    Seat::B
                } else {
                    return Err("Both seats are already taken".to_string());
                }
            }
        };
        let fresh_token = generate_seat_token(&self.id, self.seed, target);
        self.seat_tokens[target.idx()] = fresh_token.clone();
        self.player_names[target.idx()] = Some(player_name);
        self.revision += 1;
        Ok((target, fresh_token))
    }

    /// Attach an AI bot to `seat`, rotating its seat token so no external caller can impersonate the bot.
    pub fn set_bot(&mut self, seat: Seat, strategy: BotStrategy) {
        self.seat_tokens[seat.idx()] = generate_seat_token(&self.id, self.seed, seat);
        self.bot_seats[seat.idx()] = Some(strategy);
        self.player_names[seat.idx()] = Some(strategy.label().to_string());
        self.revision += 1;
    }

    /// Compute the current phase from the round state.
    pub fn phase(&self) -> Phase {
        if self.ended {
            return Phase::Over;
        }
        let owed_commit = self
            .waiting
            .iter()
            .any(|&w| self.seal[w as usize].is_none());
        if owed_commit {
            if self.conflict_subs.is_empty() {
                Phase::Commit
            } else {
                Phase::Resubmit
            }
        } else if self.waiting.iter().any(|&w| !self.revealed[w as usize]) {
            Phase::Reveal
        } else {
            Phase::Resolve
        }
    }

    pub fn is_waiting(&self, seat: Seat) -> bool {
        self.waiting.contains(&seat.pid())
    }

    pub fn is_locked(&self, seat: Seat) -> bool {
        !self.marks.is_empty() && !self.is_waiting(seat)
    }

    pub fn is_marked(&self, c: Coord) -> bool {
        self.marks.contains(&c)
    }

    pub fn active_cooldown_squares(&self) -> Vec<Coord> {
        let cd = self.config.cooldown_turns as usize;
        if cd == 0 || self.turns_played.is_empty() {
            return Vec::new();
        }
        let window = self.turns_played.len().min(cd);
        let mut out = Vec::new();
        for t in &self.turns_played[self.turns_played.len() - window..] {
            for &sq in &t.landed_squares {
                if !out.contains(&sq) {
                    out.push(sq);
                }
            }
        }
        out
    }

    pub fn is_on_cooldown(&self, c: Coord) -> bool {
        self.active_cooldown_squares().contains(&c)
    }

    pub fn movable(&self, c: Coord) -> bool {
        let p = self.board.cell_at(c);
        p != VAC && p != AUTO && !self.is_marked(c) && !self.is_on_cooldown(c)
    }

    pub fn clear_selection(&mut self, seat: Seat) {
        self.sel[seat.idx()] = None;
    }

    /// Per-round, per-seat keyed commitment seal (`blake3` hex) salted with `self.seal_secret`.
    pub fn compute_server_seal(&self, seat: Seat, mv: &Move) -> String {
        let mut h = blake3::Hasher::new();
        h.update(b"automatafl-server/seal/v2:");
        h.update(self.seal_secret.as_bytes());
        h.update(b":");
        h.update(self.id.as_bytes());
        h.update(&self.seed.to_le_bytes());
        h.update(&self.turn_no.to_le_bytes());
        h.update(&(self.conflict_subs.len() as u64).to_le_bytes());
        h.update(&[seat.idx() as u8]);
        h.update(&mv.frm.0.to_le_bytes());
        h.update(&mv.frm.1.to_le_bytes());
        h.update(&mv.to.0.to_le_bytes());
        h.update(&mv.to.1.to_le_bytes());
        let hex = h.finalize().to_hex().to_string();
        hex[..16].to_string()
    }

    /// Compute a client-verifiable commitment hash over `(game_id, turn_no, round_no, seat, frm, to, client_nonce)`.
    pub fn compute_client_commitment(
        game_id: &str,
        turn_no: u64,
        round_no: usize,
        seat: Seat,
        frm: Coord,
        to: Coord,
        client_nonce: &str,
    ) -> String {
        let mut h = blake3::Hasher::new();
        h.update(b"automatafl-client-commit/v1:");
        h.update(game_id.as_bytes());
        h.update(b":");
        h.update(&turn_no.to_le_bytes());
        h.update(&round_no.to_le_bytes());
        h.update(&[seat.idx() as u8]);
        h.update(&frm.0.to_le_bytes());
        h.update(&frm.1.to_le_bytes());
        h.update(&to.0.to_le_bytes());
        h.update(&to.1.to_le_bytes());
        h.update(client_nonce.as_bytes());
        h.finalize().to_hex().to_string()
    }

    /// Alias for [`GameSession::compute_client_commitment`].
    pub fn compute_commitment(
        game_id: &str,
        turn_no: u64,
        round_no: usize,
        seat: Seat,
        frm: Coord,
        to: Coord,
        client_nonce: &str,
    ) -> String {
        Self::compute_client_commitment(game_id, turn_no, round_no, seat, frm, to, client_nonce)
    }

    /// Select a piece at `src` for `seat` during the commit/resubmit phase.
    pub fn select_source(&mut self, seat: Seat, src: Coord) -> Result<(), String> {
        if self.ended {
            return Err("The match is already decided.".to_string());
        }
        if !self.phase().is_sealing() {
            return Err("The commit phase is closed this round.".to_string());
        }
        if !self.is_waiting(seat) {
            return Err(
                "Your move is LOCKED this round: it was not part of the clash, so it stands."
                    .to_string(),
            );
        }
        let i = seat.idx();
        if self.seal[i].is_some() {
            return Err("Your move is already sealed this round.".to_string());
        }
        if !self.board.in_bounds(src) {
            return Err(format!("Square ({},{}) is off the board.", src.0, src.1));
        }
        if self.is_marked(src) {
            return Err(format!(
                "({},{}) is MARKED: a clash burned that square for the rest of this turn.",
                src.0, src.1
            ));
        }
        if self.is_on_cooldown(src) {
            return Err(format!(
                "({},{}) is on cooldown from a recent move.",
                src.0, src.1
            ));
        }
        if !self.movable(src) {
            return Err(format!(
                "({},{}) holds no movable piece (vacuum or the automaton).",
                src.0, src.1
            ));
        }
        self.sel[i] = Some(src);
        Ok(())
    }

    /// Seal a move `(frm, to)` for `seat`, optionally checking `expected_revision` first.
    pub fn submit_move_with_revision(
        &mut self,
        seat: Seat,
        frm: Coord,
        to: Coord,
        expected_revision: Option<u64>,
    ) -> Result<(), String> {
        if let Some(exp) = expected_revision {
            if exp != self.revision {
                return Err(format!(
                    "Stale revision: expected {exp}, current revision is {}.",
                    self.revision
                ));
            }
        }
        self.submit_move(seat, frm, to)
    }

    /// Seal a move `(frm, to)` for `seat`.
    ///
    /// Stores the move in the private server vault (`secret_committed[seat]`), publishes only the
    /// commitment seal to other viewers, triggers any waiting AI bot (from public state only!),
    /// and if `CommitMode::AutoRevealOnBothSealed` is active and all waiting seats have sealed,
    /// automatically reveals and resolves the round.
    pub fn submit_move(&mut self, seat: Seat, frm: Coord, to: Coord) -> Result<(), String> {
        if self.ended {
            return Err("The match is already decided.".to_string());
        }
        if !self.phase().is_sealing() {
            return Err("The commit phase is closed this round.".to_string());
        }
        if !self.is_waiting(seat) {
            return Err(
                "Your move is LOCKED this round: it was not part of the clash, so it stands."
                    .to_string(),
            );
        }
        let i = seat.idx();
        if self.seal[i].is_some() {
            return Err("Your move is already sealed this round.".to_string());
        }
        if self.is_on_cooldown(frm) {
            return Err(format!(
                "({},{}) is on cooldown from a recent move.",
                frm.0, frm.1
            ));
        }

        let mv = Move {
            who: seat.pid(),
            frm,
            to,
        };
        match move_legal(&self.board, &self.marks, &mv)? {
            true => {}
            false if self.is_marked(frm) || self.is_marked(to) => {
                return Err(format!(
                    "({},{}) → ({},{}) names a MARKED square: a clash burned it for the rest of this turn.",
                    frm.0, frm.1, to.0, to.1
                ));
            }
            false => {
                return Err(format!(
                    "Illegal move ({},{}) → ({},{}): must be a rook line to a distinct in-bounds square and never move the automaton.",
                    frm.0, frm.1, to.0, to.1
                ));
            }
        }

        // If the other seat is a waiting AI bot that hasn't committed yet, compute the bot's move
        // BEFORE storing or touching anything from the human's move (and passing only public state).
        self.maybe_commit_bot_moves()?;

        let seal = self.compute_server_seal(seat, &mv);
        self.sel[i] = Some(frm);
        self.secret_committed[i] = Some(mv);
        self.seal[i] = Some(seal);
        self.is_blind_hash[i] = false;
        self.commits_count += 1;
        self.revision += 1;

        self.maybe_auto_advance()?;
        Ok(())
    }

    /// Commit using the seat's previously selected source (`sel[seat]`) and destination `to`.
    pub fn commit_selected(&mut self, seat: Seat, to: Coord) -> Result<(), String> {
        let Some(frm) = self.sel[seat.idx()] else {
            return Err("Select one of your pieces first.".to_string());
        };
        self.submit_move(seat, frm, to)
    }

    /// Submit a blind client-side commitment hash with optional `expected_revision` check.
    pub fn commit_blind_hash_with_revision(
        &mut self,
        seat: Seat,
        commitment_hex: String,
        expected_revision: Option<u64>,
    ) -> Result<(), String> {
        if let Some(exp) = expected_revision {
            if exp != self.revision {
                return Err(format!(
                    "Stale revision: expected {exp}, current revision is {}.",
                    self.revision
                ));
            }
        }
        self.commit_blind_hash(seat, commitment_hex)
    }

    /// Submit a blind client-side commitment hash without revealing `(frm, to)` to the server yet.
    pub fn commit_blind_hash(&mut self, seat: Seat, commitment_hex: String) -> Result<(), String> {
        if self.ended {
            return Err("The match is already decided.".to_string());
        }
        if !self.phase().is_sealing() {
            return Err("The commit phase is closed this round.".to_string());
        }
        if !self.is_waiting(seat) {
            return Err("Your move is LOCKED this round.".to_string());
        }
        let i = seat.idx();
        if self.seal[i].is_some() {
            return Err("Your move is already sealed this round.".to_string());
        }
        let normalized = commitment_hex.trim().to_ascii_lowercase();
        if normalized.len() < 32
            || normalized.len() > 64
            || !normalized.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(
                "Commitment hash must be between 32 and 64 hexadecimal characters.".to_string(),
            );
        }

        self.maybe_commit_bot_moves()?;

        self.seal[i] = Some(normalized);
        self.secret_committed[i] = None;
        self.is_blind_hash[i] = true;
        self.commits_count += 1;
        self.revision += 1;
        Ok(())
    }

    /// Reveal a previously committed move (either opening a server-sealed move in `ExplicitCommitReveal`
    /// mode, or opening a blind client commitment hash with `(frm, to, client_nonce)`).
    pub fn reveal_move(
        &mut self,
        seat: Seat,
        reveal_details: Option<(Coord, Coord, String)>,
    ) -> Result<(), String> {
        if self.ended {
            return Err("The match is already decided.".to_string());
        }
        if !matches!(self.phase(), Phase::Reveal) {
            return Err("Both moves must be sealed before revealing.".to_string());
        }

        // If the human caller's move is already locked from a previous conflict round and only
        // AI bot seats remain in `self.waiting`, allow the human to trigger the bot's reveal.
        if !self.is_waiting(seat) {
            let only_bots_waiting = !self.waiting.is_empty()
                && self
                    .waiting
                    .iter()
                    .all(|&w| self.bot_seats[w as usize].is_some());
            if only_bots_waiting {
                for b_seat in [Seat::A, Seat::B] {
                    let bi = b_seat.idx();
                    if self.bot_seats[bi].is_some()
                        && self.is_waiting(b_seat)
                        && self.seal[bi].is_some()
                        && !self.revealed[bi]
                    {
                        self.revealed[bi] = true;
                        self.reveals_count += 1;
                    }
                }
                self.revision += 1;
                if self.config.commit_mode == CommitMode::AutoRevealOnBothSealed
                    && matches!(self.phase(), Phase::Resolve)
                {
                    self.resolve_round()?;
                }
                return Ok(());
            }
            return Err("Your move is LOCKED this round and already stands.".to_string());
        }

        let i = seat.idx();
        if self.revealed[i] {
            return Err("You have already revealed your move this round.".to_string());
        }

        if self.is_blind_hash[i] {
            let Some((frm, to, nonce)) = reveal_details else {
                return Err(
                    "Blind hash commitment requires (frm, to, client_nonce) to reveal.".to_string(),
                );
            };
            let expected = Self::compute_client_commitment(
                &self.id,
                self.turn_no,
                self.conflict_subs.len(),
                seat,
                frm,
                to,
                &nonce,
            );
            let sealed = self.seal[i].as_deref().unwrap_or("");
            if sealed.len() < 32
                || sealed.len() > expected.len()
                || !constant_time_eq(expected[..sealed.len()].as_bytes(), sealed.as_bytes())
            {
                return Err(
                    "Revealed move and nonce do not match your sealed commitment hash.".to_string(),
                );
            }
            if self.is_on_cooldown(frm) {
                return Err(format!(
                    "({},{}) is on cooldown from a recent move.",
                    frm.0, frm.1
                ));
            }
            let mv = Move {
                who: seat.pid(),
                frm,
                to,
            };
            if !move_legal(&self.board, &self.marks, &mv)? {
                return Err("Revealed move is illegal on the current board and marks.".to_string());
            }
            self.secret_committed[i] = Some(mv);
            self.is_blind_hash[i] = false;
        } else {
            let Some(mv) = self.secret_committed[i] else {
                return Err("No sealed move found to reveal.".to_string());
            };
            let expected = self.compute_server_seal(seat, &mv);
            let sealed = self.seal[i].as_deref().unwrap_or("");
            if !constant_time_eq(sealed.as_bytes(), expected.as_bytes()) {
                return Err("Internal seal mismatch.".to_string());
            }
        }

        self.revealed[i] = true;
        self.reveals_count += 1;
        self.revision += 1;

        // If the other seat is a bot that is waiting to reveal, reveal it too
        for b_seat in [Seat::A, Seat::B] {
            let bi = b_seat.idx();
            if self.bot_seats[bi].is_some()
                && self.is_waiting(b_seat)
                && self.seal[bi].is_some()
                && !self.revealed[bi]
            {
                self.revealed[bi] = true;
                self.reveals_count += 1;
            }
        }

        if self.config.commit_mode == CommitMode::AutoRevealOnBothSealed
            && matches!(self.phase(), Phase::Resolve)
        {
            self.resolve_round()?;
        }
        Ok(())
    }

    pub fn last_auto_delta(&self) -> Option<Coord> {
        self.turns_played.iter().rev().find_map(|t| {
            let dx = t.auto_step.to.0 - t.auto_step.from.0;
            let dy = t.auto_step.to.1 - t.auto_step.from.1;
            if dx != 0 || dy != 0 {
                Some((dx, dy))
            } else {
                None
            }
        })
    }

    /// Execute the round resolution (`AutomataflRules.roundStep`) once all waiting seats have revealed.
    pub fn resolve_round(&mut self) -> Result<RoundOutcome, String> {
        if self.ended {
            return Err("The match is already decided.".to_string());
        }
        if !matches!(self.phase(), Phase::Resolve) {
            return Err("All waiting seats must seal and reveal before resolving.".to_string());
        }

        let subs: Vec<Move> = self
            .waiting
            .iter()
            .filter_map(|&w| self.secret_committed[w as usize])
            .collect();

        let last_auto_delta = self.last_auto_delta();

        let (outcome, mid) = round_with_config(
            &self.config,
            last_auto_delta,
            &self.board,
            &self.goals,
            &self.marks,
            &self.locked,
            &self.waiting,
            &subs,
        )?;

        let mut round_all_moves = self.locked.clone();
        round_all_moves.extend(subs.iter().copied());

        match outcome.clone() {
            RoundOutcome::Again {
                marks,
                locked,
                waiting,
            } => {
                if waiting.is_empty() {
                    return Err(
                        "Ruleset marked a clash but named no waiting seats to re-submit."
                            .to_string(),
                    );
                }
                if marks.len() <= self.marks.len() {
                    return Err(format!(
                        "Round re-entered without marking a new square ({} -> {}).",
                        self.marks.len(),
                        marks.len()
                    ));
                }

                let new_marks: Vec<Coord> = marks
                    .iter()
                    .copied()
                    .filter(|c| !self.marks.contains(c))
                    .collect();
                let round_idx = self.conflict_subs.len() + 1;
                let summary = format!(
                    "Round {} clashed on {}: seats {:?} must re-submit.",
                    round_idx,
                    new_marks
                        .iter()
                        .map(|c| format!("({},{})", c.0, c.1))
                        .collect::<Vec<_>>()
                        .join(", "),
                    waiting
                );

                self.conflict_subs.push(round_all_moves.clone());
                self.last_clash = Some(ClashDisclosure {
                    round_no: round_idx,
                    new_marks,
                    all_marks: marks.clone(),
                    submitted_moves: round_all_moves,
                    locked_moves: locked.clone(),
                    waiting_seats: waiting.clone(),
                    summary,
                });
                self.marks = marks;
                self.locked = locked;
                self.waiting = waiting;

                // Reset only the waiting seats; locked seats keep their revealed standing moves!
                for &w in &self.waiting {
                    let k = w as usize;
                    self.sel[k] = None;
                    self.secret_committed[k] = None;
                    self.seal[k] = None;
                    self.is_blind_hash[k] = false;
                    self.revealed[k] = false;
                }

                self.revision += 1;

                // If only AI bots are in `waiting`, let them re-submit immediately
                if self
                    .waiting
                    .iter()
                    .all(|&w| self.bot_seats[w as usize].is_some())
                {
                    self.maybe_commit_bot_moves()?;
                    self.maybe_auto_advance()?;
                }

                Ok(outcome)
            }
            RoundOutcome::Resolved {
                board: next_board,
                win,
            } => {
                let (_, auto_why) =
                    sense_reading_with_config(&mid, &self.config, last_auto_delta);
                let step_rec = AutoStepRecord {
                    turn_no: self.turn_no,
                    from: mid.auto,
                    to: next_board.auto,
                    why: auto_why,
                };
                let landed_squares = compute_landed_squares(
                    &self.config,
                    &self.board,
                    &self.marks,
                    &round_all_moves,
                );
                let prev_board = self.board.clone();
                self.board = next_board.clone();
                self.turn_no += 1;
                self.last_step = Some(step_rec.clone());
                self.last_clash = None;

                let capped = self.turn_no >= self.config.max_turns;
                let mut turn_winner = win.map(Seat::from_idx);
                if turn_winner.is_none() && capped {
                    turn_winner = adjudicate_capped(&self.board, &self.goals)?.map(Seat::from_idx);
                    self.finish_reason = Some(match turn_winner {
                        Some(w) => FinishReason::TurnCapAdjudicated { winner: w },
                        None => FinishReason::DrawByTurnCap,
                    });
                } else if let Some(w) = turn_winner {
                    self.finish_reason = Some(FinishReason::GoalReached { winner: w });
                }

                self.winner = turn_winner;
                self.ended = self.winner.is_some() || capped;

                self.turns_played.push(PlayedTurn {
                    turn_no: self.turn_no - 1,
                    start: prev_board,
                    conflict_subs: std::mem::take(&mut self.conflict_subs),
                    clean_subs: round_all_moves,
                    landed_squares,
                    post_board: next_board,
                    auto_step: step_rec,
                    winner: self.winner,
                });

                // Clear round state (`ClearState`) for the next turn
                self.marks.clear();
                self.locked.clear();
                self.waiting = vec![0, 1];
                self.sel = [None, None];
                self.secret_committed = [None, None];
                self.seal = [None, None];
                self.is_blind_hash = [false, false];
                self.revealed = [false, false];
                self.revision += 1;

                Ok(outcome)
            }
        }
    }

    /// Resign the match on behalf of `seat`.
    pub fn resign(&mut self, seat: Seat) -> Result<(), String> {
        if self.ended {
            return Err("The match is already over.".to_string());
        }
        let winner = seat.other();
        self.winner = Some(winner);
        self.finish_reason = Some(FinishReason::Resignation {
            winner,
            resigned: seat,
        });
        self.ended = true;
        self.revision += 1;
        Ok(())
    }

    /// Have any waiting AI bot seats compute and seal their move from public state only.
    pub fn maybe_commit_bot_moves(&mut self) -> Result<(), String> {
        if self.ended || !self.phase().is_sealing() {
            return Ok(());
        }
        let last_auto_delta = self.last_auto_delta();
        let cooldown_squares = self.active_cooldown_squares();
        for seat in [Seat::A, Seat::B] {
            let i = seat.idx();
            let Some(strategy) = self.bot_seats[i] else {
                continue;
            };
            if !self.is_waiting(seat) || self.seal[i].is_some() {
                continue;
            }
            let round_seed = self
                .seed
                .wrapping_add(self.turn_no.wrapping_mul(1_000_003))
                .wrapping_add((self.conflict_subs.len() as u64).wrapping_mul(97));
            // Strictly passes ONLY public state (`board`, `goals`, `marks`, `locked`, `cooldown_squares`)
            if let Some(mv) = choose_bot_move_with_config(
                strategy,
                &self.config,
                last_auto_delta,
                &cooldown_squares,
                &self.board,
                &self.goals,
                &self.marks,
                &self.locked,
                seat,
                round_seed,
            ) {
                let seal = self.compute_server_seal(seat, &mv);
                self.sel[i] = Some(mv.frm);
                self.secret_committed[i] = Some(mv);
                self.seal[i] = Some(seal);
                self.is_blind_hash[i] = false;
                self.commits_count += 1;
                self.revision += 1;
            }
        }
        Ok(())
    }

    fn maybe_auto_advance(&mut self) -> Result<(), String> {
        if self.config.commit_mode != CommitMode::AutoRevealOnBothSealed {
            return Ok(());
        }
        // Check if all waiting seats have sealed with server-known plaintext moves
        let all_sealed_with_plaintext = self.waiting.iter().all(|&w| {
            let k = w as usize;
            self.seal[k].is_some() && !self.is_blind_hash[k] && self.secret_committed[k].is_some()
        });
        if all_sealed_with_plaintext {
            for &w in &self.waiting {
                let k = w as usize;
                if !self.revealed[k] {
                    self.revealed[k] = true;
                    self.reveals_count += 1;
                }
            }
            self.resolve_round()?;
        }
        Ok(())
    }

    /// Re-verify the entire recorded turn history from `start_board` through the oracle.
    pub fn verify_replay(&self) -> Result<usize, String> {
        let mut b = self.start_board.clone();
        b.validate()?;
        let mut last_auto_delta: Option<Coord> = None;

        for (idx, t) in self.turns_played.iter().enumerate() {
            if b != t.start {
                return Err(format!("Turn {idx}: start board diverged from prior post_board"));
            }
            let mut marks: Vec<Coord> = Vec::new();
            let mut locked: Vec<Move> = Vec::new();
            let mut waiting: Vec<u32> = vec![0, 1];

            for (r_idx, c_subs) in t.conflict_subs.iter().enumerate() {
                let (out, _) = round_with_config(
                    &self.config,
                    last_auto_delta,
                    &b,
                    &self.goals,
                    &marks,
                    &locked,
                    &waiting,
                    c_subs,
                )?;
                match out {
                    RoundOutcome::Again {
                        marks: next_m,
                        locked: next_l,
                        waiting: next_w,
                    } => {
                        marks = next_m;
                        locked = next_l;
                        waiting = next_w;
                    }
                    RoundOutcome::Resolved { .. } => {
                        return Err(format!(
                            "Turn {idx} conflict round {r_idx} unexpectedly resolved on replay"
                        ));
                    }
                }
            }

            let fresh_subs: Vec<Move> = t
                .clean_subs
                .iter()
                .copied()
                .filter(|m| waiting.contains(&m.who))
                .collect();
            let (out, _) = round_with_config(
                &self.config,
                last_auto_delta,
                &b,
                &self.goals,
                &marks,
                &locked,
                &waiting,
                &fresh_subs,
            )?;
            match out {
                RoundOutcome::Resolved { board: next_b, .. } => {
                    if next_b != t.post_board {
                        return Err(format!(
                            "Turn {idx}: replayed post_board diverged from recorded post_board"
                        ));
                    }
                    if t.auto_step.to != t.auto_step.from {
                        last_auto_delta = Some((
                            t.auto_step.to.0 - t.auto_step.from.0,
                            t.auto_step.to.1 - t.auto_step.from.1,
                        ));
                    }
                    b = next_b;
                }
                RoundOutcome::Again { .. } => {
                    return Err(format!(
                        "Turn {idx}: clean round unexpectedly clashed on replay"
                    ));
                }
            }
        }

        if b != self.board {
            return Err("Live board diverged from replayed history".to_string());
        }
        Ok(self.turns_played.len())
    }

    /// Build a strictly viewer-scoped [`GameView`] for `viewer` (`None` = spectator).
    ///
    /// **Fog-of-War Guarantee**: Any seat `s` where `Some(s) != viewer` and `!self.revealed[s.idx()]`
    /// has its `own_selection`, `visible_move`, `own_move_forecast`, `legal_targets`, and
    /// `executable_targets` completely redacted (`None` / empty).
    pub fn view_for(&self, viewer: Option<Seat>) -> GameView {
        let phase = self.phase();
        let last_auto_delta = self.last_auto_delta();
        let cooldown_squares = self.active_cooldown_squares();
        let seats = [Seat::A, Seat::B].map(|s| {
            let i = s.idx();
            let is_you = viewer == Some(s);
            let is_waiting = self.is_waiting(s);
            let is_locked = self.is_locked(s);
            let has_committed = self.seal[i].is_some();
            let has_revealed = self.revealed[i];

            // Only show selection to the seat owner while unsealed
            let own_selection = if is_you && !has_committed {
                self.sel[i]
            } else {
                None
            };

            // Only show move coordinates to the seat owner OR once officially revealed (e.g., locked or Resolve phase)
            let visible_move = if is_you || has_revealed {
                self.secret_committed[i]
            } else {
                None
            };

            let own_forecast = if is_you && !has_revealed {
                self.secret_committed[i].and_then(|mv| {
                    own_move_forecast_with_config(
                        &self.config,
                        last_auto_delta,
                        &self.board,
                        &self.marks,
                        &self.locked,
                        &self.goals,
                        s.pid(),
                        &mv,
                    )
                })
            } else {
                None
            };

            let (legals, executables) = if let Some(src) = own_selection {
                let l = legal_targets(&self.board, &self.marks, s.pid(), src).unwrap_or_default();
                let e = executable_targets(&self.board, &self.marks, &self.locked, s.pid(), src)
                    .unwrap_or_default();
                (l, e)
            } else {
                (Vec::new(), Vec::new())
            };

            SeatView {
                seat: s,
                label: Cow::Borrowed(s.label()),
                player_name: self.player_names[i].clone(),
                is_claimed: self.player_names[i].is_some() || self.bot_seats[i].is_some(),
                is_you,
                is_bot: self.bot_seats[i].is_some(),
                bot_strategy: self.bot_seats[i],
                is_waiting,
                is_locked,
                has_committed,
                has_revealed,
                commitment_seal: self.seal[i].clone(),
                own_selection,
                visible_move,
                own_move_forecast: own_forecast,
                legal_targets: legals,
                executable_targets: executables,
            }
        });

        let lines = sightlines_of(&self.board);
        let sight_squares = lines
            .as_ref()
            .map(|l| sightline_squares(&self.board, l))
            .unwrap_or_default();
        let sightlines_vec = lines.map(|a| a.to_vec()).unwrap_or_default();

        let (auto_next, auto_why) =
            sense_reading_with_config(&self.board, &self.config, last_auto_delta);
        let standing_val = standing(&self.board, &self.goals).ok();

        let status_headline = match (&self.finish_reason, self.ended) {
            (Some(FinishReason::GoalReached { winner }), _) => format!(
                "Automatafl · Automaton reached Seat {}'s goal · WINNER: {}",
                winner.label(),
                winner.label()
            ),
            (Some(FinishReason::TurnCapAdjudicated { winner }), _) => format!(
                "Automatafl · Turn cap adjudicated in favor of Seat {} · WINNER: {}",
                winner.label(),
                winner.label()
            ),
            (Some(FinishReason::Resignation { winner, resigned }), _) => format!(
                "Automatafl · Seat {} resigned · WINNER: {}",
                resigned.label(),
                winner.label()
            ),
            (Some(FinishReason::DrawByTurnCap), _) | (None, true) => {
                "Automatafl · Match ended in a draw at the turn cap".to_string()
            }
            (None, false) => format!(
                "Automatafl · Turn {}{} · Phase: {:?}",
                self.turn_no,
                if self.marks.is_empty() {
                    String::new()
                } else {
                    format!(" · Round {}", self.conflict_subs.len() + 1)
                },
                phase
            ),
        };

        let next_step_hint = match viewer {
            None => "Spectator mode: both players' unrevealed moves are hidden by fog-of-war.".to_string(),
            Some(s) => {
                let sv = &seats[s.idx()];
                if self.ended {
                    "The match is complete.".to_string()
                } else if sv.is_locked {
                    format!(
                        "Your move is LOCKED and stands untouched. Waiting for Seat {} to re-submit.",
                        s.other().label()
                    )
                } else {
                    match phase {
                        Phase::Commit | Phase::Resubmit if sv.has_committed => format!(
                            "Move sealed ({}). Waiting for Seat {} to seal.",
                            sv.commitment_seal.as_deref().unwrap_or("sealed"),
                            s.other().label()
                        ),
                        Phase::Commit if sv.own_selection.is_some() => {
                            "Piece selected. Click a glowing target square to seal your move.".to_string()
                        }
                        Phase::Commit => {
                            "Click any piece (R or A) to pick it up and light its rook-line targets.".to_string()
                        }
                        Phase::Resubmit => format!(
                            "Clash occurred! Marked squares (×) are dead for this turn. Select a piece and seal a fresh move."
                        ),
                        Phase::Reveal if !sv.has_revealed => {
                            "Both moves are sealed! Press Reveal to open your commitment.".to_string()
                        }
                        Phase::Reveal => format!(
                            "You have revealed. Waiting for Seat {} to reveal.",
                            s.other().label()
                        ),
                        Phase::Resolve => "Both moves are open. Press Resolve to execute the round!".to_string(),
                        Phase::Over => "The match is over.".to_string(),
                    }
                }
            }
        };

        let turn_history: Vec<TurnSummary> = self
            .turns_played
            .iter()
            .map(|t| TurnSummary {
                turn_no: t.turn_no,
                moves: t.clean_subs.clone(),
                conflict_rounds: t.conflict_subs.len(),
                landed_squares: t.landed_squares.clone(),
                auto_step: t.auto_step.clone(),
                winner: t.winner,
            })
            .collect();

        let mut view = GameView {
            id: self.id.clone(),
            name: self.name.clone(),
            is_public: self.is_public,
            config: self.config.clone(),
            revision: self.revision,
            viewer_seat: viewer,
            phase,
            turn_no: self.turn_no,
            conflict_round_no: self.conflict_subs.len(),
            max_turns: self.config.max_turns,
            ended: self.ended,
            winner: self.winner,
            finish_reason: self.finish_reason.clone(),
            board: self.board.clone(),
            goals: self.goals.clone(),
            marks: self.marks.clone(),
            cooldown_squares,
            locked_moves: self.locked.clone(),
            waiting_seats: self.waiting.clone(),
            seats,
            cells: Vec::new(),
            sightlines: sightlines_vec,
            sightline_loop_squares: sight_squares,
            automaton_next_target: auto_next,
            automaton_next_why: auto_why,
            standing: standing_val,
            last_step: self.last_step.clone(),
            last_clash: self.last_clash.clone(),
            turn_history,
            status_headline,
            next_step_hint,
            turns_played_count: self.turns_played.len(),
        };
        view.cells = view.derive_cells();
        view
    }
}

fn generate_seat_token(game_id: &str, seed: u64, seat: Seat) -> String {
    let mut rng = rand::rng();
    let r1: u64 = rng.random();
    let r2: u64 = rng.random();
    let mut h = blake3::Hasher::new();
    h.update(b"automatafl-server/seat-token/v2:");
    h.update(game_id.as_bytes());
    h.update(&seed.to_le_bytes());
    h.update(&r1.to_le_bytes());
    h.update(&r2.to_le_bytes());
    h.update(&[seat.idx() as u8]);
    let hex = h.finalize().to_hex().to_string();
    format!(
        "afs1-{}-{}",
        seat.label().to_lowercase(),
        &hex[..32]
    )
}

fn generate_seal_secret() -> String {
    let mut rng = rand::rng();
    let r1: u64 = rng.random();
    let r2: u64 = rng.random();
    format!("{r1:016x}{r2:016x}")
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

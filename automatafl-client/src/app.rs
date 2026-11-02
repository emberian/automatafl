//! Interactive client state machine (`ClientApp`) for Automatafl (`R6`).
//!
//! Supports:
//! - Playing locally against any simultaneous-move AI bot (`Novice`, `Tactical`, `Minimax`,
//!   `DecoupledUct`, `RegretMatching`).
//! - Playing local 2-player fog-of-war pass-and-play (with viewer switching between `Seat::A`,
//!   `Seat::B`, and `Spectator` so neither player's sealed move is ever leaked on screen).
//! - Playing remotely against a live `automatafl-server` via `RemoteClient`.
//! - Live switching of R2 variant settings (`BoardPreset`, `ConfluenceMode`, `TieBreakMode`,
//!   `two_cycle_swaps`, `cooldown_turns`, `railgun_fling`).

use automatafl_server::bot::BotStrategy;
use automatafl_server::protocol::{
    BoardPreset, ConfluenceMode, Coord, GameConfig, Seat, TieBreakMode,
};
use automatafl_server::session::{GameSession, GameView, Phase};

use crate::tui::{render_frame_ansi, render_frame_plain};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientMode {
    VsBot(BotStrategy),
    LocalPassAndPlay,
}

impl ClientMode {
    pub fn label(&self) -> String {
        match self {
            ClientMode::VsBot(b) => format!("vs {}", b.label()),
            ClientMode::LocalPassAndPlay => "Local Fog-of-War Pass & Play".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCommand {
    MoveCursor(i32, i32),
    ConfirmAtCursor,
    ClearSelection,
    RevealOrResolve,
    CyclePreset,
    CycleConfluence,
    CycleTieBreak,
    ToggleSwaps,
    ToggleRailgun,
    CycleBotStrategy,
    SwitchViewer,
    Resign,
    NewGame,
}

pub struct ClientApp {
    pub mode: ClientMode,
    pub config: GameConfig,
    pub session: GameSession,
    pub viewer: Option<Seat>,
    pub cursor: Coord,
    pub last_message: Option<String>,
    game_seq: u64,
}

impl ClientApp {
    pub fn new(mode: ClientMode, config: GameConfig) -> Result<Self, String> {
        let mut app = Self {
            mode: mode.clone(),
            config: config.clone(),
            session: GameSession::new(
                "local-1".to_string(),
                "Local Table".to_string(),
                true,
                config,
                0xA070_4A7A_F101,
                0,
            )?,
            viewer: Some(Seat::A),
            cursor: (0, 0),
            last_message: None,
            game_seq: 1,
        };
        app.reset_session()?;
        Ok(app)
    }

    pub fn reset_session(&mut self) -> Result<(), String> {
        self.game_seq += 1;
        let id = format!("local-{}", self.game_seq);
        let mut session = GameSession::new(
            id,
            self.mode.label(),
            true,
            self.config.clone(),
            0xA070_4A7A_0000 ^ self.game_seq,
            0,
        )?;
        let _ = session.join_seat(Some(Seat::A), "Player A".to_string())?;
        match self.mode {
            ClientMode::VsBot(strategy) => {
                session.set_bot(Seat::B, strategy);
            }
            ClientMode::LocalPassAndPlay => {
                let _ = session.join_seat(Some(Seat::B), "Player B".to_string())?;
            }
        }
        let mid = (session.board.n as i32) / 2;
        self.cursor = (mid, mid);
        self.viewer = Some(Seat::A);
        self.session = session;
        self.last_message = Some("Started new match.".to_string());
        Ok(())
    }

    pub fn current_view(&self) -> GameView {
        self.session.view_for(self.viewer)
    }

    pub fn render_plain(&self) -> String {
        let view = self.current_view();
        let mut frame = render_frame_plain(&view, Some(self.cursor), &self.mode.label());
        if let Some(ref msg) = self.last_message {
            frame.push_str(&format!("Status: {msg}\n"));
        }
        frame
    }

    pub fn render_ansi(&self) -> String {
        let view = self.current_view();
        let mut frame = render_frame_ansi(&view, Some(self.cursor), &self.mode.label());
        if let Some(ref msg) = self.last_message {
            frame.push_str(&format!("Status: {msg}\n"));
        }
        frame
    }

    pub fn handle_key(&mut self, cmd: KeyCommand) -> Result<(), String> {
        match cmd {
            KeyCommand::MoveCursor(dx, dy) => {
                let n = self.session.board.n as i32;
                self.cursor.0 = (self.cursor.0 + dx).clamp(0, n - 1);
                self.cursor.1 = (self.cursor.1 + dy).clamp(0, n - 1);
                Ok(())
            }
            KeyCommand::ConfirmAtCursor => self.confirm_at_cursor(),
            KeyCommand::ClearSelection => {
                if let Some(seat) = self.viewer {
                    self.session.clear_selection(seat);
                    self.last_message = Some("Cleared piece selection.".to_string());
                }
                Ok(())
            }
            KeyCommand::RevealOrResolve => {
                match self.session.phase() {
                    Phase::Reveal => {
                        if let Some(seat) = self.viewer {
                            self.session.reveal_move(seat, None)?;
                            self.last_message =
                                Some(format!("Seat {} revealed move.", seat.label()));
                        }
                    }
                    Phase::Resolve => {
                        self.session.resolve_round()?;
                        self.last_message = Some("Resolved round.".to_string());
                    }
                    _ => {
                        self.last_message =
                            Some("Round is not yet ready to reveal/resolve.".to_string());
                    }
                }
                Ok(())
            }
            KeyCommand::CyclePreset => {
                self.config.preset = match self.config.preset {
                    BoardPreset::Classic11x11 => BoardPreset::Tactical9x9,
                    BoardPreset::Tactical9x9 => BoardPreset::Blitz7x7,
                    BoardPreset::Blitz7x7 => BoardPreset::Mini5x5,
                    BoardPreset::Mini5x5 => BoardPreset::Classic11x11,
                };
                self.reset_session()
            }
            KeyCommand::CycleConfluence => {
                self.config.confluence = match self.config.confluence {
                    ConfluenceMode::Conflict => ConfluenceMode::BunchBefore,
                    ConfluenceMode::BunchBefore => ConfluenceMode::Annihilate,
                    ConfluenceMode::Annihilate => ConfluenceMode::Conflict,
                };
                self.session.config.confluence = self.config.confluence;
                self.last_message = Some(format!(
                    "Confluence mode set to {}",
                    self.config.confluence.label()
                ));
                Ok(())
            }
            KeyCommand::CycleTieBreak => {
                self.config.tie_break = match self.config.tie_break {
                    TieBreakMode::Column => TieBreakMode::Row,
                    TieBreakMode::Row => TieBreakMode::Freeze,
                    TieBreakMode::Freeze => TieBreakMode::Momentum {
                        prefer_same_axis: true,
                    },
                    TieBreakMode::Momentum {
                        prefer_same_axis: true,
                    } => TieBreakMode::Momentum {
                        prefer_same_axis: false,
                    },
                    TieBreakMode::Momentum {
                        prefer_same_axis: false,
                    } => TieBreakMode::Column,
                };
                self.session.config.tie_break = self.config.tie_break;
                self.session.board.col_rule =
                    matches!(self.config.tie_break, TieBreakMode::Column);
                self.last_message = Some(format!(
                    "Tie-break mode set to {}",
                    self.config.tie_break.label()
                ));
                Ok(())
            }
            KeyCommand::ToggleSwaps => {
                self.config.two_cycle_swaps = !self.config.two_cycle_swaps;
                self.session.config.two_cycle_swaps = self.config.two_cycle_swaps;
                self.last_message = Some(format!(
                    "2-cycle swaps: {}",
                    if self.config.two_cycle_swaps {
                        "ON"
                    } else {
                        "OFF"
                    }
                ));
                Ok(())
            }
            KeyCommand::ToggleRailgun => {
                self.config.railgun_fling = !self.config.railgun_fling;
                self.session.config.railgun_fling = self.config.railgun_fling;
                self.last_message = Some(format!(
                    "Railgun fling: {}",
                    if self.config.railgun_fling {
                        "ON"
                    } else {
                        "OFF"
                    }
                ));
                Ok(())
            }
            KeyCommand::CycleBotStrategy => {
                let next_bot = match self.mode {
                    ClientMode::VsBot(BotStrategy::Novice) => BotStrategy::Tactical,
                    ClientMode::VsBot(BotStrategy::Tactical) => BotStrategy::Minimax,
                    ClientMode::VsBot(BotStrategy::Minimax) => BotStrategy::DecoupledUct,
                    ClientMode::VsBot(BotStrategy::DecoupledUct) => BotStrategy::RegretMatching,
                    ClientMode::VsBot(BotStrategy::RegretMatching) | ClientMode::LocalPassAndPlay => {
                        BotStrategy::Novice
                    }
                };
                self.mode = ClientMode::VsBot(next_bot);
                self.session.set_bot(Seat::B, next_bot);
                self.last_message = Some(format!("AI Bot switched to {}", next_bot.label()));
                Ok(())
            }
            KeyCommand::SwitchViewer => {
                self.viewer = match self.viewer {
                    Some(Seat::A) => Some(Seat::B),
                    Some(Seat::B) => None,
                    None => Some(Seat::A),
                };
                self.last_message = Some(format!("Switched viewer to {:?}", self.viewer));
                Ok(())
            }
            KeyCommand::Resign => {
                if let Some(seat) = self.viewer {
                    self.session.resign(seat)?;
                    self.last_message = Some(format!("Seat {} resigned.", seat.label()));
                }
                Ok(())
            }
            KeyCommand::NewGame => self.reset_session(),
        }
    }

    fn confirm_at_cursor(&mut self) -> Result<(), String> {
        let Some(seat) = self.viewer else {
            return Err("Spectators cannot select or submit moves.".to_string());
        };
        let view = self.current_view();
        let my_seat_view = &view.seats[seat.idx()];

        if let Some(src) = my_seat_view.own_selection {
            if src == self.cursor {
                self.session.clear_selection(seat);
                self.last_message = Some("Deselected piece.".to_string());
                return Ok(());
            }
            if my_seat_view.legal_targets.contains(&self.cursor) {
                let dst = self.cursor;
                self.session.submit_move(seat, src, dst)?;
                if self.mode == ClientMode::LocalPassAndPlay {
                    // In local pass-and-play, if the other seat is still waiting to seal,
                    // automatically rotate the active viewer to the other seat so the first
                    // player's secret move is hidden!
                    let other = seat.other();
                    if self.session.is_waiting(other) && self.session.seal[other.idx()].is_none() {
                        self.viewer = Some(other);
                        self.last_message = Some(format!(
                            "Seat {} sealed move (hidden). Hand terminal to Seat {}!",
                            seat.label(),
                            other.label()
                        ));
                        return Ok(());
                    }
                }
                self.last_message = Some(format!(
                    "Submitted move ({},{}) -> ({},{}).",
                    src.0, src.1, dst.0, dst.1
                ));
                return Ok(());
            }
        }

        // Otherwise, select the piece at the cursor
        self.session.select_source(seat, self.cursor)?;
        self.last_message = Some(format!(
            "Selected piece at ({},{}).",
            self.cursor.0, self.cursor.1
        ));
        Ok(())
    }

    pub fn apply_client_selection(view: &mut GameView, coord: Option<Coord>) -> Result<(), String> {
        apply_client_selection(view, coord)
    }
}

/// Zero-round-trip client-side piece selection and target calculation on a `GameView`
/// (used by remote TUI mode so selecting a piece requires 0 HTTP requests).
pub fn apply_client_selection(view: &mut GameView, coord: Option<Coord>) -> Result<(), String> {
    let Some(seat) = view.viewer_seat else {
        return Err("Spectators cannot select pieces".to_string());
    };
    let s_idx = seat.idx();
    match coord {
        None => {
            view.seats[s_idx].own_selection = None;
            view.seats[s_idx].legal_targets.clear();
            view.seats[s_idx].executable_targets.clear();
        }
        Some(c) => {
            if view.ended {
                return Err("The match is already decided.".to_string());
            }
            if !view.phase.is_sealing() {
                return Err("The commit phase is closed this round.".to_string());
            }
            if !view.seats[s_idx].is_waiting {
                return Err("Your move is LOCKED this round.".to_string());
            }
            if view.seats[s_idx].has_committed {
                return Err("Your move is already sealed this round.".to_string());
            }
            if c.0 < 0
                || c.1 < 0
                || (c.0 as usize) >= view.board.n
                || (c.1 as usize) >= view.board.n
            {
                return Err("Coordinate out of bounds".to_string());
            }
            if view.marks.contains(&c) {
                return Err("Square is marked × this turn".to_string());
            }
            if view.cooldown_squares.contains(&c) {
                return Err("Piece is on cooldown (❄️) this turn".to_string());
            }
            let p = view.board.cell_at(c);
            if p == automatafl_server::protocol::VAC || p == automatafl_server::protocol::AUTO {
                return Err("Select a Repulsor (R) or Attractor (A)".to_string());
            }
            let pid = seat.pid();
            let legals =
                automatafl_server::oracle::legal_targets(&view.board, &view.marks, pid, c)
                    .unwrap_or_default();
            let execs = automatafl_server::oracle::executable_targets(
                &view.board,
                &view.marks,
                &view.locked_moves,
                pid,
                c,
            )
            .unwrap_or_default();
            view.seats[s_idx].own_selection = Some(c);
            view.seats[s_idx].legal_targets = legals;
            view.seats[s_idx].executable_targets = execs;
        }
    }
    view.cells = view.derive_cells();
    Ok(())
}

/// Parsed single-keystroke action from terminal raw mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawKeyAction {
    Command(KeyCommand),
    Refresh,
    Quit,
}

/// Parse the leading keystroke or ANSI escape sequence from `buf` in terminal raw mode,
/// returning `(action, bytes_consumed)`.
pub fn parse_raw_key_bytes(buf: &[u8]) -> Option<(RawKeyAction, usize)> {
    if buf.is_empty() {
        return None;
    }
    // Check ANSI CSI / SS3 arrow escape sequences: `\x1b[A` / `\x1bOA`
    if buf[0] == 0x1b {
        if buf.len() >= 3 && (buf[1] == b'[' || buf[1] == b'O') {
            let act = match buf[2] {
                b'A' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, -1))), // Up
                b'B' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, 1))),  // Down
                b'C' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(1, 0))),  // Right
                b'D' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(-1, 0))), // Left
                _ => None,
            };
            if let Some(a) = act {
                return Some((a, 3));
            }
        }
        // Standalone Escape clears selection
        return Some((RawKeyAction::Command(KeyCommand::ClearSelection), 1));
    }

    let act = match buf[0] {
        b'q' | b'Q' | 0x03 | 0x04 => Some(RawKeyAction::Quit),
        b'h' | b'H' | b'a' | b'A' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(-1, 0))),
        b'l' | b'L' | b'd' | b'D' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(1, 0))),
        b'k' | b'K' | b'w' | b'W' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, -1))),
        b'j' | b'J' | b's' | b'S' => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, 1))),
        b'\r' | b'\n' | b' ' => Some(RawKeyAction::Command(KeyCommand::ConfirmAtCursor)),
        b'x' | b'X' | 0x7f | 0x08 => Some(RawKeyAction::Command(KeyCommand::ClearSelection)),
        b'r' | b'R' => Some(RawKeyAction::Command(KeyCommand::RevealOrResolve)),
        b'v' | b'V' | b'p' | b'P' => Some(RawKeyAction::Command(KeyCommand::CyclePreset)),
        b'c' | b'C' | b'm' | b'M' => Some(RawKeyAction::Command(KeyCommand::CycleConfluence)),
        b't' | b'T' => Some(RawKeyAction::Command(KeyCommand::CycleTieBreak)),
        b'z' | b'Z' => Some(RawKeyAction::Command(KeyCommand::ToggleSwaps)),
        b'g' | b'G' => Some(RawKeyAction::Command(KeyCommand::ToggleRailgun)),
        b'b' | b'B' => Some(RawKeyAction::Command(KeyCommand::CycleBotStrategy)),
        b'\t' => Some(RawKeyAction::Command(KeyCommand::SwitchViewer)),
        b'u' | b'U' => Some(RawKeyAction::Command(KeyCommand::Resign)),
        b'n' | b'N' => Some(RawKeyAction::Command(KeyCommand::NewGame)),
        b'f' | b'F' => Some(RawKeyAction::Refresh),
        _ => None,
    };
    act.map(|a| (a, 1))
}


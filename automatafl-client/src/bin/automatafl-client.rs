//! Interactive TUI / CLI binary for the Automatafl Multi-Target Client (`R6`).
//!
//! Supports both:
//! - **Instant single-keystroke input in terminal raw mode** (when `stdin` is a TTY),
//!   allowing arrow keys (`↑`/`↓`/`←`/`→`), `hjkl`, `Enter`/`Space`, `Esc`, `Tab`, and
//!   variant hotkeys to respond on a single keypress with zero `Enter` needed.
//! - **Line mode fallback** when `stdin` is piped or `--line-mode` is passed.
//! - **Zero-round-trip client-side piece selection** in remote mode via `apply_client_selection`.

use std::env;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::process::Command;

use automatafl_client::app::{
    apply_client_selection, parse_raw_key_bytes, ClientApp, ClientMode, KeyCommand, RawKeyAction,
};
use automatafl_client::remote::RemoteClient;
use automatafl_client::tui::render_frame_ansi;
use automatafl_server::bot::{choose_bot_move, BotStrategy};
use automatafl_server::protocol::{BoardPreset, GameConfig, Seat};

/// RAII terminal raw-mode guard (`stty -icanon -echo min 1 time 0`), restoring the
/// saved `stty -g` state (or `stty sane`) automatically on drop.
struct RawModeGuard {
    saved_state: Option<String>,
    active: bool,
}

impl RawModeGuard {
    fn enter() -> Self {
        if !io::stdin().is_terminal() {
            return Self {
                saved_state: None,
                active: false,
            };
        }
        let saved_state = Command::new("stty")
            .arg("-g")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        let ok = Command::new("stty")
            .args(["-icanon", "-echo", "min", "1", "time", "0"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        Self {
            saved_state,
            active: ok,
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.active {
            if let Some(ref st) = self.saved_state {
                let _ = Command::new("stty").arg(st).status();
            } else {
                let _ = Command::new("stty").arg("sane").status();
            }
        }
    }
}

fn read_next_action(raw_active: bool) -> io::Result<Option<RawKeyAction>> {
    if raw_active {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 8];
        let n = stdin.read(&mut buf)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(parse_raw_key_bytes(&buf[..n]).map(|(act, _)| act))
    } else {
        let stdin = io::stdin();
        let mut line = String::new();
        let n = stdin.lock().read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        let act = match trimmed {
            "q" | "quit" => Some(RawKeyAction::Quit),
            "h" | "left" => Some(RawKeyAction::Command(KeyCommand::MoveCursor(-1, 0))),
            "l" | "right" => Some(RawKeyAction::Command(KeyCommand::MoveCursor(1, 0))),
            "k" | "up" => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, -1))),
            "j" | "down" => Some(RawKeyAction::Command(KeyCommand::MoveCursor(0, 1))),
            "" | "enter" | " " => Some(RawKeyAction::Command(KeyCommand::ConfirmAtCursor)),
            "x" | "esc" => Some(RawKeyAction::Command(KeyCommand::ClearSelection)),
            "r" => Some(RawKeyAction::Command(KeyCommand::RevealOrResolve)),
            "v" | "p" => Some(RawKeyAction::Command(KeyCommand::CyclePreset)),
            "c" | "m" => Some(RawKeyAction::Command(KeyCommand::CycleConfluence)),
            "t" => Some(RawKeyAction::Command(KeyCommand::CycleTieBreak)),
            "b" => Some(RawKeyAction::Command(KeyCommand::CycleBotStrategy)),
            "tab" => Some(RawKeyAction::Command(KeyCommand::SwitchViewer)),
            "n" => Some(RawKeyAction::Command(KeyCommand::NewGame)),
            "f" | "refresh" => Some(RawKeyAction::Refresh),
            _ => None,
        };
        Ok(act)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut mode = ClientMode::VsBot(BotStrategy::DecoupledUct);
    let mut config = GameConfig::default();
    let mut server_url: Option<String> = None;
    let mut demo = false;
    let mut force_line_mode = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" | "-m" => {
                if let Some(val) = args.next() {
                    if val == "local" {
                        mode = ClientMode::LocalPassAndPlay;
                    }
                }
            }
            "--bot" | "-b" => {
                if let Some(val) = args.next() {
                    let strat = match val.as_str() {
                        "novice" => BotStrategy::Novice,
                        "tactical" => BotStrategy::Tactical,
                        "minimax" => BotStrategy::Minimax,
                        "regret_matching" => BotStrategy::RegretMatching,
                        _ => BotStrategy::DecoupledUct,
                    };
                    mode = ClientMode::VsBot(strat);
                }
            }
            "--preset" | "-p" => {
                if let Some(val) = args.next() {
                    config.preset = match val.as_str() {
                        "tactical9x9" | "9x9" => BoardPreset::Tactical9x9,
                        "blitz7x7" | "7x7" => BoardPreset::Blitz7x7,
                        "mini5x5" | "5x5" => BoardPreset::Mini5x5,
                        _ => BoardPreset::Classic11x11,
                    };
                }
            }
            "--server" | "-s" => {
                server_url = args.next();
            }
            "--line-mode" => {
                force_line_mode = true;
            }
            "--demo" => {
                demo = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: automatafl-client [--mode bot|local] [--bot decoupled_uct|regret_matching|minimax|tactical|novice] [--preset 11x11|9x9|7x7|5x5] [--server http://127.0.0.1:3030] [--line-mode] [--demo]"
                );
                return Ok(());
            }
            _ => {}
        }
    }

    if let Some(url) = server_url {
        let client = RemoteClient::new(&url)?;
        let vs_bot = match mode {
            ClientMode::VsBot(strat) => Some(strat),
            ClientMode::LocalPassAndPlay => None,
        };
        let created = client
            .create_game(config, vs_bot, Seat::A, Some("TUI Player".to_string()))
            .await?;
        let game_id = created.game_id;
        let token = created.creator_token.unwrap_or_default();
        let mut view = created.view;
        let mut cursor = view.board.auto;

        if demo {
            println!(
                "{}",
                render_frame_ansi(&view, Some(cursor), &format!("Remote {url}"))
            );
            return Ok(());
        }

        let raw_guard = if force_line_mode {
            RawModeGuard {
                saved_state: None,
                active: false,
            }
        } else {
            RawModeGuard::enter()
        };

        loop {
            print!(
                "\x1b[2J\x1b[H{}",
                render_frame_ansi(&view, Some(cursor), &format!("Remote {url} [{game_id}]"))
            );
            print!(
                "Remote Key (hjkl/Arrows, Enter/Space=select/seal, Esc/x=clear, r=reveal/resolve, f=refresh, q=quit): "
            );
            io::stdout().flush()?;

            let Some(action) = read_next_action(raw_guard.active)? else {
                break;
            };
            let max_c = (view.board.n as i32) - 1;
            match action {
                RawKeyAction::Quit => break,
                RawKeyAction::Refresh => {
                    if let Ok(v) = client.get_view(&game_id, Some(&token)).await {
                        view = v;
                    }
                }
                RawKeyAction::Command(KeyCommand::MoveCursor(dx, dy)) => {
                    cursor.0 = (cursor.0 + dx).clamp(0, max_c);
                    cursor.1 = (cursor.1 + dy).clamp(0, max_c);
                }
                RawKeyAction::Command(KeyCommand::ClearSelection) => {
                    let _ = apply_client_selection(&mut view, None);
                }
                RawKeyAction::Command(KeyCommand::RevealOrResolve) => {
                    if view.phase == automatafl_server::session::Phase::Reveal {
                        if let Ok(v) = client.reveal_move(&game_id, &token, None).await {
                            view = v;
                        }
                    } else if view.phase == automatafl_server::session::Phase::Resolve {
                        if let Ok(v) = client.resolve_round(&game_id, Some(&token)).await {
                            view = v;
                        }
                    }
                }
                RawKeyAction::Command(KeyCommand::ConfirmAtCursor) => {
                    if let Some(seat) = view.viewer_seat {
                        let idx = seat.idx();
                        let sel = view.seats[idx].own_selection;
                        if let Some(src) = sel {
                            if src == cursor {
                                let _ = apply_client_selection(&mut view, None);
                            } else if view.seats[idx].legal_targets.contains(&cursor) {
                                if let Ok(v) =
                                    client.submit_move(&game_id, &token, src, cursor).await
                                {
                                    view = v;
                                }
                            } else {
                                let _ = apply_client_selection(&mut view, Some(cursor));
                            }
                        } else {
                            let _ = apply_client_selection(&mut view, Some(cursor));
                        }
                    }
                }
                _ => {}
            }
        }
        return Ok(());
    }

    let mut app = ClientApp::new(mode, config)?;
    if demo {
        println!("{}", app.render_plain());
        // Execute 1 turn in demo mode and print the resulting board
        if let Some(mv) = choose_bot_move(
            BotStrategy::Tactical,
            &app.session.board,
            &app.session.goals,
            &app.session.marks,
            &app.session.locked,
            Seat::A,
            42,
        ) {
            app.cursor = mv.frm;
            let _ = app.handle_key(KeyCommand::ConfirmAtCursor);
            app.cursor = mv.to;
            let _ = app.handle_key(KeyCommand::ConfirmAtCursor);
            println!("{}", app.render_plain());
        }
        return Ok(());
    }

    let raw_guard = if force_line_mode {
        RawModeGuard {
            saved_state: None,
            active: false,
        }
    } else {
        RawModeGuard::enter()
    };

    loop {
        print!("\x1b[2J\x1b[H{}", app.render_ansi());
        print!(
            "Key (hjkl/Arrows, Enter/Space, Esc/x=clear, r=reveal/resolve, v=preset, c=confluence, t=tiebreak, b=bot, Tab=seat, q=quit): "
        );
        io::stdout().flush()?;

        let Some(action) = read_next_action(raw_guard.active)? else {
            break;
        };
        match action {
            RawKeyAction::Quit => break,
            RawKeyAction::Refresh => {}
            RawKeyAction::Command(c) => {
                if let Err(e) = app.handle_key(c) {
                    app.last_message = Some(format!("Error: {e}"));
                }
            }
        }
    }

    Ok(())
}

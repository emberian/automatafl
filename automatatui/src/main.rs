use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use automatafl_logic::{Coord, Game, Particle, Pid, RoundState};
use clap::Parser;
use crossterm::{
    event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tokio::time::interval;
use uuid::Uuid;

use api::{
    AppliedMove, EventEnvelope, EventsResponse, GameEvent, GameSnapshot, MoveAttemptResult,
    PlayerSummary,
};

mod api;

#[derive(Parser, Debug)]
#[command(author, version, about = "Play Automatafl from the terminal", long_about = None)]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:3000")]
    server: String,
    #[arg(long)]
    player: Option<Uuid>,
    #[arg(long)]
    player_name: Option<String>,
    #[arg(long)]
    player_password: Option<String>,
    #[arg(long)]
    game: Option<Uuid>,
    #[arg(long)]
    create_game: bool,
    #[arg(long, default_value_t = 250)]
    poll_ms: u64,
}

struct BackendClient {
    base: String,
    http: reqwest::Client,
}

impl BackendClient {
    fn new(base: String) -> Result<Self> {
        Ok(Self {
            base,
            http: reqwest::Client::new(),
        })
    }

    fn url(&self, path: &str) -> String {
        let mut base = self.base.trim_end_matches('/').to_string();
        base.push('/');
        base.push_str(path.trim_start_matches('/'));
        base
    }

    async fn register_player(&self, displayname: &str, password: &str) -> Result<Uuid> {
        let url = self.url("/api/v1/register");
        let resp = self
            .http
            .post(url)
            .json(&api::RegisterPlayerRequest {
                displayname,
                password,
            })
            .send()
            .await?;
        let uuid = resp.error_for_status()?.json::<Uuid>().await?;
        Ok(uuid)
    }

    async fn create_game(&self) -> Result<Uuid> {
        let url = self.url("/api/v1/games");
        let resp = self.http.post(url).send().await?;
        Ok(resp.error_for_status()?.json::<Uuid>().await?)
    }

    async fn join_game(&self, game: Uuid, player: Uuid) -> Result<Pid> {
        let url = self.url(&format!("/api/v1/games/{}", game));
        let resp = self
            .http
            .post(url)
            .query(&[("player_uuid", player.to_string())])
            .send()
            .await?;
        Ok(resp.error_for_status()?.json::<Pid>().await?)
    }

    async fn snapshot(&self, game: Uuid) -> Result<GameSnapshot> {
        let url = self.url(&format!("/api/v1/games/{}", game));
        let resp = self.http.get(url).send().await?;
        Ok(resp.error_for_status()?.json::<GameSnapshot>().await?)
    }

    async fn fetch_events(
        &self,
        game: Uuid,
        player: Option<Uuid>,
        since: u64,
    ) -> Result<EventsResponse> {
        let url = self.url(&format!("/api/v1/games/{}/events", game));
        let mut query: Vec<(String, String)> = Vec::new();
        if let Some(player) = player {
            query.push(("player_uuid".into(), player.to_string()));
        }
        if since > 0 {
            query.push(("since".into(), since.to_string()));
        }
        let resp = self.http.get(url).query(&query).send().await?;
        Ok(resp.error_for_status()?.json::<EventsResponse>().await?)
    }

    async fn perform_move(
        &self,
        game: Uuid,
        player: Uuid,
        from: Coord,
        to: Coord,
    ) -> Result<MoveAttemptResult> {
        let url = self.url(&format!("/api/v1/games/{}/move", game));
        let resp = self
            .http
            .post(url)
            .query(&[("player_uuid", player.to_string())])
            .json(&api::PerformMoveRequest { from, to })
            .send()
            .await?;
        Ok(resp.error_for_status()?.json::<MoveAttemptResult>().await?)
    }
}

struct App {
    client: BackendClient,
    game_id: Uuid,
    player_uuid: Uuid,
    pid: Pid,
    game: Option<Game>,
    players: Vec<PlayerSummary>,
    cursor: Coord,
    phase: SelectionPhase,
    log: Vec<String>,
    next_seq: u64,
    needs_state_refresh: bool,
    should_quit: bool,
    locked_players: Vec<Pid>,
    current_round: RoundState,
}

#[derive(Copy, Clone)]
enum SelectionPhase {
    SelectingSource,
    SelectingDestination { from: Coord },
}

impl SelectionPhase {
    fn selected(&self) -> Option<Coord> {
        match self {
            SelectionPhase::SelectingDestination { from } => Some(*from),
            SelectionPhase::SelectingSource => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            SelectionPhase::SelectingSource => "Select source",
            SelectionPhase::SelectingDestination { .. } => "Select destination",
        }
    }
}

impl App {
    fn new(client: BackendClient, game_id: Uuid, player_uuid: Uuid, pid: Pid) -> Self {
        Self {
            client,
            game_id,
            player_uuid,
            pid,
            game: None,
            players: Vec::new(),
            cursor: Coord { x: 0, y: 0 },
            phase: SelectionPhase::SelectingSource,
            log: Vec::new(),
            next_seq: 0,
            needs_state_refresh: true,
            should_quit: false,
            locked_players: Vec::new(),
            current_round: RoundState::Fresh,
        }
    }

    fn push_log<S: Into<String>>(&mut self, msg: S) {
        self.log.push(msg.into());
        if self.log.len() > 200 {
            let excess = self.log.len() - 200;
            self.log.drain(0..excess);
        }
    }

    fn player_label(&self) -> String {
        if let Some(info) = self.players.iter().find(|p| p.uuid == self.player_uuid) {
            format!("{} (P{})", info.displayname, info.pid.0)
        } else {
            format!("Player {}", self.pid.0)
        }
    }

    fn name_for_pid(&self, pid: Pid) -> String {
        self.players
            .iter()
            .find(|p| p.pid == pid)
            .map(|p| p.displayname.clone())
            .unwrap_or_else(|| format!("Player {}", pid.0))
    }

    fn selected_coord(&self) -> Option<Coord> {
        self.phase.selected()
    }

    fn clamp_cursor(&mut self) {
        if let Some(game) = &self.game {
            let width = game.board.size.x.saturating_sub(1);
            let height = game.board.size.y.saturating_sub(1);
            self.cursor.x = self.cursor.x.min(width);
            self.cursor.y = self.cursor.y.min(height);
        }
    }

    async fn refresh_state(&mut self) -> Result<()> {
        let snapshot = self.client.snapshot(self.game_id).await?;
        debug_assert_eq!(snapshot.id, self.game_id);
        self.current_round = snapshot.game.round;
        self.locked_players = snapshot.game.locked_players.iter().copied().collect();
        self.game = Some(snapshot.game);
        self.players = snapshot.players;
        self.clamp_cursor();
        self.needs_state_refresh = false;
        Ok(())
    }

    async fn poll(&mut self) -> Result<()> {
        if self.should_quit {
            return Ok(());
        }
        let EventsResponse { events, next_seq } = self
            .client
            .fetch_events(self.game_id, Some(self.player_uuid), self.next_seq)
            .await?;
        self.next_seq = next_seq;
        for EventEnvelope { seq, event } in events {
            let _ = seq;
            self.handle_event(event);
        }
        if self.needs_state_refresh {
            self.refresh_state().await?;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: GameEvent) {
        match event {
            GameEvent::PlayerJoined { player } => {
                self.players.retain(|p| p.uuid != player.uuid);
                self.players.push(player.clone());
                self.push_log(format!("{} joined the game", player.displayname));
            }
            GameEvent::MoveQueued { pid } => {
                let name = self.name_for_pid(pid);
                self.push_log(format!("{} submitted a move", name));
            }
            GameEvent::MoveRejected { pid, feedback } => {
                let name = self.name_for_pid(pid);
                self.push_log(format!("Move rejected for {}: {}", name, feedback));
                if pid == self.pid {
                    self.phase = SelectionPhase::SelectingSource;
                }
            }
            GameEvent::ConflictsDetected { moves } => {
                if moves.is_empty() {
                    self.push_log("Conflict detected but no moves reported".to_string());
                } else {
                    let summary: Vec<String> = moves
                        .iter()
                        .map(|mv| {
                            format!(
                                "{}: {} -> {}",
                                self.name_for_pid(mv.who),
                                fmt_coord(mv.from),
                                fmt_coord(mv.to)
                            )
                        })
                        .collect();
                    self.push_log(format!(
                        "Conflicts detected:
{}",
                        summary.join(
                            "
"
                        )
                    ));
                }
                self.needs_state_refresh = true;
                self.phase = SelectionPhase::SelectingSource;
            }
            GameEvent::MovesResolved {
                results,
                automaton,
                winner,
            } => {
                for AppliedMove { mv, outcome } in results {
                    let name = self.name_for_pid(mv.who);
                    self.push_log(format!(
                        "{}: {} -> {} => {}",
                        name,
                        fmt_coord(mv.from),
                        fmt_coord(mv.to),
                        outcome
                    ));
                }
                if let Some(auto) = automaton {
                    self.push_log(format!(
                        "Automaton moved from {} to {}",
                        fmt_coord(auto.from),
                        fmt_coord(auto.to)
                    ));
                }
                if let Some(winner) = winner {
                    let name = self.name_for_pid(winner);
                    self.push_log(format!("{} wins the game!", name));
                    self.should_quit = true;
                }
                self.needs_state_refresh = true;
                self.phase = SelectionPhase::SelectingSource;
            }
            GameEvent::RoundState { round, locked } => {
                self.current_round = round;
                self.locked_players = locked;
                self.push_log(format!("Round state: {:?}", round));
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c') = key.code {
                self.should_quit = true;
                return Ok(());
            }
        }
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('r') => {
                self.needs_state_refresh = true;
            }
            KeyCode::Esc => {
                self.phase = SelectionPhase::SelectingSource;
            }
            KeyCode::Left => {
                self.cursor.x = self.cursor.x.saturating_sub(1);
            }
            KeyCode::Right => {
                if let Some(game) = &self.game {
                    let max = game.board.size.x.saturating_sub(1);
                    if self.cursor.x < max {
                        self.cursor.x += 1;
                    }
                }
            }
            KeyCode::Up => {
                if let Some(game) = &self.game {
                    let max = game.board.size.y.saturating_sub(1);
                    if self.cursor.y < max {
                        self.cursor.y += 1;
                    }
                }
            }
            KeyCode::Down => {
                self.cursor.y = self.cursor.y.saturating_sub(1);
            }
            KeyCode::Enter => {
                if self.game.is_none() {
                    return Ok(());
                }
                match self.phase {
                    SelectionPhase::SelectingSource => {
                        self.phase = SelectionPhase::SelectingDestination { from: self.cursor };
                        self.push_log(format!("Selected {}", fmt_coord(self.cursor)));
                    }
                    SelectionPhase::SelectingDestination { from } => {
                        let to = self.cursor;
                        self.phase = SelectionPhase::SelectingSource;
                        self.send_move(from, to).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn send_move(&mut self, from: Coord, to: Coord) -> Result<()> {
        let result = self
            .client
            .perform_move(self.game_id, self.player_uuid, from, to)
            .await;
        match result {
            Ok(outcome) => {
                self.push_log(format!(
                    "Your move {} -> {} => {}",
                    fmt_coord(from),
                    fmt_coord(to),
                    outcome.feedback
                ));
                if outcome.enqueued {
                    self.push_log("All moves submitted. Resolving...".to_string());
                }
            }
            Err(err) => {
                self.push_log(format!("Move submission failed: {err:?}"));
            }
        }
        Ok(())
    }

    fn board_lines(&self) -> Vec<Line<'static>> {
        let Some(game) = &self.game else {
            return vec![Line::from("Waiting for game state...")];
        };
        let mut lines = Vec::new();
        for y in (0..game.board.size.y).rev() {
            let mut spans = Vec::new();
            for x in 0..game.board.size.x {
                let coord = Coord { x, y };
                let cell = game.board.particles[(x as usize, y as usize)];
                let mut style = Style::default();
                if let Some((_, owner)) = game.goals.iter().find(|(c, _)| *c == coord) {
                    style = style.bg(player_color(*owner));
                }
                if cell.conflict {
                    style = style.fg(Color::Red).add_modifier(Modifier::BOLD);
                } else if cell.passable {
                    style = style.fg(Color::Yellow);
                }
                if let Some(selected) = self.selected_coord() {
                    if selected == coord {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                }
                if self.cursor == coord {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                let ch = match cell.what {
                    Particle::Vacuum => '.',
                    Particle::Repulsor => 'R',
                    Particle::Attractor => 'A',
                    Particle::Automaton => '@',
                };
                spans.push(Span::styled(format!(" {} ", ch), style));
            }
            lines.push(Line::from(spans));
        }
        lines
    }

    fn locked_label(&self) -> String {
        if self.locked_players.is_empty() {
            "None".to_string()
        } else {
            let mut names: Vec<String> = self
                .locked_players
                .iter()
                .map(|pid| self.name_for_pid(*pid))
                .collect();
            names.sort();
            names.join(", ")
        }
    }
}

fn player_color(pid: Pid) -> Color {
    match pid.0 % 6 {
        0 => Color::Blue,
        1 => Color::Green,
        2 => Color::Magenta,
        3 => Color::Cyan,
        4 => Color::LightYellow,
        _ => Color::LightBlue,
    }
}

fn fmt_coord(coord: Coord) -> String {
    format!("({}, {})", coord.x, coord.y)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = BackendClient::new(cli.server.clone())?;

    let player_uuid = match (cli.player, cli.player_name.as_deref()) {
        (Some(uuid), _) => uuid,
        (None, Some(name)) => {
            let password = cli.player_password.as_deref().unwrap_or("");
            client
                .register_player(name, password)
                .await
                .context("failed to register player")?
        }
        (None, None) => {
            return Err(anyhow!(
                "Provide either --player UUID or --player-name to register a new player"
            ));
        }
    };

    let game_uuid = match (cli.game, cli.create_game) {
        (Some(uuid), _) => uuid,
        (None, true) => client
            .create_game()
            .await
            .context("failed to create game")?,
        (None, false) => {
            return Err(anyhow!(
                "Provide --game UUID to join or --create-game to make a new one"
            ));
        }
    };

    let pid = client
        .join_game(game_uuid, player_uuid)
        .await
        .context("failed to join game")?;

    let mut app = App::new(client, game_uuid, player_uuid, pid);
    app.push_log(format!("Joined game {game_uuid} as player {}", pid.0));
    app.refresh_state().await?;
    if let Err(err) = app.poll().await {
        app.push_log(format!("Initial event poll failed: {err:?}"));
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut reader = EventStream::new();
    let mut ticker = interval(Duration::from_millis(cli.poll_ms));

    loop {
        terminal.draw(|frame| draw_ui(frame, &app))?;

        tokio::select! {
            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(CrosstermEvent::Key(key))) => {
                        if let Err(err) = app.handle_key(key).await {
                            app.push_log(format!("Input error: {err:?}"));
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        app.push_log(format!("Input stream error: {err:?}"));
                    }
                    None => {
                        app.push_log("Input stream closed".to_string());
                        app.should_quit = true;
                    }
                }
            }
            _ = ticker.tick() => {
                if let Err(err) = app.poll().await {
                    app.push_log(format!("Polling failed: {err:?}"));
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    terminal.show_cursor()?;
    {
        let backend = terminal.backend_mut();
        execute!(backend, LeaveAlternateScreen)?;
        backend.flush()?;
    }
    Ok(())
}

fn draw_ui(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(frame.size());

    let board_block = Block::default().title("Board").borders(Borders::ALL);
    let board_area = board_block.inner(chunks[0]);
    frame.render_widget(board_block, chunks[0]);
    let board = Paragraph::new(app.board_lines()).wrap(Wrap { trim: false });
    frame.render_widget(board, board_area);

    let mut sidebar_lines = Vec::new();
    sidebar_lines.push(Line::from(format!("You are: {}", app.player_label())));
    sidebar_lines.push(Line::from(format!("Cursor: {}", fmt_coord(app.cursor))));
    sidebar_lines.push(Line::from(format!(
        "Selection: {}",
        app.selected_coord()
            .map(fmt_coord)
            .unwrap_or_else(|| "(none)".to_string())
    )));
    sidebar_lines.push(Line::from(format!("Phase: {}", app.phase.label())));
    sidebar_lines.push(Line::from(format!("Round: {:?}", app.current_round)));
    sidebar_lines.push(Line::from(format!(
        "Locked players: {}",
        app.locked_label()
    )));
    sidebar_lines.push(Line::from(
        "Controls: arrows move, Enter select, Esc cancel, r refresh, q quit",
    ));
    sidebar_lines.push(Line::from(""));

    let mut recent: Vec<String> = app.log.iter().rev().take(20).cloned().collect();
    recent.reverse();
    for entry in recent {
        sidebar_lines.push(Line::from(entry));
    }

    let sidebar = Paragraph::new(sidebar_lines)
        .block(Block::default().title("Status").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(sidebar, chunks[1]);
}

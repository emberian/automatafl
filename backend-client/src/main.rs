use automatafl_api_types::GameLifecycle;
use automatafl_backend_client::*;
use automatafl_logic::{Coord, MoveFeedback};

use anyhow::{Context, Result, anyhow};

use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "automatafl-client")]
#[command(about = "CLI client for Automatafl backend", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Backend URL
    #[arg(short, long, default_value = "http://localhost:3000")]
    url: String,

    /// Session token (or use stored session)
    #[arg(short, long)]
    token: Option<Uuid>,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a new player
    Register {
        /// Display name
        displayname: String,
        /// Password
        password: String,
    },

    /// Login as a player
    Login {
        /// Display name
        displayname: String,
        /// Password
        password: String,
    },

    /// Logout (clear session)
    Logout,

    /// Check server health
    Health,

    /// Game management
    #[command(subcommand)]
    Game(GameCommands),

    /// Move management
    #[command(subcommand)]
    Move(MoveCommands),

    /// Chat management
    #[command(subcommand)]
    Chat(ChatCommands),

    /// Player profile management
    #[command(subcommand)]
    Profile(ProfileCommands),

    /// View leaderboards
    #[command(subcommand)]
    Leaderboard(LeaderboardCommands),

    /// Matchmaking
    #[command(subcommand)]
    Matchmaking(MatchmakingCommands),

    /// Admin commands
    #[command(subcommand)]
    Admin(AdminCommands),

    /// Game history
    History {
        /// Game ID
        game_id: Uuid,
        /// Filter by event kind
        #[arg(short, long)]
        kind: Option<String>,
        /// Filter events since timestamp
        #[arg(long)]
        since: Option<u64>,
        /// Filter events until timestamp
        #[arg(long)]
        until: Option<u64>,
    },
}

#[derive(Subcommand)]
enum GameCommands {
    /// List all games
    List,

    /// Create a new game
    Create {
        /// Number of players (2 or 4)
        #[arg(short, long, default_value = "2")]
        players: u8,

        /// Use column rule for tie-breaking
        #[arg(short, long)]
        column_rule: bool,
    },

    /// Join an existing game
    Join {
        /// Game ID
        game_id: Uuid,
    },

    /// Get game state
    State {
        /// Game ID
        game_id: Uuid,
    },

    /// Get goals for a game
    Goals {
        /// Game ID
        game_id: Uuid,
    },

    /// Save game state
    Save {
        /// Game ID
        game_id: Uuid,
    },

    /// List snapshots
    Snapshots {
        /// Game ID
        game_id: Uuid,
    },

    /// Load a snapshot
    Load {
        /// Game ID
        game_id: Uuid,
        /// Snapshot index
        index: usize,
    },
}

#[derive(Subcommand)]
enum MoveCommands {
    /// Show pending move
    Pending {
        /// Game ID
        game_id: Uuid,
    },

    /// Perform a move
    Do {
        /// Game ID
        game_id: Uuid,
        /// From X coordinate
        from_x: u8,
        /// From Y coordinate
        from_y: u8,
        /// To X coordinate
        to_x: u8,
        /// To Y coordinate
        to_y: u8,
    },

    /// Complete the round (manual trigger)
    Complete {
        /// Game ID
        game_id: Uuid,
    },
}

#[derive(Subcommand)]
enum ChatCommands {
    /// Send a chat message
    Send {
        /// Game ID
        game_id: Uuid,
        /// Message to send
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        message: Vec<String>,
    },

    /// Get chat history
    History {
        /// Game ID
        game_id: Uuid,
    },
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// Get player profile
    Get {
        /// Player ID
        player_id: Uuid,
    },

    /// Update your profile
    Update {
        /// Player ID
        player_id: Uuid,
        /// Bio text
        #[arg(short, long)]
        bio: Option<String>,
        /// Avatar URL
        #[arg(short, long)]
        avatar: Option<String>,
    },

    /// Get player stats
    Stats {
        /// Player ID
        player_id: Uuid,
    },
}

#[derive(Subcommand)]
enum LeaderboardCommands {
    /// ELO rating leaderboard
    Elo,

    /// Most wins leaderboard
    Wins,

    /// Most games played leaderboard
    Games,
}

#[derive(Subcommand)]
enum MatchmakingCommands {
    /// Join matchmaking queue
    Join {
        /// Number of players (2 or 4)
        #[arg(short, long, default_value = "2")]
        players: u8,

        /// Use column rule for tie-breaking
        #[arg(short, long)]
        column_rule: bool,
    },

    /// Leave matchmaking queue
    Leave,

    /// Check matchmaking status
    Status,
}

#[derive(Subcommand)]
enum AdminCommands {
    /// Player management
    #[command(subcommand)]
    Player(AdminPlayerCommands),

    /// Game management
    #[command(subcommand)]
    Game(AdminGameCommands),

    /// Session management
    #[command(subcommand)]
    Session(AdminSessionCommands),

    /// Matchmaking queue management
    #[command(subcommand)]
    Queue(AdminQueueCommands),

    /// Database statistics
    Stats,

    /// List database tables
    Tables,
}

#[derive(Subcommand)]
enum AdminPlayerCommands {
    /// List all players
    List,

    /// Get player details
    Get {
        /// Player ID
        player_id: Uuid,
    },

    /// Update player
    Update {
        /// Player ID
        player_id: Uuid,
        /// JSON data to update
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        data: Vec<String>,
    },

    /// Delete player
    Delete {
        /// Player ID
        player_id: Uuid,
    },

    /// Get player stats
    Stats {
        /// Player ID
        player_id: Uuid,
    },

    /// Update player stats
    UpdateStats {
        /// Player ID
        player_id: Uuid,
        /// JSON data to update
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        data: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AdminGameCommands {
    /// List all games
    List,

    /// Get game details
    Get {
        /// Game ID
        game_id: Uuid,
    },

    /// Delete a game
    Delete {
        /// Game ID
        game_id: Uuid,
    },

    /// Force complete a round
    ForceComplete {
        /// Game ID
        game_id: Uuid,
    },

    /// Set game lifecycle
    SetLifecycle {
        /// Game ID
        game_id: Uuid,
        /// Lifecycle (Waiting, InProgress, Finished)
        lifecycle: String,
    },

    /// Get game events
    Events {
        /// Game ID
        game_id: Uuid,
    },

    /// Get game chat
    Chat {
        /// Game ID
        game_id: Uuid,
    },

    /// Delete chat message
    DeleteChat {
        /// Game ID
        game_id: Uuid,
        /// Message timestamp
        timestamp: u64,
    },

    /// List game snapshots
    Snapshots {
        /// Game ID
        game_id: Uuid,
    },

    /// Delete snapshot
    DeleteSnapshot {
        /// Game ID
        game_id: Uuid,
        /// Snapshot index
        index: usize,
    },
}

#[derive(Subcommand)]
enum AdminSessionCommands {
    /// List all sessions
    List,

    /// Delete a session
    Delete {
        /// Session ID
        session_id: Uuid,
    },

    /// Cleanup expired sessions
    Cleanup,
}

#[derive(Subcommand)]
enum AdminQueueCommands {
    /// List matchmaking queue
    List,

    /// Remove player from queue
    Remove {
        /// Player ID
        player_id: Uuid,
    },
}

// ============================================================================
// Session Management
// ============================================================================

fn session_file() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Could not find config directory")?;
    Ok(config_dir.join("automatafl-client").join("session.txt"))
}

fn save_session(token: Uuid) -> Result<()> {
    let path = session_file()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, token.to_string())?;
    println!("{}", format!("Session saved to {:?}", path).dimmed());
    Ok(())
}

fn load_session() -> Result<Option<Uuid>> {
    let path = session_file()?;
    if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        let token = Uuid::parse_str(content.trim())?;
        Ok(Some(token))
    } else {
        Ok(None)
    }
}

fn clear_session() -> Result<()> {
    let path = session_file()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("{}", "Session cleared".green());
    }
    Ok(())
}

fn parse_lifecycle(value: &str) -> Result<GameLifecycle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "waiting" => Ok(GameLifecycle::Waiting),
        "inprogress" | "in_progress" => Ok(GameLifecycle::InProgress),
        "finished" => Ok(GameLifecycle::Finished),
        _ => Err(anyhow!(
            "Invalid lifecycle '{}'. Use Waiting, InProgress, or Finished.",
            value
        )),
    }
}

// ============================================================================
// Command Handlers
// ============================================================================

async fn handle_register(
    client: &AutomataflClient,
    displayname: String,
    password: String,
) -> Result<()> {
    println!("{}", "Registering...".cyan());
    let response = client.register(displayname.clone(), password).await?;
    println!("{}", "✓ Registration successful!".green());
    println!("Player ID: {}", response.player_id.to_string().yellow());
    println!(
        "\n{}",
        format!(
            "Now run: automatafl-client login {} <password>",
            displayname
        )
        .dimmed()
    );
    Ok(())
}

async fn handle_login(
    client: &mut AutomataflClient,
    displayname: String,
    password: String,
) -> Result<()> {
    println!("{}", "Logging in...".cyan());
    let response = client.login(displayname, password).await?;
    println!("{}", "✓ Login successful!".green());
    println!("Session ID: {}", response.session_id.to_string().yellow());
    println!("Player ID: {}", response.player_id.to_string().yellow());
    save_session(response.session_id)?;
    Ok(())
}

async fn handle_logout(client: &mut AutomataflClient) -> Result<()> {
    println!("{}", "Logging out...".cyan());
    client.logout().await?;
    clear_session()?;
    println!("{}", "✓ Logged out successfully!".green());
    Ok(())
}

async fn handle_health(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Checking server health...".cyan());
    let health = client.health_check().await?;
    println!("{}", "✓ Server is healthy!".green());
    println!("Status: {}", health.status.green());
    println!("API Version: {}", health.api_version);
    if let Some(version) = health.cargo_package_version {
        println!("Backend Version: {}", version);
    }
    println!("Timestamp: {}", health.timestamp);
    Ok(())
}

async fn handle_game_list(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching games...".cyan());
    let games = client.list_games().await?;

    if games.is_empty() {
        println!("{}", "No games found".dimmed());
    } else {
        println!(
            "\n{} {} found:\n",
            games.len(),
            if games.len() == 1 { "game" } else { "games" }
        );
        for game in games {
            let lifecycle_str = match game.lifecycle {
                GameLifecycle::Waiting => "Waiting".yellow(),
                GameLifecycle::InProgress => "In Progress".green(),
                GameLifecycle::Finished => "Finished".red(),
            };
            println!("  {} {}", "Game ID:".bold(), game.id.to_string().cyan());
            println!("    Status: {}", lifecycle_str);
            println!("    Players: {}/{}", game.player_count, game.max_players);
            println!("    Created: {} (by {})", game.created_at, game.created_by);
            println!();
        }
    }
    Ok(())
}

async fn handle_game_create(
    client: &AutomataflClient,
    players: u8,
    column_rule: bool,
) -> Result<()> {
    println!("{}", format!("Creating {}-player game...", players).cyan());
    let game_id = client.create_game(players, column_rule).await?;
    println!("{}", "✓ Game created!".green());
    println!("Game ID: {}", game_id.to_string().yellow());
    println!(
        "\n{}",
        format!("To join: automatafl-client game join {}", game_id).dimmed()
    );
    Ok(())
}

async fn handle_game_join(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Joining game {}...", game_id).cyan());
    let pid = client.join_game(game_id).await?;
    println!("{}", "✓ Joined game!".green());
    println!("You are Player {}", pid.0.to_string().yellow());
    Ok(())
}

async fn handle_game_state(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching state for game {}...", game_id).cyan()
    );
    let state = client.get_game_state(game_id).await?;
    println!("\n{}", serde_json::to_string_pretty(&state)?);
    Ok(())
}

async fn handle_game_goals(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching goals for game {}...", game_id).cyan()
    );
    let goals = client.get_goals(game_id).await?;
    println!(
        "\n{} {}:\n",
        goals.len(),
        if goals.len() == 1 { "goal" } else { "goals" }
    );
    for (coord, pid) in goals {
        println!("  Player {} goal at ({}, {})", pid.0, coord.x, coord.y);
    }
    Ok(())
}

async fn handle_move_pending(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching pending move for game {}...", game_id).cyan()
    );
    let pending = client.get_pending_move(game_id).await?;
    match pending {
        Some(m) => {
            println!(
                "Pending move: Player {} from ({}, {}) to ({}, {})",
                m.who.0, m.from.x, m.from.y, m.to.x, m.to.y
            );
        }
        None => {
            println!("{}", "No pending move".dimmed());
        }
    }
    Ok(())
}

async fn handle_move_do(
    client: &AutomataflClient,
    game_id: Uuid,
    from_x: u8,
    from_y: u8,
    to_x: u8,
    to_y: u8,
) -> Result<()> {
    println!(
        "{}",
        format!(
            "Performing move ({}, {}) → ({}, {})...",
            from_x, from_y, to_x, to_y
        )
        .cyan()
    );
    let result = client
        .perform_move(
            game_id,
            Coord {
                x: from_x,
                y: from_y,
            },
            Coord { x: to_x, y: to_y },
        )
        .await?;

    match result.feedback {
        MoveFeedback::Committed => {
            println!("{}", "✓ Move committed!".green());
            if result.auto_completed {
                println!("{}", "  Round auto-completed!".green());
            } else if result.ready_to_complete {
                println!(
                    "{}",
                    "  All players ready - round can be completed".yellow()
                );
            } else {
                println!("{}", "  Waiting for other players...".dimmed());
            }
        }
        MoveFeedback::MustMove => {
            println!(
                "{}",
                "✗ Error: Source and destination must be different".red()
            );
        }
        MoveFeedback::AxisAlignedOnly => {
            println!(
                "{}",
                "✗ Error: Move must be along a row or column (like a Rook)".red()
            );
        }
        MoveFeedback::WaitYourTurn => {
            println!(
                "{}",
                "✗ Error: Wait for conflict resolution to complete".red()
            );
        }
        MoveFeedback::GameOver => {
            println!("{}", "✗ Error: Game is already over".red());
        }
        MoveFeedback::SeeCoords(details) => {
            println!("{}", "✗ Move invalid:".red());
            println!("{}", serde_json::to_string_pretty(&details)?);
        }
    }

    Ok(())
}

async fn handle_move_complete(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", "Completing round...".cyan());
    let result = client.complete_round(game_id).await?;
    println!("{}", "✓ Round completed!".green());
    println!("Message: {}", result.message);
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_chat_send(
    client: &AutomataflClient,
    game_id: Uuid,
    message: Vec<String>,
) -> Result<()> {
    let message_text = message.join(" ");
    println!("{}", format!("Sending message: {}", message_text).cyan());
    let response = client.send_chat(game_id, message_text).await?;
    println!("{}", "✓ Message sent!".green());
    println!("Timestamp: {}", response.timestamp);
    Ok(())
}

async fn handle_chat_history(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching chat history for game {}...", game_id).cyan()
    );
    let messages = client.get_chat(game_id).await?;

    if messages.is_empty() {
        println!("{}", "No messages yet".dimmed());
    } else {
        println!(
            "\n{} {}:\n",
            messages.len(),
            if messages.len() == 1 {
                "message"
            } else {
                "messages"
            }
        );
        for msg in messages {
            println!(
                "[{}] {}: {}",
                msg.timestamp,
                msg.displayname.bold(),
                msg.message
            );
        }
    }
    Ok(())
}

async fn handle_game_history(
    client: &AutomataflClient,
    game_id: Uuid,
    kind: Option<String>,
    since: Option<u64>,
    until: Option<u64>,
) -> Result<()> {
    println!(
        "{}",
        format!("Fetching game history for {}...", game_id).cyan()
    );
    let events = client
        .get_game_history_filtered(game_id, since, until, kind)
        .await?;

    if events.is_empty() {
        println!("{}", "No events found".dimmed());
    } else {
        println!(
            "\n{} {}:\n",
            events.len(),
            if events.len() == 1 { "event" } else { "events" }
        );
        for event in events {
            let timestamp_str = event
                .timestamp
                .map(|t| t.to_string())
                .unwrap_or_else(|| "N/A".to_string());
            // Extract kind from the event data enum
            let kind = match &event.data {
                automatafl_api_types::GameEventData::PlayerJoined { .. } => "PLAYER_JOINED",
                automatafl_api_types::GameEventData::GameStarted { .. } => "GAME_STARTED",
                automatafl_api_types::GameEventData::MoveAcknowledged { .. } => "MOVE_ACK",
                automatafl_api_types::GameEventData::MoveInvalid { .. } => "MOVE_INVALID",
                automatafl_api_types::GameEventData::Move { .. } => "MOVE",
                automatafl_api_types::GameEventData::AutomatonStep { .. } => "AUTOMATON_STEP",
                automatafl_api_types::GameEventData::GameOver { .. } => "GAME_OVER",
                automatafl_api_types::GameEventData::EloUpdate { .. } => "ELO_UPDATE",
                automatafl_api_types::GameEventData::RoundComplete { .. } => "ROUND_COMPLETE",
                automatafl_api_types::GameEventData::Conflicts { .. } => "CONFLICTS",
                automatafl_api_types::GameEventData::Chat { .. } => "CHAT",
                automatafl_api_types::GameEventData::GameLoaded { .. } => "GAME_LOADED",
                automatafl_api_types::GameEventData::State { .. } => "STATE",
            };

            println!(
                "[{}] {}: {}",
                timestamp_str,
                kind.bold(),
                serde_json::to_string(&event.data)?
            );
        }
    }
    Ok(())
}

async fn handle_profile_get(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching profile for {}...", player_id).cyan()
    );
    let profile = client.get_player_profile(player_id).await?;

    println!("\n{}", "Player Profile:".bold());
    println!("  ID: {}", profile.id);
    println!("  Name: {}", profile.displayname.green());
    println!("  ELO: {}", profile.elo_rating.to_string().yellow());
    println!("  Created: {}", profile.created_at);
    if let Some(bio) = profile.bio {
        println!("  Bio: {}", bio);
    }
    if let Some(avatar) = profile.avatar_url {
        println!("  Avatar: {}", avatar);
    }
    Ok(())
}

async fn handle_profile_update(
    client: &AutomataflClient,
    player_id: Uuid,
    bio: Option<String>,
    avatar: Option<String>,
) -> Result<()> {
    println!("{}", "Updating profile...".cyan());
    client.update_player_profile(player_id, bio, avatar).await?;
    println!("{}", "✓ Profile updated!".green());
    Ok(())
}

async fn handle_profile_stats(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching stats for {}...", player_id).cyan());
    let stats = client.get_player_stats(player_id).await?;

    println!("\n{}", "Player Stats:".bold());
    println!("  Games Played: {}", stats.games_played);
    println!("  Games Won: {}", stats.games_won);
    println!("  Win Rate: {:.1}%", stats.win_rate * 100.0);
    println!("  Total Playtime: {}s", stats.total_playtime);
    Ok(())
}

async fn handle_leaderboard(client: &AutomataflClient, board_type: &str) -> Result<()> {
    println!(
        "{}",
        format!("Fetching {} leaderboard...", board_type).cyan()
    );

    let leaderboard = match board_type {
        "elo" => client.get_leaderboard_elo().await?,
        "wins" => client.get_leaderboard_wins().await?,
        "games" => client.get_leaderboard_games().await?,
        _ => unreachable!(),
    };

    println!(
        "\n{} Leaderboard ({} players):\n",
        board_type.to_uppercase(),
        leaderboard.total_players
    );

    for entry in leaderboard.entries {
        println!(
            "  {}. {} - {}",
            entry.rank.to_string().yellow(),
            entry.displayname.bold(),
            entry.value.to_string().cyan()
        );
    }

    Ok(())
}

async fn handle_matchmaking_join(
    client: &AutomataflClient,
    players: u8,
    column_rule: bool,
) -> Result<()> {
    println!(
        "{}",
        format!("Joining matchmaking queue ({} players)...", players).cyan()
    );
    client.join_matchmaking(players, column_rule).await?;
    println!("{}", "✓ Joined matchmaking queue!".green());
    println!("{}", "Waiting for match...".dimmed());
    Ok(())
}

async fn handle_matchmaking_leave(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Leaving matchmaking queue...".cyan());
    client.leave_matchmaking().await?;
    println!("{}", "✓ Left matchmaking queue".green());
    Ok(())
}

async fn handle_matchmaking_status(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Checking matchmaking status...".cyan());
    let status = client.get_matchmaking_status().await?;

    if status.in_queue {
        println!("{}", "✓ In matchmaking queue".green());
        if let Some(queued_at) = status.queued_at {
            println!("  Queued at: {}", queued_at);
        }
        if let Some(wait_time) = status.estimated_wait_time {
            println!("  Estimated wait: {}s", wait_time);
        }
    } else {
        println!("{}", "Not in matchmaking queue".dimmed());
    }

    Ok(())
}

// Admin handlers

async fn handle_admin_player_list(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching all players...".cyan());
    let result = client.admin_list_players().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_player_get(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching player {}...", player_id).cyan());
    let result = client.admin_get_player(player_id).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_player_update(
    client: &AutomataflClient,
    player_id: Uuid,
    data: Vec<String>,
) -> Result<()> {
    let json_str = data.join(" ");
    let json_data: serde_json::Value = serde_json::from_str(&json_str)?;
    println!("{}", format!("Updating player {}...", player_id).cyan());
    client.admin_update_player(player_id, json_data).await?;
    println!("{}", "✓ Player updated!".green());
    Ok(())
}

async fn handle_admin_player_delete(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!("{}", format!("Deleting player {}...", player_id).cyan());
    client.admin_delete_player(player_id).await?;
    println!("{}", "✓ Player deleted!".green());
    Ok(())
}

async fn handle_admin_player_stats(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching stats for player {}...", player_id).cyan()
    );
    let result = client.admin_get_player_stats(player_id).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_player_update_stats(
    client: &AutomataflClient,
    player_id: Uuid,
    data: Vec<String>,
) -> Result<()> {
    let json_str = data.join(" ");
    let json_data: serde_json::Value = serde_json::from_str(&json_str)?;
    println!(
        "{}",
        format!("Updating stats for player {}...", player_id).cyan()
    );
    client
        .admin_update_player_stats(player_id, json_data)
        .await?;
    println!("{}", "✓ Stats updated!".green());
    Ok(())
}

async fn handle_admin_game_list(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching all games...".cyan());
    let result = client.admin_list_games().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_game_get(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching game {}...", game_id).cyan());
    let result = client.admin_get_game(game_id).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_game_delete(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Deleting game {}...", game_id).cyan());
    client.admin_delete_game(game_id).await?;
    println!("{}", "✓ Game deleted!".green());
    Ok(())
}

async fn handle_admin_game_force_complete(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Force completing round for game {}...", game_id).cyan()
    );
    let result = client.admin_force_complete_round(game_id).await?;
    println!("{}", "✓ Round force completed!".green());
    println!("Message: {}", result.message);
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_game_set_lifecycle(
    client: &AutomataflClient,
    game_id: Uuid,
    lifecycle: String,
) -> Result<()> {
    let lifecycle_enum = parse_lifecycle(&lifecycle)?;
    println!(
        "{}",
        format!(
            "Setting game {} lifecycle to {:?}...",
            game_id, lifecycle_enum
        )
        .cyan()
    );
    client
        .admin_set_game_lifecycle(game_id, lifecycle_enum)
        .await?;
    println!("{}", "✓ Lifecycle updated!".green());
    Ok(())
}

async fn handle_admin_game_events(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching events for game {}...", game_id).cyan()
    );
    let events = client.admin_get_game_events(game_id).await?;
    println!(
        "\n{} {}:\n",
        events.len(),
        if events.len() == 1 { "event" } else { "events" }
    );
    for event in events {
        println!("{}", serde_json::to_string_pretty(&event)?);
    }
    Ok(())
}

async fn handle_admin_game_chat(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching chat for game {}...", game_id).cyan()
    );
    let messages = client.admin_get_game_chat(game_id).await?;
    println!(
        "\n{} {}:\n",
        messages.len(),
        if messages.len() == 1 {
            "message"
        } else {
            "messages"
        }
    );
    for msg in messages {
        println!(
            "[{}] {}: {}",
            msg.timestamp,
            msg.displayname.bold(),
            msg.message
        );
    }
    Ok(())
}

async fn handle_admin_game_delete_chat(
    client: &AutomataflClient,
    game_id: Uuid,
    timestamp: u64,
) -> Result<()> {
    println!(
        "{}",
        format!(
            "Deleting chat message {} from game {}...",
            timestamp, game_id
        )
        .cyan()
    );
    client.admin_delete_chat_message(game_id, timestamp).await?;
    println!("{}", "✓ Chat message deleted!".green());
    Ok(())
}

async fn handle_admin_game_snapshots(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Fetching snapshots for game {}...", game_id).cyan()
    );
    let result = client.admin_list_snapshots(game_id).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_game_delete_snapshot(
    client: &AutomataflClient,
    game_id: Uuid,
    index: usize,
) -> Result<()> {
    println!(
        "{}",
        format!("Deleting snapshot {} from game {}...", index, game_id).cyan()
    );
    client.admin_delete_snapshot(game_id, index).await?;
    println!("{}", "✓ Snapshot deleted!".green());
    Ok(())
}

async fn handle_admin_session_list(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching all sessions...".cyan());
    let result = client.admin_list_sessions().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_session_delete(client: &AutomataflClient, session_id: Uuid) -> Result<()> {
    println!("{}", format!("Deleting session {}...", session_id).cyan());
    client.admin_delete_session(session_id).await?;
    println!("{}", "✓ Session deleted!".green());
    Ok(())
}

async fn handle_admin_session_cleanup(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Cleaning up expired sessions...".cyan());
    let result = client.admin_cleanup_expired_sessions().await?;
    println!("{}", "✓ Cleanup complete!".green());
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_queue_list(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching matchmaking queue...".cyan());
    let result = client.admin_list_matchmaking_queue().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_queue_remove(client: &AutomataflClient, player_id: Uuid) -> Result<()> {
    println!(
        "{}",
        format!("Removing player {} from queue...", player_id).cyan()
    );
    client.admin_remove_from_matchmaking(player_id).await?;
    println!("{}", "✓ Player removed from queue!".green());
    Ok(())
}

async fn handle_admin_stats(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching database stats...".cyan());
    let result = client.admin_get_database_stats().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_admin_tables(client: &AutomataflClient) -> Result<()> {
    println!("{}", "Fetching database tables...".cyan());
    let result = client.admin_list_tables().await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create client
    let token = cli.token.or_else(|| load_session().ok().flatten());
    let mut client = if let Some(t) = token {
        AutomataflClient::with_session(&cli.url, t)
    } else {
        AutomataflClient::new(&cli.url)
    };

    // Handle commands
    let result = match cli.command {
        Commands::Register {
            displayname,
            password,
        } => handle_register(&client, displayname, password).await,
        Commands::Login {
            displayname,
            password,
        } => handle_login(&mut client, displayname, password).await,
        Commands::Logout => handle_logout(&mut client).await,
        Commands::Health => handle_health(&client).await,
        Commands::Game(game_cmd) => match game_cmd {
            GameCommands::List => handle_game_list(&client).await,
            GameCommands::Create {
                players,
                column_rule,
            } => handle_game_create(&client, players, column_rule).await,
            GameCommands::Join { game_id } => handle_game_join(&client, game_id).await,
            GameCommands::State { game_id } => handle_game_state(&client, game_id).await,
            GameCommands::Goals { game_id } => handle_game_goals(&client, game_id).await,
            GameCommands::Save { game_id } => {
                let result = client.save_game(game_id).await?;
                println!("{}", "✓ Snapshot saved!".green());
                println!("Snapshot index: {}", result.snapshot_index);
                Ok(())
            }
            GameCommands::Snapshots { game_id } => {
                let result = client.list_snapshots(game_id).await?;
                if result.snapshots.is_empty() {
                    println!("{}", "No snapshots found".dimmed());
                } else {
                    println!(
                        "\n{} {}:\n",
                        result.snapshots.len(),
                        if result.snapshots.len() == 1 {
                            "snapshot"
                        } else {
                            "snapshots"
                        }
                    );
                    for snapshot in result.snapshots {
                        println!(
                            "  Index {} at timestamp {}",
                            snapshot.index, snapshot.timestamp
                        );
                    }
                }
                Ok(())
            }
            GameCommands::Load { game_id, index } => {
                client.load_game(game_id, index).await?;
                println!("{}", "✓ Game loaded!".green());
                Ok(())
            }
        },
        Commands::Move(move_cmd) => match move_cmd {
            MoveCommands::Pending { game_id } => handle_move_pending(&client, game_id).await,
            MoveCommands::Do {
                game_id,
                from_x,
                from_y,
                to_x,
                to_y,
            } => handle_move_do(&client, game_id, from_x, from_y, to_x, to_y).await,
            MoveCommands::Complete { game_id } => handle_move_complete(&client, game_id).await,
        },
        Commands::Chat(chat_cmd) => match chat_cmd {
            ChatCommands::Send { game_id, message } => {
                handle_chat_send(&client, game_id, message).await
            }
            ChatCommands::History { game_id } => handle_chat_history(&client, game_id).await,
        },
        Commands::History {
            game_id,
            kind,
            since,
            until,
        } => handle_game_history(&client, game_id, kind, since, until).await,
        Commands::Profile(profile_cmd) => match profile_cmd {
            ProfileCommands::Get { player_id } => handle_profile_get(&client, player_id).await,
            ProfileCommands::Update {
                player_id,
                bio,
                avatar,
            } => handle_profile_update(&client, player_id, bio, avatar).await,
            ProfileCommands::Stats { player_id } => handle_profile_stats(&client, player_id).await,
        },
        Commands::Leaderboard(leaderboard_cmd) => match leaderboard_cmd {
            LeaderboardCommands::Elo => handle_leaderboard(&client, "elo").await,
            LeaderboardCommands::Wins => handle_leaderboard(&client, "wins").await,
            LeaderboardCommands::Games => handle_leaderboard(&client, "games").await,
        },
        Commands::Matchmaking(matchmaking_cmd) => match matchmaking_cmd {
            MatchmakingCommands::Join {
                players,
                column_rule,
            } => handle_matchmaking_join(&client, players, column_rule).await,
            MatchmakingCommands::Leave => handle_matchmaking_leave(&client).await,
            MatchmakingCommands::Status => handle_matchmaking_status(&client).await,
        },
        Commands::Admin(admin_cmd) => match admin_cmd {
            AdminCommands::Player(player_cmd) => match player_cmd {
                AdminPlayerCommands::List => handle_admin_player_list(&client).await,
                AdminPlayerCommands::Get { player_id } => {
                    handle_admin_player_get(&client, player_id).await
                }
                AdminPlayerCommands::Update { player_id, data } => {
                    handle_admin_player_update(&client, player_id, data).await
                }
                AdminPlayerCommands::Delete { player_id } => {
                    handle_admin_player_delete(&client, player_id).await
                }
                AdminPlayerCommands::Stats { player_id } => {
                    handle_admin_player_stats(&client, player_id).await
                }
                AdminPlayerCommands::UpdateStats { player_id, data } => {
                    handle_admin_player_update_stats(&client, player_id, data).await
                }
            },
            AdminCommands::Game(game_cmd) => match game_cmd {
                AdminGameCommands::List => handle_admin_game_list(&client).await,
                AdminGameCommands::Get { game_id } => handle_admin_game_get(&client, game_id).await,
                AdminGameCommands::Delete { game_id } => {
                    handle_admin_game_delete(&client, game_id).await
                }
                AdminGameCommands::ForceComplete { game_id } => {
                    handle_admin_game_force_complete(&client, game_id).await
                }
                AdminGameCommands::SetLifecycle { game_id, lifecycle } => {
                    handle_admin_game_set_lifecycle(&client, game_id, lifecycle).await
                }
                AdminGameCommands::Events { game_id } => {
                    handle_admin_game_events(&client, game_id).await
                }
                AdminGameCommands::Chat { game_id } => {
                    handle_admin_game_chat(&client, game_id).await
                }
                AdminGameCommands::DeleteChat { game_id, timestamp } => {
                    handle_admin_game_delete_chat(&client, game_id, timestamp).await
                }
                AdminGameCommands::Snapshots { game_id } => {
                    handle_admin_game_snapshots(&client, game_id).await
                }
                AdminGameCommands::DeleteSnapshot { game_id, index } => {
                    handle_admin_game_delete_snapshot(&client, game_id, index).await
                }
            },
            AdminCommands::Session(session_cmd) => match session_cmd {
                AdminSessionCommands::List => handle_admin_session_list(&client).await,
                AdminSessionCommands::Delete { session_id } => {
                    handle_admin_session_delete(&client, session_id).await
                }
                AdminSessionCommands::Cleanup => handle_admin_session_cleanup(&client).await,
            },
            AdminCommands::Queue(queue_cmd) => match queue_cmd {
                AdminQueueCommands::List => handle_admin_queue_list(&client).await,
                AdminQueueCommands::Remove { player_id } => {
                    handle_admin_queue_remove(&client, player_id).await
                }
            },
            AdminCommands::Stats => handle_admin_stats(&client).await,
            AdminCommands::Tables => handle_admin_tables(&client).await,
        },
    };

    // Handle errors nicely
    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }

    Ok(())
}

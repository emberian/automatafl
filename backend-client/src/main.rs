use automatafl_api_types::GameLifecycle;
use automatafl_backend_client::*;
use automatafl_logic::{Coord, MoveFeedback};

use anyhow::{Context, Result};

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

// ============================================================================
// Session Management
// ============================================================================

fn session_file() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?;
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

// ============================================================================
// Command Handlers
// ============================================================================

async fn handle_register(client: &AutomataflClient, displayname: String, password: String) -> Result<()> {
    println!("{}", "Registering...".cyan());
    let response = client.register(displayname.clone(), password).await?;
    println!("{}", "✓ Registration successful!".green());
    println!("Player ID: {}", response.player_id.to_string().yellow());
    println!("\n{}", format!("Now run: automatafl-client login {} <password>", displayname).dimmed());
    Ok(())
}

async fn handle_login(client: &mut AutomataflClient, displayname: String, password: String) -> Result<()> {
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
        println!("\n{} {} found:\n", games.len(), if games.len() == 1 { "game" } else { "games" });
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

async fn handle_game_create(client: &AutomataflClient, players: u8, column_rule: bool) -> Result<()> {
    println!("{}", format!("Creating {}-player game...", players).cyan());
    let game_id = client.create_game(players, column_rule).await?;
    println!("{}", "✓ Game created!".green());
    println!("Game ID: {}", game_id.to_string().yellow());
    println!("\n{}", format!("To join: automatafl-client game join {}", game_id).dimmed());
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
    println!("{}", format!("Fetching state for game {}...", game_id).cyan());
    let state = client.get_game_state(game_id).await?;
    println!("\n{}", serde_json::to_string_pretty(&state)?);
    Ok(())
}

async fn handle_game_goals(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching goals for game {}...", game_id).cyan());
    let goals = client.get_goals(game_id).await?;
    println!("\n{} {}:\n", goals.len(), if goals.len() == 1 { "goal" } else { "goals" });
    for (coord, pid) in goals {
        println!("  Player {} goal at ({}, {})", pid.0, coord.x, coord.y);
    }
    Ok(())
}

async fn handle_move_pending(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching pending move for game {}...", game_id).cyan());
    let pending = client.get_pending_move(game_id).await?;
    match pending {
        Some(m) => {
            println!("Pending move: Player {} from ({}, {}) to ({}, {})",
                m.who.0, m.from.x, m.from.y, m.to.x, m.to.y);
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
    println!("{}", format!("Performing move ({}, {}) → ({}, {})...", from_x, from_y, to_x, to_y).cyan());
    let result = client.perform_move(
        game_id,
        Coord { x: from_x, y: from_y },
        Coord { x: to_x, y: to_y },
    ).await?;

    match result.feedback {
        MoveFeedback::Committed => {
            println!("{}", "✓ Move committed!".green());
            if result.auto_completed {
                println!("{}", "  Round auto-completed!".green());
            } else if result.ready_to_complete {
                println!("{}", "  All players ready - round can be completed".yellow());
            } else {
                println!("{}", "  Waiting for other players...".dimmed());
            }
        }
        MoveFeedback::MustMove => {
            println!("{}", "✗ Error: Source and destination must be different".red());
        }
        MoveFeedback::AxisAlignedOnly => {
            println!("{}", "✗ Error: Move must be along a row or column (like a Rook)".red());
        }
        MoveFeedback::WaitYourTurn => {
            println!("{}", "✗ Error: Wait for conflict resolution to complete".red());
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
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn handle_chat_send(client: &AutomataflClient, game_id: Uuid, message: Vec<String>) -> Result<()> {
    let message_text = message.join(" ");
    println!("{}", format!("Sending message: {}", message_text).cyan());
    client.send_chat(game_id, message_text).await?;
    println!("{}", "✓ Message sent!".green());
    Ok(())
}

async fn handle_chat_history(client: &AutomataflClient, game_id: Uuid) -> Result<()> {
    println!("{}", format!("Fetching chat history for game {}...", game_id).cyan());
    let messages = client.get_chat(game_id).await?;

    if messages.is_empty() {
        println!("{}", "No messages yet".dimmed());
    } else {
        println!("\n{} {}:\n", messages.len(), if messages.len() == 1 { "message" } else { "messages" });
        for msg in messages {
            println!("[{}] {}: {}",
                msg.timestamp,
                msg.displayname.bold(),
                msg.message
            );
        }
    }
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
        Commands::Register { displayname, password } => {
            handle_register(&client, displayname, password).await
        }
        Commands::Login { displayname, password } => {
            handle_login(&mut client, displayname, password).await
        }
        Commands::Logout => {
            handle_logout(&mut client).await
        }
        Commands::Health => {
            handle_health(&client).await
        }
        Commands::Game(game_cmd) => match game_cmd {
            GameCommands::List => handle_game_list(&client).await,
            GameCommands::Create { players, column_rule } => {
                handle_game_create(&client, players, column_rule).await
            }
            GameCommands::Join { game_id } => handle_game_join(&client, game_id).await,
            GameCommands::State { game_id } => handle_game_state(&client, game_id).await,
            GameCommands::Goals { game_id } => handle_game_goals(&client, game_id).await,
            GameCommands::Save { game_id } => {
                let result = client.save_game(game_id).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                Ok(())
            }
            GameCommands::Snapshots { game_id } => {
                let result = client.list_snapshots(game_id).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
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
            MoveCommands::Do { game_id, from_x, from_y, to_x, to_y } => {
                handle_move_do(&client, game_id, from_x, from_y, to_x, to_y).await
            }
            MoveCommands::Complete { game_id } => handle_move_complete(&client, game_id).await,
        },
        Commands::Chat(chat_cmd) => match chat_cmd {
            ChatCommands::Send { game_id, message } => {
                handle_chat_send(&client, game_id, message).await
            }
            ChatCommands::History { game_id } => handle_chat_history(&client, game_id).await,
        },
    };

    // Handle errors nicely
    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }

    Ok(())
}

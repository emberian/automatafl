use automatafl_api_types::GameLifecycle;
use automatafl_logic::{Game, Pid};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use surrealdb::{
    Surreal,
    engine::any::{Any, connect},
    opt::auth::Root,
};
use uuid::Uuid;
use futures::Stream;

pub type Db = Arc<Surreal<Any>>;

/// Initialize the database - either embedded SurrealKV or external connection
pub async fn init_db(uri: Option<String>) -> Result<Db, surrealdb::Error> {
    let db_uri = uri.unwrap_or_else(|| "surrealkv://automatafl.db".to_string());

    tracing::info!("Connecting to database: {}", db_uri);
    let db = connect(&db_uri).await?;

    // Sign in as root for embedded database
    if db_uri.starts_with("surrealkv://") {
        db.signin(Root {
            username: "root",
            password: "root",
        })
        .await?;
    }

    // Use namespace and database
    db.use_ns("automatafl").use_db("main").await?;

    // Run migrations
    migrate(&db).await?;

    Ok(Arc::new(db))
}

/// Database schema and migrations
async fn migrate(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    // Check if migrations already applied
    let applied: Result<Vec<MigrationRecord>, _> = db.select("migrations").await;
    if let Ok(migrations) = applied {
        if migrations.iter().any(|m| m.version == "v1") {
            tracing::info!("Migrations already applied, skipping");
            return Ok(());
        }
    }

    tracing::info!("Running database migrations...");

    // Define tables and indexes

    // Players table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS players SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS id ON TABLE players TYPE string;
        DEFINE FIELD IF NOT EXISTS displayname ON TABLE players TYPE string;
        DEFINE FIELD IF NOT EXISTS password_hash ON TABLE players TYPE string;
        DEFINE FIELD IF NOT EXISTS is_admin ON TABLE players TYPE bool;
        DEFINE FIELD IF NOT EXISTS bio ON TABLE players TYPE option<string>;
        DEFINE FIELD IF NOT EXISTS avatar_url ON TABLE players TYPE option<string>;
        DEFINE FIELD IF NOT EXISTS created_at ON TABLE players TYPE int;
        DEFINE FIELD IF NOT EXISTS elo_rating ON TABLE players TYPE int DEFAULT 1200;
        DEFINE INDEX IF NOT EXISTS displayname_idx ON TABLE players COLUMNS displayname UNIQUE;
        DEFINE INDEX IF NOT EXISTS elo_idx ON TABLE players COLUMNS elo_rating;
    ",
    )
    .await?;

    // Sessions table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS sessions SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS id ON TABLE sessions TYPE string;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE sessions TYPE string;
        DEFINE FIELD IF NOT EXISTS expires_at ON TABLE sessions TYPE int;
        DEFINE INDEX IF NOT EXISTS player_idx ON TABLE sessions COLUMNS player_id;
    ",
    )
    .await?;

    // Games table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS games SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS id ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS game_state ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS lifecycle ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS created_at ON TABLE games TYPE int;
        DEFINE FIELD IF NOT EXISTS created_by ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS player_count ON TABLE games TYPE int;
    ",
    )
    .await?;

    // Game players join table
    db.query("
        DEFINE TABLE IF NOT EXISTS game_players SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE game_players TYPE string;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE game_players TYPE string;
        DEFINE FIELD IF NOT EXISTS player_pid ON TABLE game_players TYPE int;
        DEFINE INDEX IF NOT EXISTS game_player_idx ON TABLE game_players COLUMNS game_id, player_id UNIQUE;
    ").await?;

    // Game events table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS game_events SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE game_events TYPE string;
        DEFINE FIELD IF NOT EXISTS timestamp ON TABLE game_events TYPE int;
        DEFINE FIELD IF NOT EXISTS event ON TABLE game_events TYPE object;
        DEFINE INDEX IF NOT EXISTS game_time_idx ON TABLE game_events COLUMNS game_id, timestamp;
    ",
    )
    .await?;

    // Chat messages table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS chat_messages SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE chat_messages TYPE string;
        DEFINE FIELD IF NOT EXISTS timestamp ON TABLE chat_messages TYPE int;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE chat_messages TYPE string;
        DEFINE FIELD IF NOT EXISTS displayname ON TABLE chat_messages TYPE string;
        DEFINE FIELD IF NOT EXISTS message ON TABLE chat_messages TYPE string;
        DEFINE INDEX IF NOT EXISTS chat_game_idx ON TABLE chat_messages COLUMNS game_id, timestamp;
    ",
    )
    .await?;

    // Snapshots table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS snapshots SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE snapshots TYPE string;
        DEFINE FIELD IF NOT EXISTS index ON TABLE snapshots TYPE int;
        DEFINE FIELD IF NOT EXISTS timestamp ON TABLE snapshots TYPE int;
        DEFINE FIELD IF NOT EXISTS snapshot_data ON TABLE snapshots TYPE string;
        DEFINE INDEX IF NOT EXISTS snapshot_idx ON TABLE snapshots COLUMNS game_id, index UNIQUE;
    ",
    )
    .await?;

    // Matchmaking queue table
    db.query("
        DEFINE TABLE IF NOT EXISTS matchmaking_queue SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE matchmaking_queue TYPE string;
        DEFINE FIELD IF NOT EXISTS queued_at ON TABLE matchmaking_queue TYPE int;
        DEFINE FIELD IF NOT EXISTS game_preferences ON TABLE matchmaking_queue TYPE string;
        DEFINE INDEX IF NOT EXISTS player_queue_idx ON TABLE matchmaking_queue COLUMNS player_id UNIQUE;
    ").await?;

    // Player stats table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS player_stats SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE player_stats TYPE string;
        DEFINE FIELD IF NOT EXISTS games_played ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE FIELD IF NOT EXISTS games_won ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE FIELD IF NOT EXISTS total_playtime ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE INDEX IF NOT EXISTS stats_player_idx ON TABLE player_stats COLUMNS player_id UNIQUE;
        DEFINE INDEX IF NOT EXISTS wins_idx ON TABLE player_stats COLUMNS games_won;
        DEFINE INDEX IF NOT EXISTS games_idx ON TABLE player_stats COLUMNS games_played;
    ",
    )
    .await?;

    // Mark migrations as applied
    let migration = MigrationRecord {
        version: "v1".to_string(),
        applied_at: crate::common::timestamp(),
    };

    db.create::<Option<MigrationRecord>>("migrations")
        .content(migration)
        .await?;

    tracing::info!("Database migration completed");
    Ok(())
}

// ============================================================================
// Database record types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationRecord {
    version: String,
    applied_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRecord {
    pub id: String,
    pub displayname: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: u64,
    pub elo_rating: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub player_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecord {
    pub id: String,
    pub game_state: String, // JSON-serialized Game
    pub lifecycle: String,  // Serialized GameLifecycle
    pub created_at: u64,
    pub created_by: String,
    pub player_count: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePlayerRecord {
    pub game_id: String,
    pub player_id: String,
    pub player_pid: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEventRecord {
    pub game_id: String,
    pub timestamp: u64,
    pub event: automatafl_api_types::GameEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRecord {
    pub game_id: String,
    pub timestamp: u64,
    pub player_id: String,
    pub displayname: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub game_id: String,
    pub index: usize,
    pub timestamp: u64,
    pub snapshot_data: String, // JSON-serialized GameSnapshot
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingQueueRecord {
    pub player_id: String,
    pub queued_at: u64,
    pub game_preferences: String, // JSON-serialized preferences
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatsRecord {
    pub player_id: String,
    pub games_played: u32,
    pub games_won: u32,
    pub total_playtime: u64,
}

// ============================================================================
// Helper functions for database operations
// ============================================================================

/// Create a new player
pub async fn create_player(
    db: &Surreal<Any>,
    id: Uuid,
    displayname: String,
    password_hash: String,
    is_admin: bool,
) -> Result<(), surrealdb::Error> {
    let player = PlayerRecord {
        id: id.to_string(),
        displayname,
        password_hash,
        is_admin,
        bio: None,
        avatar_url: None,
        created_at: crate::common::timestamp(),
        elo_rating: crate::common::DEFAULT_ELO,
    };

    db.create::<Option<PlayerRecord>>(("players", id.to_string()))
        .content(player)
        .await?;

    // Initialize stats for the player
    let stats = PlayerStatsRecord {
        player_id: id.to_string(),
        games_played: 0,
        games_won: 0,
        total_playtime: 0,
    };

    db.create::<Option<PlayerStatsRecord>>(("player_stats", id.to_string()))
        .content(stats)
        .await?;

    Ok(())
}

/// Find player by displayname
pub async fn find_player_by_displayname(
    db: &Surreal<Any>,
    displayname: String,
) -> Result<Option<PlayerRecord>, surrealdb::Error> {
    let mut result = db
        .query("SELECT * FROM players WHERE displayname = $displayname")
        .bind(("displayname", displayname))
        .await?;

    let players: Vec<PlayerRecord> = result.take(0)?;
    Ok(players.into_iter().next())
}

/// Get player by ID
pub async fn get_player(
    db: &Surreal<Any>,
    id: Uuid,
) -> Result<Option<PlayerRecord>, surrealdb::Error> {
    db.select(("players", id.to_string())).await
}

/// Create a session
pub async fn create_session(
    db: &Surreal<Any>,
    session_id: Uuid,
    player_id: Uuid,
    expires_at: u64,
) -> Result<(), surrealdb::Error> {
    let session = SessionRecord {
        id: session_id.to_string(),
        player_id: player_id.to_string(),
        expires_at,
    };

    db.create::<Option<SessionRecord>>(("sessions", session_id.to_string()))
        .content(session)
        .await?;

    Ok(())
}

/// Get session by ID
pub async fn get_session(
    db: &Surreal<Any>,
    session_id: Uuid,
) -> Result<Option<SessionRecord>, surrealdb::Error> {
    db.select(("sessions", session_id.to_string())).await
}

/// Delete session
pub async fn delete_session(db: &Surreal<Any>, session_id: Uuid) -> Result<(), surrealdb::Error> {
    db.delete::<Option<SessionRecord>>(("sessions", session_id.to_string()))
        .await?;
    Ok(())
}

/// Clean up expired sessions
pub async fn cleanup_expired_sessions(
    db: &Surreal<Any>,
    now: u64,
) -> Result<u64, surrealdb::Error> {
    let mut result = db
        .query("DELETE sessions WHERE expires_at < $now RETURN BEFORE")
        .bind(("now", now))
        .await?;

    let deleted: Vec<SessionRecord> = result.take(0).unwrap_or_default();
    Ok(deleted.len() as u64)
}

/// Create a new game
pub async fn create_game(
    db: &Surreal<Any>,
    game_id: Uuid,
    game_state: &Game,
    lifecycle: &GameLifecycle,
    created_by: Uuid,
    player_count: u8,
) -> Result<(), surrealdb::Error> {
    let game = GameRecord {
        id: game_id.to_string(),
        game_state: serde_json::to_string(game_state).unwrap(),
        lifecycle: serde_json::to_string(lifecycle).unwrap(),
        created_at: crate::timestamp(),
        created_by: created_by.to_string(),
        player_count,
    };

    db.create::<Option<GameRecord>>(("games", game_id.to_string()))
        .content(game)
        .await?;

    Ok(())
}

/// Get game by ID
pub async fn get_game(
    db: &Surreal<Any>,
    game_id: Uuid,
) -> Result<Option<GameRecord>, surrealdb::Error> {
    db.select(("games", game_id.to_string())).await
}

/// Update game state
pub async fn update_game_state(
    db: &Surreal<Any>,
    game_id: Uuid,
    game_state: &Game,
    lifecycle: &GameLifecycle,
) -> Result<(), surrealdb::Error> {
    #[derive(Serialize)]
    struct GameStateUpdate {
        game_state: String,
        lifecycle: String,
    }

    let update = GameStateUpdate {
        game_state: serde_json::to_string(game_state).unwrap(),
        lifecycle: serde_json::to_string(lifecycle).unwrap(),
    };

    let _: Option<GameRecord> = db
        .update(("games", game_id.to_string()))
        .merge(update)
        .await?;

    Ok(())
}

/// Delete game
pub async fn delete_game(db: &Surreal<Any>, game_id: Uuid) -> Result<(), surrealdb::Error> {
    // Wrap all deletes in a transaction for atomicity
    db.query("BEGIN TRANSACTION;").await?;

    // Delete game and related data
    let result = async {
        db.delete::<Option<GameRecord>>(("games", game_id.to_string()))
            .await?;

        // Delete game players
        db.query("DELETE game_players WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        // Delete game events
        db.query("DELETE game_events WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        // Delete chat messages
        db.query("DELETE chat_messages WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        // Delete snapshots
        db.query("DELETE snapshots WHERE game_id = $game_id")
            .bind(("game_id", game_id.to_string()))
            .await?;

        Ok::<(), surrealdb::Error>(())
    }
    .await;

    match result {
        Ok(_) => {
            db.query("COMMIT TRANSACTION;").await?;
            Ok(())
        }
        Err(e) => {
            let _ = db.query("ROLLBACK TRANSACTION;").await;
            Err(e)
        }
    }
}

/// List all games
pub async fn list_games(db: &Surreal<Any>) -> Result<Vec<GameRecord>, surrealdb::Error> {
    let games: Vec<GameRecord> = db.select("games").await?;
    Ok(games)
}

/// Add player to game
pub async fn add_player_to_game(
    db: &Surreal<Any>,
    game_id: Uuid,
    player_id: Uuid,
    player_pid: Pid,
) -> Result<(), surrealdb::Error> {
    let record = GamePlayerRecord {
        game_id: game_id.to_string(),
        player_id: player_id.to_string(),
        player_pid: player_pid.0,
    };

    // Use a composite ID to avoid ID conflicts
    let record_id = format!("{}:{}", game_id, player_id);

    db.create::<Option<GamePlayerRecord>>(("game_players", record_id))
        .content(record)
        .await?;

    Ok(())
}

/// Get players in a game
pub async fn get_game_players(
    db: &Surreal<Any>,
    game_id: Uuid,
) -> Result<Vec<GamePlayerRecord>, surrealdb::Error> {
    let mut result = db
        .query("SELECT * FROM game_players WHERE game_id = $game_id")
        .bind(("game_id", game_id.to_string()))
        .await?;

    let players: Vec<GamePlayerRecord> = result.take(0)?;
    Ok(players)
}

/// Add game event
pub async fn add_game_event(
    db: &Surreal<Any>,
    game_id: Uuid,
    timestamp: u64,
    event: automatafl_api_types::GameEventData,
) -> Result<(), surrealdb::Error> {
    let record = GameEventRecord {
        game_id: game_id.to_string(),
        timestamp,
        event,
    };

    // Let SurrealDB generate unique ID automatically
    db.create::<Option<GameEventRecord>>("game_events")
        .content(record)
        .await?;

    Ok(())
}

/// Get game history with optional filtering
pub async fn get_game_history(
    db: &Surreal<Any>,
    game_id: Uuid,
    since: Option<u64>,
    until: Option<u64>,
    event_kind: Option<String>,
) -> Result<Vec<GameEventRecord>, surrealdb::Error> {
    let mut query = "SELECT * FROM game_events WHERE game_id = $game_id".to_string();

    if since.is_some() {
        query.push_str(" AND timestamp >= $since");
    }
    if until.is_some() {
        query.push_str(" AND timestamp <= $until");
    }
    if event_kind.is_some() {
        query.push_str(" AND event.kind = $event_kind");
    }

    query.push_str(" ORDER BY timestamp ASC");

    let mut result = db
        .query(&query)
        .bind(("game_id", game_id.to_string()))
        .bind(("since", since))
        .bind(("until", until))
        .bind(("event_kind", event_kind))
        .await?;

    let events: Vec<GameEventRecord> = result.take(0)?;
    Ok(events)
}

/// Add chat message
pub async fn add_chat_message(
    db: &Surreal<Any>,
    game_id: Uuid,
    timestamp: u64,
    player_id: Uuid,
    displayname: String,
    message: String,
) -> Result<(), surrealdb::Error> {
    let record = ChatMessageRecord {
        game_id: game_id.to_string(),
        timestamp,
        player_id: player_id.to_string(),
        displayname,
        message,
    };

    // Let SurrealDB generate unique ID automatically
    db.create::<Option<ChatMessageRecord>>("chat_messages")
        .content(record)
        .await?;

    Ok(())
}

/// Get chat messages for a game
pub async fn get_chat_messages(
    db: &Surreal<Any>,
    game_id: Uuid,
) -> Result<Vec<ChatMessageRecord>, surrealdb::Error> {
    let mut result = db
        .query("SELECT * FROM chat_messages WHERE game_id = $game_id ORDER BY timestamp ASC")
        .bind(("game_id", game_id.to_string()))
        .await?;

    let messages: Vec<ChatMessageRecord> = result.take(0)?;
    Ok(messages)
}

/// Save a game snapshot
pub async fn save_snapshot(
    db: &Surreal<Any>,
    game_id: Uuid,
    index: usize,
    timestamp: u64,
    snapshot_data: &str,
) -> Result<(), surrealdb::Error> {
    let record = SnapshotRecord {
        game_id: game_id.to_string(),
        index,
        timestamp,
        snapshot_data: snapshot_data.to_string(),
    };

    let record_id = format!("{}:{}", game_id, index);

    db.create::<Option<SnapshotRecord>>(("snapshots", record_id))
        .content(record)
        .await?;

    Ok(())
}

/// List snapshots for a game
pub async fn list_snapshots(
    db: &Surreal<Any>,
    game_id: Uuid,
) -> Result<Vec<SnapshotRecord>, surrealdb::Error> {
    let mut result = db
        .query("SELECT * FROM snapshots WHERE game_id = $game_id ORDER BY index ASC")
        .bind(("game_id", game_id.to_string()))
        .await?;

    let snapshots: Vec<SnapshotRecord> = result.take(0)?;
    Ok(snapshots)
}

/// Get a specific snapshot
pub async fn get_snapshot(
    db: &Surreal<Any>,
    game_id: Uuid,
    index: usize,
) -> Result<Option<SnapshotRecord>, surrealdb::Error> {
    let record_id = format!("{}:{}", game_id, index);
    db.select(("snapshots", record_id)).await
}

/// Get player stats
pub async fn get_player_stats(
    db: &Surreal<Any>,
    player_id: Uuid,
) -> Result<Option<PlayerStatsRecord>, surrealdb::Error> {
    db.select(("player_stats", player_id.to_string())).await
}

/// Update player stats after a game
/// Get all players (for admin)
pub async fn get_all_players(db: &Surreal<Any>) -> Result<Vec<PlayerRecord>, surrealdb::Error> {
    let players: Vec<PlayerRecord> = db.select("players").await?;
    Ok(players)
}

/// Load game state and player IDs from database
pub async fn load_game_state(
    db: &Surreal<Any>,
    game_id: Uuid,
) -> Result<
    Option<(
        Game,
        GameLifecycle,
        std::collections::HashMap<Uuid, automatafl_logic::Pid>,
    )>,
    surrealdb::Error,
> {
    use automatafl_logic::Pid;
    use std::collections::HashMap;

    let game_record = match get_game(db, game_id).await? {
        Some(g) => g,
        None => return Ok(None),
    };

    let game_state: Game = serde_json::from_str(&game_record.game_state)
        .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;

    let lifecycle: GameLifecycle = serde_json::from_str(&game_record.lifecycle)
        .map_err(|e| surrealdb::Error::new(surrealdb::error::Db::Thrown(e.to_string())))?;

    let game_players = get_game_players(db, game_id).await?;
    let player_ids: HashMap<Uuid, Pid> = game_players
        .iter()
        .filter_map(|gp| {
            Uuid::parse_str(&gp.player_id)
                .ok()
                .map(|id| (id, Pid(gp.player_pid)))
        })
        .collect();

    Ok(Some((game_state, lifecycle, player_ids)))
}

/// Update player profile
pub async fn update_player_profile(
    db: &Surreal<Any>,
    player_id: Uuid,
    bio: Option<String>,
    avatar_url: Option<String>,
) -> Result<(), surrealdb::Error> {
    #[derive(Serialize)]
    struct ProfileUpdate {
        bio: Option<String>,
        avatar_url: Option<String>,
    }

    let update = ProfileUpdate { bio, avatar_url };

    let _: Option<PlayerRecord> = db
        .update(("players", player_id.to_string()))
        .merge(update)
        .await?;

    Ok(())
}

/// Update player ELO rating
pub async fn update_player_elo(
    db: &Surreal<Any>,
    player_id: Uuid,
    new_elo: i32,
) -> Result<(), surrealdb::Error> {
    #[derive(Serialize)]
    struct EloUpdate {
        elo_rating: i32,
    }

    let update = EloUpdate { elo_rating: new_elo };

    let _: Option<PlayerRecord> = db
        .update(("players", player_id.to_string()))
        .merge(update)
        .await?;

    Ok(())
}

/// Get leaderboard by ELO rating
pub async fn get_leaderboard_by_elo(
    db: &Surreal<Any>,
    limit: usize,
) -> Result<Vec<(PlayerRecord, usize)>, surrealdb::Error> {
    let mut result = db
        .query("SELECT * FROM players ORDER BY elo_rating DESC LIMIT $limit")
        .bind(("limit", limit))
        .await?;

    let players: Vec<PlayerRecord> = result.take(0)?;
    let ranked: Vec<(PlayerRecord, usize)> = players
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p, i + 1))
        .collect();

    Ok(ranked)
}

/// Get leaderboard by wins
pub async fn get_leaderboard_by_wins(
    db: &Surreal<Any>,
    limit: usize,
) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, surrealdb::Error> {
    let mut result = db
        .query(
            "SELECT players.*, player_stats.* FROM players
         INNER JOIN player_stats ON players.id = player_stats.player_id
         ORDER BY player_stats.games_won DESC LIMIT $limit",
        )
        .bind(("limit", limit))
        .await?;

    #[derive(serde::Deserialize)]
    struct JoinResult {
        #[serde(flatten)]
        player: PlayerRecord,
        #[serde(flatten)]
        stats: PlayerStatsRecord,
    }

    let results: Vec<JoinResult> = result.take(0)?;
    let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r.player, r.stats, i + 1))
        .collect();

    Ok(ranked)
}

/// Get leaderboard by games played
pub async fn get_leaderboard_by_games(
    db: &Surreal<Any>,
    limit: usize,
) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, surrealdb::Error> {
    let mut result = db
        .query(
            "SELECT players.*, player_stats.* FROM players
         INNER JOIN player_stats ON players.id = player_stats.player_id
         ORDER BY player_stats.games_played DESC LIMIT $limit",
        )
        .bind(("limit", limit))
        .await?;

    #[derive(serde::Deserialize)]
    struct JoinResult {
        #[serde(flatten)]
        player: PlayerRecord,
        #[serde(flatten)]
        stats: PlayerStatsRecord,
    }

    let results: Vec<JoinResult> = result.take(0)?;
    let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r.player, r.stats, i + 1))
        .collect();

    Ok(ranked)
}

// ============================================================================
// Matchmaking functions
// ============================================================================

/// Join matchmaking queue
pub async fn join_matchmaking_queue(
    db: &Surreal<Any>,
    player_id: Uuid,
    queued_at: u64,
    preferences: String,
) -> Result<(), surrealdb::Error> {
    let record = MatchmakingQueueRecord {
        player_id: player_id.to_string(),
        queued_at,
        game_preferences: preferences,
    };

    db.create::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id.to_string()))
        .content(record)
        .await?;

    Ok(())
}

/// Leave matchmaking queue
pub async fn leave_matchmaking_queue(
    db: &Surreal<Any>,
    player_id: Uuid,
) -> Result<(), surrealdb::Error> {
    db.delete::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id.to_string()))
        .await?;
    Ok(())
}

/// Get matchmaking queue status for a player
pub async fn get_matchmaking_status(
    db: &Surreal<Any>,
    player_id: Uuid,
) -> Result<Option<MatchmakingQueueRecord>, surrealdb::Error> {
    db.select(("matchmaking_queue", player_id.to_string()))
        .await
}

/// Get all players in matchmaking queue
pub async fn get_matchmaking_queue(
    db: &Surreal<Any>,
) -> Result<Vec<MatchmakingQueueRecord>, surrealdb::Error> {
    let queue: Vec<MatchmakingQueueRecord> = db.select("matchmaking_queue").await?;
    Ok(queue)
}

/// Remove multiple players from queue (after matching)
pub async fn remove_from_queue(
    db: &Surreal<Any>,
    player_ids: Vec<Uuid>,
) -> Result<(), surrealdb::Error> {
    for player_id in player_ids {
        let _ = db
            .delete::<Option<MatchmakingQueueRecord>>(("matchmaking_queue", player_id.to_string()))
            .await;
    }
    Ok(())
}

// ============================================================================
// Live Queries for Real-Time Events
// ============================================================================

/// Create a live query stream for game events
/// Returns a stream of new events as they're inserted into the database
pub async fn create_live_query(
    db: Db,
    game_id: Uuid,
) -> Result<impl Stream<Item = Result<surrealdb::Notification<GameEventRecord>, surrealdb::Error>>, surrealdb::Error> {
    let mut result = db
        .query("LIVE SELECT * FROM game_events WHERE game_id = $game_id ORDER BY timestamp")
        .bind(("game_id", game_id.to_string()))
        .await?;

    let stream = result.stream::<surrealdb::Notification<GameEventRecord>>(0)?;
    Ok(stream)
}

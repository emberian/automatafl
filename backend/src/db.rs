use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use surrealdb::{
    RecordId, Surreal,
    engine::any::{Any, connect},
};
use uuid::Uuid;

pub type Db = Arc<Surreal<Any>>;

/// Extract UUID from RecordId
/// SAFETY: This unwrap is safe because we only use RecordIds created with
/// RecordId::from_table_key(table, uuid) which guarantees the key is a UUID
pub fn as_uuid(id: &RecordId) -> Uuid {
    let key = id.key().into_inner_ref();
    key.clone()
        .into_value()
        .into_uuid()
        .expect("RecordId should contain UUID when created from from_table_key")
        .into()
}

/// Convert a slice of RecordIds to UUIDs efficiently
pub fn as_uuids(ids: &[RecordId]) -> Vec<Uuid> {
    ids.iter().map(as_uuid).collect()
}

/// Convert UUIDs to RecordId strings for batch queries
pub fn uuids_to_record_strings(uuids: &[Uuid]) -> Vec<String> {
    uuids
        .iter()
        .map(|uuid| format!("players/{}", uuid))
        .collect()
}

/// Extract UUID from a RecordId with better error handling
pub fn try_as_uuid(id: &RecordId) -> Option<Uuid> {
    let key = id.key().into_inner_ref();
    key.clone().into_value().into_uuid().map(|uuid| uuid.into())
}

/// Initialize the database - either embedded SurrealKV or external connection
pub async fn init_db(uri: Option<String>) -> Result<Db, surrealdb::Error> {
    let db_uri = uri.unwrap_or_else(|| "surrealkv://automatafl.db".to_string());

    tracing::info!("Connecting to database: {}", db_uri);
    let db = connect(&db_uri).await?;

    // // Sign in as root for embedded database
    // if db_uri.starts_with("surrealkv://") {
    //     db.signin(Root {
    //         username: "root",
    //         password: "root",
    //     })
    //     .await?;
    // }

    // Use namespace and database
    db.use_ns("automatafl").use_db("main").await?;

    // Run migrations
    migrate(&db).await?;

    Ok(Arc::new(db))
}

/// Database schema and migrations
async fn migrate(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    let applied_existing: Vec<MigrationRecord> = match db.select("migrations").await {
        Ok(records) => records,
        Err(_) => Vec::new(),
    };

    let versions: Vec<String> = applied_existing
        .into_iter()
        .map(|migration| migration.version)
        .collect();

    let mut applied_any = false;

    if !versions.iter().any(|v| v == "v1") {
        tracing::info!("Applying database migration v1...");
        apply_v1(db).await?;
        record_migration(db, "v1").await?;
        applied_any = true;
    }

    if !versions.iter().any(|v| v == "v2") {
        tracing::info!("Applying database migration v2...");
        apply_v2(db).await?;
        record_migration(db, "v2").await?;
        applied_any = true;
    }

    if !versions.iter().any(|v| v == "v3") {
        tracing::info!("Applying database migration v3...");
        apply_v3(db).await?;
        record_migration(db, "v3").await?;
        applied_any = true;
    }

    if !versions.iter().any(|v| v == "v4") {
        tracing::info!("Applying database migration v4...");
        apply_v4(db).await?;
        record_migration(db, "v4").await?;
        applied_any = true;
    }

    if applied_any {
        tracing::info!("Database migrations applied");
    } else {
        tracing::info!("Database migrations already current");
    }

    Ok(())
}

async fn apply_v1(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    // Define tables and indexes for initial schema

    // Players table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS players SCHEMAFULL;
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
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE sessions TYPE record<players>;
        DEFINE FIELD IF NOT EXISTS expires_at ON TABLE sessions TYPE int;
        DEFINE INDEX IF NOT EXISTS player_idx ON TABLE sessions COLUMNS player_id;
    ",
    )
    .await?;

    // Games table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS games SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_state ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS lifecycle ON TABLE games TYPE string;
        DEFINE FIELD IF NOT EXISTS created_at ON TABLE games TYPE int;
        DEFINE FIELD IF NOT EXISTS created_by ON TABLE games TYPE record<players>;
        DEFINE FIELD IF NOT EXISTS player_count ON TABLE games TYPE int;
    ",
    )
    .await?;

    // Game players join table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS game_players SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE game_players TYPE record<games>;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE game_players TYPE record<players>;
        DEFINE FIELD IF NOT EXISTS player_pid ON TABLE game_players TYPE int;
        DEFINE INDEX IF NOT EXISTS game_player_idx ON TABLE game_players COLUMNS game_id, player_id UNIQUE;
    ",
    )
    .await?;

    // Game events table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS game_events SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE game_events TYPE record<games>;
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
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE chat_messages TYPE record<games>;
        DEFINE FIELD IF NOT EXISTS timestamp ON TABLE chat_messages TYPE int;
    DEFINE FIELD IF NOT EXISTS player_id ON TABLE chat_messages TYPE record<players>;
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
        DEFINE FIELD IF NOT EXISTS game_id ON TABLE snapshots TYPE record<games>;
        DEFINE FIELD IF NOT EXISTS index ON TABLE snapshots TYPE int;
        DEFINE FIELD IF NOT EXISTS timestamp ON TABLE snapshots TYPE int;
        DEFINE FIELD IF NOT EXISTS snapshot_data ON TABLE snapshots TYPE string;
        DEFINE INDEX IF NOT EXISTS snapshot_idx ON TABLE snapshots COLUMNS game_id, index UNIQUE;
    ",
    )
    .await?;

    // Matchmaking queue table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS matchmaking_queue SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE matchmaking_queue TYPE record<players>;
        DEFINE FIELD IF NOT EXISTS queued_at ON TABLE matchmaking_queue TYPE int;
        DEFINE FIELD IF NOT EXISTS game_preferences ON TABLE matchmaking_queue TYPE string;
        DEFINE INDEX IF NOT EXISTS player_queue_idx ON TABLE matchmaking_queue COLUMNS player_id UNIQUE;
    ",
    )
    .await?;

    // Player stats table
    db.query(
        "
        DEFINE TABLE IF NOT EXISTS player_stats SCHEMAFULL;
        DEFINE FIELD IF NOT EXISTS player_id ON TABLE player_stats TYPE record<players>;
        DEFINE FIELD IF NOT EXISTS games_played ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE FIELD IF NOT EXISTS games_won ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE FIELD IF NOT EXISTS total_playtime ON TABLE player_stats TYPE int DEFAULT 0;
        DEFINE INDEX IF NOT EXISTS player_stats_idx ON TABLE player_stats COLUMNS player_id UNIQUE;
        DEFINE INDEX IF NOT EXISTS wins_idx ON TABLE player_stats COLUMNS games_won;
        DEFINE INDEX IF NOT EXISTS games_idx ON TABLE player_stats COLUMNS games_played;
    ",
    )
    .await?;

    Ok(())
}

async fn apply_v2(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    db.query(
        "
        DEFINE FIELD IF NOT EXISTS password_salt ON TABLE players TYPE option<string>;
    ",
    )
    .await?;

    Ok(())
}

async fn apply_v3(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    db.query(
        "
        DEFINE FIELD player_id ON TABLE chat_messages TYPE record<players>;
    ",
    )
    .await?;

    Ok(())
}

async fn apply_v4(db: &Surreal<Any>) -> Result<(), surrealdb::Error> {
    // Add performance indexes for frequently queried columns
    db.query(
        "
        DEFINE INDEX IF NOT EXISTS created_at_idx ON TABLE games COLUMNS created_at;
        DEFINE INDEX IF NOT EXISTS lifecycle_idx ON TABLE games COLUMNS lifecycle;
        DEFINE INDEX IF NOT EXISTS expires_at_idx ON TABLE sessions COLUMNS expires_at;
        DEFINE INDEX IF NOT EXISTS queued_at_idx ON TABLE matchmaking_queue COLUMNS queued_at;
    ",
    )
    .await?;

    Ok(())
}

async fn record_migration(db: &Surreal<Any>, version: &str) -> Result<(), surrealdb::Error> {
    let migration = MigrationRecord {
        version: version.to_string(),
        applied_at: crate::common::timestamp(),
    };

    db.create::<Option<MigrationRecord>>("migrations")
        .content(migration)
        .await?;

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
    pub id: RecordId,
    pub displayname: String,
    pub password_hash: String,
    pub password_salt: Option<String>,
    pub is_admin: bool,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: u64,
    pub elo_rating: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: RecordId,
    pub player_id: RecordId,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRecord {
    pub id: RecordId,
    pub game_state: String, // JSON-serialized Game
    pub lifecycle: String,  // Serialized GameLifecycle
    pub created_at: u64,
    pub created_by: RecordId,
    pub player_count: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePlayerRecord {
    pub game_id: RecordId,
    pub player_id: RecordId,
    pub player_pid: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEventRecord {
    pub game_id: RecordId,
    pub timestamp: u64,
    pub event: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRecord {
    pub game_id: RecordId,
    pub timestamp: u64,
    pub player_id: RecordId,
    pub displayname: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub game_id: RecordId,
    pub index: usize,
    pub timestamp: u64,
    pub snapshot_data: String, // JSON-serialized GameSnapshot
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingQueueRecord {
    pub player_id: RecordId,
    pub queued_at: u64,
    pub game_preferences: String, // JSON-serialized preferences
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatsRecord {
    pub player_id: RecordId,
    pub games_played: u32,
    pub games_won: u32,
    pub total_playtime: u64,
}

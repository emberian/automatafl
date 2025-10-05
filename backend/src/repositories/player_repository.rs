use crate::db::{Db, PlayerRecord, PlayerStatsRecord};
use serde_json::Value;
use surrealdb::RecordId;
use uuid::Uuid;

/// Repository for player-related database operations
pub struct PlayerRepository {
    db: Db,
}

/// Stats update for a single player
pub struct PlayerStatsUpdate {
    pub player_id: Uuid,
    pub won: bool,
    pub playtime: u64,
    pub new_elo: Option<i32>,
}

impl PlayerRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Create a new player and initialize stats record
    pub async fn create(
        &self,
        player_id: Uuid,
        displayname: String,
        password_hash: String,
        password_salt: String,
        is_admin: bool,
    ) -> Result<(), surrealdb::Error> {
        let player_record = PlayerRecord {
            id: RecordId::from_table_key("players", player_id),
            displayname,
            password_hash,
            password_salt: Some(password_salt),
            is_admin,
            bio: None,
            avatar_url: None,
            created_at: crate::common::timestamp(),
            elo_rating: crate::common::DEFAULT_ELO,
        };

        self.db
            .create::<Option<PlayerRecord>>(("players", player_id))
            .content(player_record)
            .await?;

        let stats_record = PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        };

        self.db
            .create::<Option<PlayerStatsRecord>>(("player_stats", player_id))
            .content(stats_record)
            .await?;

        Ok(())
    }

    /// Get player by ID
    pub async fn get(&self, player_id: Uuid) -> Result<Option<PlayerRecord>, surrealdb::Error> {
        self.db.select(("players", player_id)).await
    }

    /// Find player by displayname
    pub async fn find_by_displayname(
        &self,
        displayname: String,
    ) -> Result<Option<PlayerRecord>, surrealdb::Error> {
        let mut result = self
            .db
            .query("SELECT * FROM players WHERE displayname = $displayname")
            .bind(("displayname", displayname))
            .await?;

        let players: Vec<PlayerRecord> = result.take(0)?;
        Ok(players.into_iter().next())
    }

    /// Get player stats
    pub async fn get_stats(
        &self,
        player_id: Uuid,
    ) -> Result<Option<PlayerStatsRecord>, surrealdb::Error> {
        self.db.select(("player_stats", player_id)).await
    }

    /// List all players (admin use case)
    pub async fn list_all(&self) -> Result<Vec<PlayerRecord>, surrealdb::Error> {
        self.db.select("players").await
    }

    /// Update multiple player stats atomically in a single batch transaction
    /// This is the key improvement - all stats are updated together or not at all
    pub async fn update_stats_batch(
        &self,
        updates: &[PlayerStatsUpdate],
    ) -> Result<(), surrealdb::Error> {
        if updates.is_empty() {
            return Ok(());
        }

        // Get all existing stats in a single query
        let player_ids: Vec<RecordId> = updates
            .iter()
            .map(|u| RecordId::from_table_key("players", u.player_id))
            .collect();
        let mut result = self
            .db
            .query("SELECT * FROM player_stats WHERE player_id IN $player_ids")
            .bind(("player_ids", player_ids))
            .await?;
        let existing_stats: Vec<PlayerStatsRecord> = result.take(0).unwrap_or_default();

        // Create a map for quick lookup using as_uuid helper
        let mut stats_map = std::collections::HashMap::new();
        for stats in existing_stats {
            let player_id = crate::db::as_uuid(&stats.player_id);
            stats_map.insert(player_id, stats);
        }

        // Build transaction with properly parameterized queries
        // SECURITY: All values are bound as parameters, not interpolated
        self.db.query("BEGIN TRANSACTION;").await?;

        let outcome =
            async {
                for (idx, update) in updates.iter().enumerate() {
                    let existing = stats_map.get(&update.player_id);

                    // Calculate new values
                    let new_games_played = if let Some(existing) = existing {
                        existing.games_played + 1
                    } else {
                        1
                    };

                    let new_games_won = if let Some(existing) = existing {
                        existing.games_won + if update.won { 1 } else { 0 }
                    } else {
                        if update.won { 1 } else { 0 }
                    };

                    let new_total_playtime = if let Some(existing) = existing {
                        existing.total_playtime + update.playtime
                    } else {
                        update.playtime
                    };

                    // FIXED: Use proper parameterized queries
                    self.db
                        .query(
                            "UPDATE player_stats SET 
                            games_played = $games_played,
                            games_won = $games_won,
                            total_playtime = $total_playtime
                        WHERE player_id = $player_id",
                        )
                        .bind((
                            "player_id",
                            RecordId::from_table_key("players", update.player_id),
                        ))
                        .bind(("games_played", new_games_played))
                        .bind(("games_won", new_games_won))
                        .bind(("total_playtime", new_total_playtime))
                        .await?;

                    // Create stats record if it doesn't exist
                    if existing.is_none() {
                        self.db
                        .query(
                            "IF (SELECT * FROM player_stats WHERE player_id = $player_id) = [] THEN
                                CREATE player_stats SET
                                    player_id = $player_id,
                                    games_played = $games_played,
                                    games_won = $games_won,
                                    total_playtime = $total_playtime
                            END"
                        )
                        .bind(("player_id", RecordId::from_table_key("players", update.player_id)))
                        .bind(("games_played", new_games_played))
                        .bind(("games_won", new_games_won))
                        .bind(("total_playtime", new_total_playtime))
                        .await?;
                    }

                    // Update ELO if provided
                    if let Some(new_elo) = update.new_elo {
                        self.db
                            .query("UPDATE players SET elo_rating = $elo WHERE id = $player_id")
                            .bind((
                                "player_id",
                                RecordId::from_table_key("players", update.player_id),
                            ))
                            .bind(("elo", new_elo))
                            .await?;
                    }
                }
                Ok::<(), surrealdb::Error>(())
            }
            .await;

        match outcome {
            Ok(()) => {
                self.db.query("COMMIT TRANSACTION;").await?;
                Ok(())
            }
            Err(err) => {
                let _ = self.db.query("ROLLBACK TRANSACTION;").await;
                Err(err)
            }
        }
    }

    /// Update player profile
    pub async fn update_profile(
        &self,
        player_id: Uuid,
        bio: Option<String>,
        avatar_url: Option<String>,
    ) -> Result<(), surrealdb::Error> {
        let _: Option<PlayerRecord> = self
            .db
            .update(("players", player_id))
            .merge(serde_json::json!({
                "bio": bio,
                "avatar_url": avatar_url,
            }))
            .await?;

        Ok(())
    }

    /// Update arbitrary admin-managed fields on a player record
    pub async fn update_admin_fields(
        &self,
        player_id: Uuid,
        displayname: Option<String>,
        is_admin: Option<bool>,
        bio: Option<String>,
        avatar_url: Option<String>,
        elo_rating: Option<i32>,
    ) -> Result<(), surrealdb::Error> {
        let mut updates = serde_json::Map::new();

        if let Some(value) = displayname {
            updates.insert("displayname".to_string(), Value::String(value));
        }
        if let Some(value) = is_admin {
            updates.insert("is_admin".to_string(), Value::Bool(value));
        }
        if let Some(value) = bio {
            updates.insert("bio".to_string(), Value::String(value));
        }
        if let Some(value) = avatar_url {
            updates.insert("avatar_url".to_string(), Value::String(value));
        }
        if let Some(value) = elo_rating {
            updates.insert(
                "elo_rating".to_string(),
                Value::Number((value as i64).into()),
            );
        }

        if updates.is_empty() {
            return Ok(());
        }

        let _: Option<PlayerRecord> = self
            .db
            .update(("players", player_id))
            .merge(Value::Object(updates))
            .await?;

        Ok(())
    }

    /// Remove a player record entirely
    pub async fn delete(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db
            .delete::<Option<PlayerRecord>>(("players", player_id))
            .await?;
        Ok(())
    }

    /// Remove stats for a player
    pub async fn delete_stats(&self, player_id: Uuid) -> Result<(), surrealdb::Error> {
        self.db
            .delete::<Option<PlayerStatsRecord>>(("player_stats", player_id))
            .await?;
        Ok(())
    }

    /// Get leaderboard by ELO rating
    pub async fn get_leaderboard_by_elo(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, usize)>, surrealdb::Error> {
        let mut result = self
            .db
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
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, surrealdb::Error> {
        // Use a single query with JOIN to fetch both player and stats
        let query = r#"
            SELECT
                player_id,
                games_won,
                games_played,
                total_playtime,
                ->players.* AS player
            FROM player_stats
            ORDER BY games_won DESC
            LIMIT $limit
        "#;

        let mut result = self.db.query(query).bind(("limit", limit as i64)).await?;

        #[derive(serde::Deserialize)]
        struct LeaderboardRow {
            player_id: surrealdb::RecordId,
            games_won: u32,
            games_played: u32,
            total_playtime: u64,
            player: Vec<PlayerRecord>,
        }

        let rows: Vec<LeaderboardRow> = result.take(0).unwrap_or_default();
        let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = rows
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| {
                row.player.into_iter().next().map(|player| {
                    let stats = PlayerStatsRecord {
                        player_id: row.player_id,
                        games_won: row.games_won,
                        games_played: row.games_played,
                        total_playtime: row.total_playtime,
                    };
                    (player, stats, index + 1)
                })
            })
            .collect();

        Ok(ranked)
    }

    /// Get leaderboard by games played
    pub async fn get_leaderboard_by_games(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, surrealdb::Error> {
        // Use a single query with JOIN to fetch both player and stats
        let query = r#"
            SELECT
                player_id,
                games_won,
                games_played,
                total_playtime,
                ->players.* AS player
            FROM player_stats
            ORDER BY games_played DESC
            LIMIT $limit
        "#;

        let mut result = self.db.query(query).bind(("limit", limit as i64)).await?;

        #[derive(serde::Deserialize)]
        struct LeaderboardRow {
            player_id: surrealdb::RecordId,
            games_won: u32,
            games_played: u32,
            total_playtime: u64,
            player: Vec<PlayerRecord>,
        }

        let rows: Vec<LeaderboardRow> = result.take(0).unwrap_or_default();
        let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = rows
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| {
                row.player.into_iter().next().map(|player| {
                    let stats = PlayerStatsRecord {
                        player_id: row.player_id,
                        games_won: row.games_won,
                        games_played: row.games_played,
                        total_playtime: row.total_playtime,
                    };
                    (player, stats, index + 1)
                })
            })
            .collect();

        Ok(ranked)
    }

    /// Update selected fields on player stats
    pub async fn update_stats_fields(
        &self,
        player_id: Uuid,
        games_played: Option<u32>,
        games_won: Option<u32>,
        total_playtime: Option<u64>,
    ) -> Result<(), surrealdb::Error> {
        let mut updates = serde_json::Map::new();

        if let Some(value) = games_played {
            updates.insert(
                "games_played".to_string(),
                Value::Number((value as u64).into()),
            );
        }
        if let Some(value) = games_won {
            updates.insert(
                "games_won".to_string(),
                Value::Number((value as u64).into()),
            );
        }
        if let Some(value) = total_playtime {
            updates.insert("total_playtime".to_string(), Value::Number(value.into()));
        }

        if updates.is_empty() {
            return Ok(());
        }

        let _: Option<PlayerStatsRecord> = self
            .db
            .update(("player_stats", player_id))
            .merge(Value::Object(updates))
            .await?;

        Ok(())
    }
}

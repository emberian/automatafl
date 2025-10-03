use crate::db::{Db, PlayerRecord, PlayerStatsRecord};
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
        is_admin: bool,
    ) -> Result<(), surrealdb::Error> {
        let player_record = PlayerRecord {
            id: RecordId::from_table_key("players", player_id),
            displayname,
            password_hash,
            is_admin,
            bio: None,
            avatar_url: None,
            created_at: crate::common::timestamp(),
            elo_rating: crate::common::DEFAULT_ELO,
        };

        self
            .db
            .create::<Option<PlayerRecord>>(("players", player_id))
            .content(player_record)
            .await?;

        let stats_record = PlayerStatsRecord {
            player_id: RecordId::from_table_key("players", player_id),
            games_played: 0,
            games_won: 0,
            total_playtime: 0,
        };

        self
            .db
            .create::<Option<PlayerStatsRecord>>(("player_stats", player_id))
            .content(stats_record)
            .await?;

        Ok(())
    }

    /// Get player by ID
    pub async fn get(&self, player_id: Uuid) -> Result<Option<PlayerRecord>, surrealdb::Error> {
        self.db.select(("players", player_id.to_string())).await
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
        self.db
            .select(("player_stats", player_id.to_string()))
            .await
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

        // Build a single transaction that updates all players
        let mut statements = vec!["BEGIN TRANSACTION;".to_string()];

        for (idx, update) in updates.iter().enumerate() {

            // Upsert stats
            statements.push(format!(
                r#"
                LET $stats{idx} = (SELECT * FROM player_stats WHERE player_id = $player_id{idx})[0];
                IF $stats{idx} THEN
                    UPDATE player_stats SET
                        games_played += 1,
                        games_won += $won_delta{idx},
                        total_playtime += $playtime{idx}
                    WHERE player_id = $player_id{idx}
                ELSE
                    CREATE player_stats SET
                        player_id = $player_id{idx},
                        games_played = 1,
                        games_won = $won_delta{idx},
                        total_playtime = $playtime{idx}
                END;
                "#,
                idx = idx
            ));

            // Update ELO if provided
            if update.new_elo.is_some() {
                statements.push(format!(
                    "IF $elo{idx} != NONE THEN UPDATE players SET elo_rating = $elo{idx} WHERE id = $player_id{idx}; END;",
                    idx = idx
                ));
            }
        }

        statements.push("COMMIT TRANSACTION;".to_string());

        let query = statements.join("\n");
        let mut request = self.db.query(query);

        // Bind all parameters
        for (idx, update) in updates.iter().enumerate() {
            request = request
                .bind((format!("player_id{}", idx), update.player_id.to_string()))
                .bind((format!("won_delta{}", idx), if update.won { 1 } else { 0 }))
                .bind((format!("playtime{}", idx), update.playtime))
                .bind((format!("elo{}", idx), update.new_elo));
        }

        request.await?;

        Ok(())
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
            .update(("players", player_id.to_string()))
            .merge(serde_json::json!({
                "bio": bio,
                "avatar_url": avatar_url,
            }))
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
        let query = r#"
            SELECT
                players.*,
                player_stats.*
            FROM player_stats
            INNER JOIN players ON player_stats.player_id = players.id
            ORDER BY player_stats.games_won DESC
            LIMIT $limit
        "#;

        let mut result = self.db.query(query).bind(("limit", limit)).await?;

        #[derive(serde::Deserialize)]
        struct Row {
            #[serde(flatten)]
            player: PlayerRecord,
            #[serde(flatten)]
            stats: PlayerStatsRecord,
        }

        let rows: Vec<Row> = result.take(0).unwrap_or_default();
        let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| (r.player, r.stats, i + 1))
            .collect();

        Ok(ranked)
    }

    /// Get leaderboard by games played
    pub async fn get_leaderboard_by_games(
        &self,
        limit: usize,
    ) -> Result<Vec<(PlayerRecord, PlayerStatsRecord, usize)>, surrealdb::Error> {
        let query = r#"
            SELECT
                players.*,
                player_stats.*
            FROM player_stats
            INNER JOIN players ON player_stats.player_id = players.id
            ORDER BY player_stats.games_played DESC
            LIMIT $limit
        "#;

        let mut result = self.db.query(query).bind(("limit", limit)).await?;

        #[derive(serde::Deserialize)]
        struct Row {
            #[serde(flatten)]
            player: PlayerRecord,
            #[serde(flatten)]
            stats: PlayerStatsRecord,
        }

        let rows: Vec<Row> = result.take(0).unwrap_or_default();
        let ranked: Vec<(PlayerRecord, PlayerStatsRecord, usize)> = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| (r.player, r.stats, i + 1))
            .collect();

        Ok(ranked)
    }
}

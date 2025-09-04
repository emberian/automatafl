// This module is reserved for future database utilities and helpers
// Currently, all database operations are done directly in handlers

use sqlx::SqlitePool;

pub async fn init_db(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePool::connect(database_url).await?;
    Ok(pool)
}

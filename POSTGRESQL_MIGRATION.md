# PostgreSQL Migration Guide

## Why Migrate from SQLite to PostgreSQL?

SQLite is excellent for development but has fundamental limitations for production:
- **Write concurrency**: Only one writer at a time
- **Network access**: Not designed for network access
- **Scalability**: Cannot scale horizontally
- **Advanced features**: Lacks advanced indexing, full-text search, etc.

## Migration Steps

### 1. Update Dependencies

```toml
# backend/Cargo.toml
[dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres", "migrate", "uuid", "chrono"] }
# Remove "sqlite" feature
```

### 2. Update Connection Code

```rust
// src/main.rs
use sqlx::postgres::PgPoolOptions; // Changed from SqlitePoolOptions

// In main()
let pool = PgPoolOptions::new()
    .max_connections(20)  // PostgreSQL handles more connections better
    .connect(&database_url)
    .await?;

// Remove SQLite-specific PRAGMAs
```

### 3. Convert SQL Migrations

Key differences to address:
- UUID type: PostgreSQL has native UUID support
- TEXT vs VARCHAR: PostgreSQL distinguishes between them
- AUTOINCREMENT: Use SERIAL or IDENTITY in PostgreSQL
- Datetime: Use TIMESTAMPTZ for timezone-aware timestamps

Example conversion:

```sql
-- SQLite (current)
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    created_at TEXT NOT NULL
);

-- PostgreSQL (new)
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### 4. Update Type Mappings

```rust
// src/state.rs
use sqlx::PgPool; // Changed from SqlitePool

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,  // Changed type
    // ... rest remains the same
}
```

### 5. Fix Query Differences

PostgreSQL uses `$1, $2, $3...` for parameters instead of `?`:

```rust
// Before (SQLite)
sqlx::query!("SELECT * FROM users WHERE id = ?", user_id)

// After (PostgreSQL)  
sqlx::query!("SELECT * FROM users WHERE id = $1", user_id)
```

### 6. Handle FOR UPDATE Properly

PostgreSQL supports row-level locking natively:

```sql
-- Add to queries that need atomic updates
SELECT * FROM games WHERE id = $1 FOR UPDATE;
```

### 7. Environment Setup

```bash
# .env
DATABASE_URL=postgresql://user:password@localhost/automatafl_db

# Docker Compose for local development
version: '3.8'
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: automatafl
      POSTGRES_PASSWORD: secure_password
      POSTGRES_DB: automatafl_db
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

### 8. Data Migration Script

```rust
// tools/migrate_data.rs
use sqlx::{SqlitePool, PgPool};

async fn migrate_data(sqlite_pool: SqlitePool, pg_pool: PgPool) -> Result<()> {
    // Migrate users
    let users = sqlx::query!("SELECT * FROM users")
        .fetch_all(&sqlite_pool)
        .await?;
    
    for user in users {
        sqlx::query!(
            "INSERT INTO users (id, username, password_hash, rating, created_at) 
             VALUES ($1, $2, $3, $4, $5)",
            user.id,
            user.username,
            user.password_hash,
            user.rating,
            user.created_at
        )
        .execute(&pg_pool)
        .await?;
    }
    
    // Repeat for other tables...
    Ok(())
}
```

## Testing Strategy

1. **Parallel Testing**: Run both databases in parallel initially
2. **Load Testing**: Use tools like `vegeta` or `k6` to verify performance improvements
3. **Backup Strategy**: Implement regular pg_dump backups
4. **Monitoring**: Add connection pool metrics to your Prometheus setup

## Production Deployment

Consider managed PostgreSQL services:
- **AWS RDS**: Automated backups, multi-AZ deployments
- **Supabase**: Built on PostgreSQL with additional features
- **DigitalOcean Managed Databases**: Simple and cost-effective
- **Fly.io Postgres**: Close to your app instances

## Rollback Plan

1. Keep SQLite database file as backup
2. Maintain ability to switch via DATABASE_URL
3. Test rollback procedure before going live

## Performance Expectations

After migration, expect:
- **10-100x** better write concurrency
- **Connection pooling** actually working effectively  
- **Sub-millisecond** query times with proper indexing
- Ability to handle **thousands of concurrent games**

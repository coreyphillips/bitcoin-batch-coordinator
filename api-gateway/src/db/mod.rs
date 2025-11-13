use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tracing::info;

pub mod schema;
pub mod queries;

/// Initialize the database and run migrations
pub async fn init_database(database_url: &str) -> Result<SqlitePool> {
    // Create connection pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    // Run migrations
    info!("📝 Running database migrations...");
    run_migrations(&pool).await?;

    Ok(pool)
}

/// Run database migrations
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    // Create batches table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS batches (
            id TEXT PRIMARY KEY,
            intent_data TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,
            state TEXT NOT NULL,
            txid TEXT,
            total_fees INTEGER,
            participant_count INTEGER DEFAULT 0,
            raw_transaction TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create batch_participants table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS batch_participants (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id TEXT NOT NULL,
            pubkey TEXT NOT NULL,
            joined_at INTEGER NOT NULL,
            commitment TEXT,
            reveal TEXT,
            signed BOOLEAN DEFAULT FALSE,
            FOREIGN KEY (batch_id) REFERENCES batches(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create stats table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL UNIQUE,
            batches_completed INTEGER DEFAULT 0,
            total_participants INTEGER DEFAULT 0,
            total_fees_saved INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create config table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create bans table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bans (
            pubkey TEXT PRIMARY KEY,
            banned_at INTEGER NOT NULL,
            expires_at INTEGER,
            offense_count INTEGER NOT NULL,
            reason TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Create indices for better query performance
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_batches_state ON batches(state)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_batches_created_at ON batches(created_at DESC)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_batch_participants_batch_id ON batch_participants(batch_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_stats_date ON stats(date DESC)")
        .execute(pool)
        .await?;

    info!("✅ Database migrations completed");
    Ok(())
}

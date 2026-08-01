//! SQLite projections — rebuildable by design.
//!
//! This database is a materialized view of the substrates (signed Nostr
//! events, on-chain state). It holds **no authoritative state**: it must be
//! droppable and rebuildable at any time (ARCHITECTURE.md §1), and a replay
//! test enforces that from M3 onward. Nothing in here may become the only
//! copy of a fact.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

pub mod quotes;
pub mod rfqs;

/// Embedded migrations from `<workspace root>/migrations/`.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// A row that cannot round-trip back into its domain type. In a pure
    /// projection this means the writer and reader disagree — a bug, never
    /// user input.
    #[error("corrupt projection row: {0}")]
    Corrupt(String),
    /// A uniqueness rule refused the write (e.g. the one-quote-per-RFQ
    /// singleton) — the caller's 409, not a server fault.
    #[error("conflict: {0}")]
    Conflict(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Open (creating if missing) the projection database and bring the schema
/// up to date. `url` is a SQLite URL, e.g. `sqlite://scarced.db` or
/// `sqlite::memory:`.
pub async fn open(url: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new().connect_with(options).await?;
    MIGRATOR.run(&pool).await?;
    tracing::info!(url, "projection store opened, schema current");
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_runs_migrations_on_fresh_db() {
        let pool = open("sqlite::memory:").await.expect("open in-memory db");
        let (value,): (String,) =
            sqlx::query_as("SELECT value FROM store_meta WHERE key = 'purpose'")
                .fetch_one(&pool)
                .await
                .expect("baseline row present");
        assert_eq!(value, "projection");
    }

    #[tokio::test]
    async fn open_is_idempotent() {
        let pool = open("sqlite::memory:").await.unwrap();
        MIGRATOR
            .run(&pool)
            .await
            .expect("re-running migrations is a no-op");
    }
}

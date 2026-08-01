//! Configuration from environment (PLAN.md M0). Secrets stay out of here —
//! the studio keys arrive in M3 and come from the environment via the
//! deployment's secret manager, never from files in the repo.

#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP surface binds, e.g. `127.0.0.1:7380`.
    pub bind: String,
    /// SQLite URL of the projection store, e.g. `sqlite://scarced.db`.
    pub db_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            bind: std::env::var("SCARCED_BIND").unwrap_or_else(|_| "127.0.0.1:7380".into()),
            db_url: std::env::var("SCARCED_DB").unwrap_or_else(|_| "sqlite://scarced.db".into()),
        })
    }
}

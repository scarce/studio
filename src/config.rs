//! Configuration from environment (PLAN.md M0). Secrets stay out of here —
//! the studio keys arrive in M3 and come from the environment via the
//! deployment's secret manager, never from files in the repo.

#[derive(Debug, Clone)]
pub struct Config {
    /// Socket address the HTTP surface binds, e.g. `127.0.0.1:7380`.
    pub bind: String,
    /// SQLite URL of the projection store, e.g. `sqlite://scarced.db`.
    pub db_url: String,
    /// Bearer token for studio-authenticated routes (quote issuance).
    /// Unset or empty disables those routes — fail-closed, never a default
    /// credential.
    pub studio_token: Option<String>,
    /// Quote-expiry sweep cadence, seconds. Reads are fail-closed against
    /// sweep lag either way; the sweep keeps the projection rows honest.
    pub sweep_interval_seconds: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            bind: std::env::var("SCARCED_BIND").unwrap_or_else(|_| "127.0.0.1:7380".into()),
            db_url: std::env::var("SCARCED_DB").unwrap_or_else(|_| "sqlite://scarced.db".into()),
            studio_token: std::env::var("SCARCED_STUDIO_TOKEN")
                .ok()
                .filter(|t| !t.trim().is_empty()),
            sweep_interval_seconds: match std::env::var("SCARCED_SWEEP_SECONDS") {
                Ok(raw) => raw.parse().ok().filter(|s| *s > 0).ok_or_else(|| {
                    anyhow::anyhow!("SCARCED_SWEEP_SECONDS must be a positive integer, got `{raw}`")
                })?,
                Err(_) => 30,
            },
        })
    }
}

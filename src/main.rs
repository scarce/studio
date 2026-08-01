//! `scarced` — the scarce-studio daemon: HTTP surface (studio-api) plus, from
//! M3, the orchestrator loop that drives Buzz and watches the substrates. One
//! binary until it hurts (PLAN.md §6).

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use studio_api::AppState;

mod config;
mod mirror;

#[derive(Parser)]
#[command(version, about = "scarced — the scarce-studio daemon")]
struct Args {
    /// YAML config file; SCARCED_* env vars override its keys
    /// (nested keys join with `__`, e.g. SCARCED_BUZZ__RELAY_URL)
    #[arg(long, value_name = "PATH")]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    studio_buzz::install_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::Config::load(args.config.as_deref())?;
    tracing::info!(bind = %config.bind, db = %config.db, "scarced starting");

    // Registry: fail-closed. A roster or skills file that does not parse is
    // a scarced that does not start (GUIDELINES.md §3.2).
    let registry = studio_registry::load(std::path::Path::new(&config.registry_dir))
        .with_context(|| format!("loading agent registry from {}", config.registry_dir))?;
    tracing::info!(
        agents = %registry.agents.keys().cloned().collect::<Vec<_>>().join(", "),
        skills = registry.skills.skills.len(),
        "agent registry loaded"
    );

    let db = studio_store::open(&config.db)
        .await
        .with_context(|| format!("opening projection store at {}", config.db))?;

    spawn_quote_expiry_sweep(db.clone(), config.sweep_seconds);

    // Public base URL for /project/{id} links; defaults to the bind address
    // for dev, `public_url: https://scarce.sh` in production config.
    let public_url = config
        .public_url
        .clone()
        .unwrap_or_else(|| format!("http://{}", config.bind))
        .trim_end_matches('/')
        .to_string();
    // The page's "open in Buzz" link — the community's https host, derived
    // from the relay URL (wss://host -> https://host).
    let community_web_url = config.buzz.as_ref().map(|b| {
        format!(
            "https://{}",
            b.relay_url
                .trim_start_matches("wss://")
                .trim_start_matches("ws://")
        )
    });

    // Lifecycle mirror: config-gated. Fail-closed at startup — a bad key or
    // channel id refuses to boot rather than silently running ledger-only.
    let lifecycle = match &config.buzz {
        Some(buzz) => {
            let port = studio_buzz::RelayBuzz::new(
                &buzz.relay_url,
                &buzz.private_key,
                buzz.auth_tag.as_deref(),
            )
            .context("building buzz relay port")?;
            let ops_channel = uuid::Uuid::parse_str(&buzz.ops_channel)
                .context("buzz.ops_channel is not a uuid")?;
            tracing::info!(relay = %buzz.relay_url, ops_channel = %ops_channel,
                studio_pubkey = %port.public_key_hex(),
                "buzz lifecycle mirror enabled");
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            mirror::spawn(port, ops_channel, db.clone(), public_url.clone(), rx);
            Some(tx)
        }
        None => {
            tracing::warn!("no buzz section configured — studio runs ledger-only");
            None
        }
    };

    let app = studio_api::router(Arc::new(AppState {
        db,
        studio_token: config.studio_token,
        lifecycle,
        public_url,
        community_web_url,
    }));
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("scarced stopped");
    Ok(())
}

/// QUOTED → LAPSED, on a timer (PLAN.md M2). Reads derive LAPSED past
/// expiry on their own; the sweep stamps the projection rows so the ledger
/// itself carries the transition timestamps.
fn spawn_quote_expiry_sweep(db: sqlx::SqlitePool, interval_seconds: u64) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(interval_seconds));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            match studio_store::quotes::sweep_lapsed(&db, chrono::Utc::now()).await {
                Ok(0) => {}
                Ok(lapsed) => tracing::info!(lapsed, "quote expiry sweep"),
                Err(e) => tracing::error!(error = %e, "quote expiry sweep failed"),
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl-c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

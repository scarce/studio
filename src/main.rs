//! `scarced` — the scarce-studio daemon: HTTP surface (studio-api) plus, from
//! M3, the orchestrator loop that drives Buzz and watches the substrates. One
//! binary until it hurts (PLAN.md §6).

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use studio_api::AppState;

mod config;

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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::Config::load(args.config.as_deref())?;
    tracing::info!(bind = %config.bind, db = %config.db, "scarced starting");
    match &config.buzz {
        Some(buzz) => {
            tracing::info!(relay = %buzz.relay_url, "buzz relay configured (orchestrator consumes it in M3)")
        }
        None => tracing::warn!("no buzz relay configured — studio runs ledger-only"),
    }

    let db = studio_store::open(&config.db)
        .await
        .with_context(|| format!("opening projection store at {}", config.db))?;

    spawn_quote_expiry_sweep(db.clone(), config.sweep_seconds);

    let app = studio_api::router(Arc::new(AppState {
        db,
        studio_token: config.studio_token,
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

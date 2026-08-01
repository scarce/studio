//! `scarced` — the scarce-studio daemon: HTTP surface (studio-api) plus, from
//! M3, the orchestrator loop that drives Buzz and watches the substrates. One
//! binary until it hurts (PLAN.md §6).

use std::sync::Arc;

use anyhow::Context;
use studio_api::AppState;

mod config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = config::Config::from_env()?;
    tracing::info!(bind = %config.bind, db = %config.db_url, "scarced starting");

    let db = studio_store::open(&config.db_url)
        .await
        .with_context(|| format!("opening projection store at {}", config.db_url))?;

    let app = studio_api::router(Arc::new(AppState { db }));
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

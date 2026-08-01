//! HTTP surface of `scarced` — serves projections, never asserts a state it
//! cannot evidence (ARCHITECTURE.md §1). Routes accrete per milestone under
//! `/api/v1`; the surface is self-describing (`GET /api/v1` lists endpoints,
//! `GET /api/v1/schemas/{name}` serves the generated JSON Schemas). Handlers
//! stay thin: parse, call `studio-core`, serialize — the logic they invoke is
//! reusable from a CLI or MCP surface without HTTP.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use sqlx::SqlitePool;

pub mod endpoints;

/// Shared state for all handlers. The pool is the projection store —
/// rebuildable, never authoritative.
pub struct AppState {
    pub db: SqlitePool,
    /// Bearer token for studio-authenticated routes (quote issuance).
    /// `None` disables those routes — fail-closed, never fail-open.
    pub studio_token: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    // `/healthz` stays unversioned (ops convention); everything else is
    // `/api/v1` so the contract can evolve without breaking callers.
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1", get(endpoints::api_index::handler))
        .route(
            "/api/v1/schemas/{name}",
            get(endpoints::get_schema::handler),
        )
        .route(
            "/api/v1/rfqs",
            post(endpoints::create_rfq::handler).get(endpoints::list_rfqs::handler),
        )
        .route("/api/v1/rfqs/{id}", get(endpoints::get_rfq::handler))
        .route(
            "/api/v1/rfqs/{id}/quote",
            post(endpoints::create_quote::handler).get(endpoints::get_quote::handler),
        )
        .with_state(state)
}

/// Liveness + readiness: 200 only when the projection store answers.
/// Fail-closed like everything else in this system — a healthz that lies
/// about a dead store would mask exactly the failures we care about.
async fn healthz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
            })),
        ),
        Err(e) => {
            tracing::error!(error = %e, "healthz: projection store unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "status": "degraded", "store": "unreachable" })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_200_with_live_store() {
        let db = studio_store::open("sqlite::memory:").await.unwrap();
        let app = router(Arc::new(AppState {
            db,
            studio_token: None,
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn healthz_returns_503_when_store_is_gone() {
        let db = studio_store::open("sqlite::memory:").await.unwrap();
        db.close().await;
        let app = router(Arc::new(AppState {
            db,
            studio_token: None,
        }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

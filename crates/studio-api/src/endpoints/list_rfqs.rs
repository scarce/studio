//! `GET /rfqs?since=<rfc3339>` — the order book, oldest first.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
pub struct ListParams {
    /// RFC 3339 timestamp; only RFQs captured at or after it are returned.
    pub since: Option<String>,
}

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    let since: Option<DateTime<Utc>> = match params.since.as_deref() {
        None => None,
        Some(raw) => match DateTime::parse_from_rfc3339(raw) {
            Ok(ts) => Some(ts.with_timezone(&Utc)),
            Err(e) => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({
                        "errors": [{ "field": "since", "message": format!("must be RFC 3339: {e}") }]
                    })),
                )
            }
        },
    };

    match studio_store::rfqs::list_since(&state.db, since).await {
        Ok(rfqs) => (StatusCode::OK, Json(serde_json::json!({ "rfqs": rfqs }))),
        Err(e) => {
            tracing::error!(error = %e, "rfq list failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            )
        }
    }
}

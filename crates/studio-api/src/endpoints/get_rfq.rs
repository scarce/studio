//! `GET /rfqs/{id}` — free read of a captured demand record.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::AppState;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match studio_store::rfqs::get(&state.db, &id).await {
        Ok(Some(rfq)) => (StatusCode::OK, Json(serde_json::json!(rfq))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "rfq not found" })),
        ),
        Err(e) => {
            tracing::error!(error = %e, rfq_id = %id, "rfq read failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            )
        }
    }
}

//! `GET /api/v1/rfqs/{id}/quote` — the buyer's free read (ARCHITECTURE.md §4:
//! status reads never taxed). The served status is fail-closed against
//! sweep lag: a quote past its expiry reads LAPSED even before the sweep
//! stamps the row.

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
    Path(rfq_id): Path<String>,
) -> impl IntoResponse {
    match studio_store::quotes::get_by_rfq(&state.db, &rfq_id).await {
        Ok(Some(quote)) => (
            StatusCode::OK,
            Json(serde_json::json!(quote.at(chrono::Utc::now()))),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no quote for this rfq" })),
        ),
        Err(e) => {
            tracing::error!(error = %e, rfq_id = %rfq_id, "quote read failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            )
        }
    }
}

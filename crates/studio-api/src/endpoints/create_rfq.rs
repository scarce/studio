//! `POST /api/v1/rfqs` — demand capture. Free, unsigned, frictionless: the
//! miss record is the studio's order book; never tax it (ARCHITECTURE.md §4).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use studio_types::NewRfq;

use crate::AppState;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Deserialize by hand so shape errors come back as 422 field errors,
    // matching the validation contract, instead of axum's opaque rejection.
    let new_rfq: NewRfq = match serde_json::from_value(body) {
        Ok(rfq) => rfq,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "errors": [{ "field": "body", "message": e.to_string() }]
                })),
            )
        }
    };

    // The handler only supplies identity and time; validation and assembly
    // are the core's single capture path, shared with any future CLI/MCP.
    let rfq = match studio_core::rfq::capture(
        new_rfq,
        uuid::Uuid::new_v4().to_string(),
        chrono::Utc::now(),
    ) {
        Ok(rfq) => rfq,
        Err(errors) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "errors": errors })),
            )
        }
    };

    match studio_store::rfqs::insert(&state.db, &rfq).await {
        Ok(()) => {
            tracing::info!(rfq_id = %rfq.id, buyer = %rfq.buyer_npub, "rfq captured");
            (StatusCode::CREATED, Json(serde_json::json!(rfq)))
        }
        Err(e) => {
            tracing::error!(error = %e, "rfq insert failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "errors": [{ "field": "server", "message": "storage failure" }] }),
                ),
            )
        }
    }
}

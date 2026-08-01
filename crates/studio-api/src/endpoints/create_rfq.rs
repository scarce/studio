//! `POST /rfqs` — demand capture. Free, unsigned, frictionless: the miss
//! record is the studio's order book; never tax it (ARCHITECTURE.md §4).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use studio_core::{NewRfq, Rfq};

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

    if let Err(errors) = new_rfq.validate() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "errors": errors })),
        );
    }

    let rfq = Rfq {
        id: uuid::Uuid::new_v4().to_string(),
        query: new_rfq.query,
        product: new_rfq.product,
        monetization: new_rfq.monetization,
        competition: new_rfq.competition,
        budget_ceiling: new_rfq.budget_ceiling,
        buyer_npub: new_rfq.buyer_npub,
        created_at: chrono::Utc::now(),
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

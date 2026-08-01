//! `POST /api/v1/rfqs/{id}/quote/accept` — buyer acceptance. Free like every
//! buyer surface (buyers never authenticate — ARCHITECTURE.md §2.1); the
//! buyer-signed upgrade rides the same reserved-signature path as the RFQ.
//! ACCEPTED stands in for FUNDED while payments are stubbed (PLAN.md §6
//! override path): accepting a live quote starts the contract.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use studio_core::quote::AcceptError;

use crate::{AppState, LifecycleBeat};

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(rfq_id): Path<String>,
) -> impl IntoResponse {
    let quote = match studio_store::quotes::get_by_rfq(&state.db, &rfq_id).await {
        Ok(Some(quote)) => quote,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "no quote exists for this rfq" })),
            )
        }
        Err(e) => {
            tracing::error!(error = %e, rfq_id = %rfq_id, "quote lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            );
        }
    };

    // Pure decision first (shared with any CLI/MCP), atomic guard second —
    // the UPDATE's WHERE clause re-checks the same rule, so a race loses
    // instead of double-accepting.
    let now = chrono::Utc::now();
    if let Err(refusal) = studio_core::quote::accept(&quote, now) {
        return refuse(refusal, &quote);
    }
    match studio_store::quotes::mark_accepted(&state.db, &rfq_id, now).await {
        Ok(1) => {}
        Ok(_) => {
            // Lost the race between read and update; re-derive the refusal.
            let refusal = studio_core::quote::accept(&quote.clone().at(now), now)
                .err()
                .unwrap_or(AcceptError::AlreadyAccepted);
            return refuse(refusal, &quote);
        }
        Err(e) => {
            tracing::error!(error = %e, rfq_id = %rfq_id, "accept update failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            );
        }
    }

    let accepted = match studio_store::quotes::get_by_rfq(&state.db, &rfq_id).await {
        Ok(Some(quote)) => quote,
        other => {
            tracing::error!(?other, rfq_id = %rfq_id, "accepted quote re-read failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            );
        }
    };
    tracing::info!(quote_id = %accepted.id, rfq_id = %rfq_id, "quote accepted — contract starting");

    match studio_store::rfqs::get(&state.db, &rfq_id).await {
        Ok(Some(rfq)) => state.emit(LifecycleBeat::QuoteAccepted {
            rfq: Box::new(rfq),
            quote: Box::new(accepted.clone()),
        }),
        other => {
            tracing::error!(?other, rfq_id = %rfq_id, "rfq re-read failed; accept beat not mirrored")
        }
    }

    (StatusCode::OK, Json(serde_json::json!(accepted)))
}

fn refuse(
    refusal: AcceptError,
    quote: &studio_types::Quote,
) -> (StatusCode, Json<serde_json::Value>) {
    let message = match refusal {
        AcceptError::AlreadyAccepted => "quote already accepted".to_string(),
        AcceptError::Lapsed => format!(
            "quote lapsed at {} and can no longer be accepted",
            quote.expires_at.to_rfc3339()
        ),
    };
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({ "error": message })),
    )
}

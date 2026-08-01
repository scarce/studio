//! `POST /api/v1/rfqs/{id}/quote` — studio-authenticated quote issuance (PLAN.md
//! M2). The quote is authored by a human/agent for now; the bearer token is
//! the studio's own door, not a buyer surface (buyers never authenticate —
//! ARCHITECTURE.md §2.1). Fail-closed: with no token configured the route is
//! disabled, never open.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use studio_store::StoreError;
use studio_types::NewQuote;

use crate::AppState;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(rfq_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let Some(expected) = state.studio_token.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "quote issuance disabled: SCARCED_STUDIO_TOKEN is not configured"
            })),
        );
    };
    if !bearer_matches(&headers, expected) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "studio bearer token required" })),
        );
    }

    match studio_store::rfqs::get(&state.db, &rfq_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "rfq not found" })),
            )
        }
        Err(e) => {
            tracing::error!(error = %e, rfq_id = %rfq_id, "rfq lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            );
        }
    }

    // Deserialize by hand so shape errors come back as 422 field errors,
    // matching the validation contract, instead of axum's opaque rejection.
    let new_quote: NewQuote = match serde_json::from_value(body) {
        Ok(quote) => quote,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "errors": [{ "field": "body", "message": e.to_string() }]
                })),
            )
        }
    };

    // The handler only supplies identity and time; validation, the policy
    // commitment hash, and assembly are the core's single issuance path,
    // shared with any future CLI/MCP.
    let quote = match studio_core::quote::issue(
        new_quote,
        rfq_id,
        uuid::Uuid::new_v4().to_string(),
        chrono::Utc::now(),
    ) {
        Ok(quote) => quote,
        Err(errors) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "errors": errors })),
            )
        }
    };

    match studio_store::quotes::insert(&state.db, &quote).await {
        Ok(()) => {
            tracing::info!(
                quote_id = %quote.id, rfq_id = %quote.rfq_id,
                policy_hash = %quote.policy_hash, "quote issued"
            );
            state.emit(crate::LifecycleBeat::QuoteIssued {
                quote: Box::new(quote.clone()),
            });
            (StatusCode::CREATED, Json(serde_json::json!(quote)))
        }
        Err(StoreError::Conflict(message)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": message })),
        ),
        Err(e) => {
            tracing::error!(error = %e, "quote insert failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "storage failure" })),
            )
        }
    }
}

/// Constant-time bearer comparison (modulo length, which the token's own
/// randomness makes uninformative).
fn bearer_matches(headers: &HeaderMap, expected: &str) -> bool {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(presented) = presented else {
        return false;
    };
    let (a, b) = (presented.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

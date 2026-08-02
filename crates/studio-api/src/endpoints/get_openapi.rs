//! `GET /openapi.json` — serves the assembled OpenAPI 3.1 document (see
//! `crate::openapi`). Root-mounted by convention: this is the well-known
//! discovery surface a payment gateway (or any client) probes first.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};

use crate::AppState;

pub async fn handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(crate::openapi::document(&state.public_url)),
    )
}

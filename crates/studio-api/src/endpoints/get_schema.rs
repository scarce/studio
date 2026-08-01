//! `GET /api/v1/schemas/{name}` — serves the generated JSON Schemas, so the
//! wire contract is discoverable from the running service itself (same values
//! as the checked-in `schemas/*.json`).

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};

pub async fn handler(Path(name): Path<String>) -> impl IntoResponse {
    match studio_types::schemas::get(&name) {
        Some(schema) => (StatusCode::OK, Json(schema)),
        None => {
            let available: Vec<&str> = studio_types::schemas::all()
                .into_iter()
                .map(|(n, _)| n)
                .collect();
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("unknown schema {name:?}"),
                    "available": available,
                })),
            )
        }
    }
}

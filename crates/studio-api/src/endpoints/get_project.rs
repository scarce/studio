//! `GET /api/v1/projects/{id}` — the public project view (schema:
//! `project`). Free read, deliberately commercial-free: this is what the
//! embedded `/project/{id}` page renders, and its URL is handed to buyers
//! who may share it onward. Assembly is `studio_core::project::view`.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use studio_types::ProjectLinks;

use crate::AppState;

pub async fn handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let rfq = match studio_store::rfqs::get(&state.db, &id).await {
        Ok(Some(rfq)) => rfq,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "project not found" })),
            )
        }
        Err(e) => {
            tracing::error!(error = %e, project_id = %id, "project rfq read failed");
            return storage_failure();
        }
    };
    let quote = match studio_store::quotes::get_by_rfq(&state.db, &id).await {
        Ok(quote) => quote,
        Err(e) => {
            tracing::error!(error = %e, project_id = %id, "project quote read failed");
            return storage_failure();
        }
    };
    let workroom = match studio_store::workrooms::get_by_rfq(&state.db, &id).await {
        Ok(workroom) => workroom,
        Err(e) => {
            tracing::error!(error = %e, project_id = %id, "project workroom read failed");
            return storage_failure();
        }
    };

    let links = ProjectLinks {
        community_web: state.community_web_url.clone(),
        buzz_desktop: crate::BUZZ_DESKTOP_URL.to_string(),
    };
    let project = studio_core::project::view(
        &rfq,
        quote.as_ref(),
        workroom
            .as_ref()
            .map(|w| (w.channel_id.as_str(), w.created_at)),
        links,
        chrono::Utc::now(),
    );
    (StatusCode::OK, Json(serde_json::json!(project)))
}

fn storage_failure() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "storage failure" })),
    )
}

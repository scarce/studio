//! `GET /api/v1` — the discovery index. A client (human, CLI, or agent) that
//! knows only the base URL can enumerate every endpoint and fetch the JSON
//! Schema of every wire type from here.

use axum::{http::StatusCode, response::IntoResponse, Json};

pub async fn handler() -> impl IntoResponse {
    let schemas: Vec<serde_json::Value> = studio_types::schemas::all()
        .into_iter()
        .map(|(name, _)| serde_json::json!({ "name": name, "href": format!("/api/v1/schemas/{name}") }))
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "service": "scarce-studio",
            "version": env!("CARGO_PKG_VERSION"),
            "endpoints": [
                { "method": "GET",  "path": "/openapi.json",          "description": "OpenAPI 3.1 description of this surface" },
                { "method": "GET",  "path": "/api/v1",                "description": "this index" },
                { "method": "GET",  "path": "/api/v1/schemas/{name}", "description": "JSON Schema of a wire type" },
                { "method": "POST", "path": "/api/v1/rfqs",           "description": "capture a demand record (schema: rfq)" },
                { "method": "GET",  "path": "/api/v1/rfqs",           "description": "list captured RFQs, oldest first (?since=<rfc3339>)" },
                { "method": "GET",  "path": "/api/v1/rfqs/{id}",      "description": "fetch one captured RFQ" },
                { "method": "POST", "path": "/api/v1/rfqs/{id}/quote", "description": "issue the quote for an RFQ (studio bearer token; schema: quote)" },
                { "method": "GET",  "path": "/api/v1/rfqs/{id}/quote", "description": "fetch the quote for an RFQ (status fail-closed against expiry)" },
                { "method": "POST", "path": "/api/v1/rfqs/{id}/quote/accept", "description": "accept a live quote (buyer, free; once) — starts the contract, returns project_url" },
                { "method": "GET",  "path": "/api/v1/projects/{id}",   "description": "public project view — no commercial fields (schema: project)" },
                { "method": "GET",  "path": "/project/{id}",           "description": "the project page (embedded web app rendering the public view)" },
            ],
            "schemas": schemas,
            "errors": "validation failures return 422 with { errors: [{ field, message }] }",
        })),
    )
}

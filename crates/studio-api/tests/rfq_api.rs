//! M1 contract tests: the curl round-trip from PLAN.md, as code.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use studio_api::{router, AppState};
use tower::ServiceExt;

const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";

async fn app() -> axum::Router {
    let db = studio_store::open("sqlite::memory:").await.unwrap();
    router(Arc::new(AppState {
        db,
        studio_token: None,
    }))
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let request = match body {
        Some(json) => Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn post_get_round_trip() {
    let app = app().await;

    let (status, created) = send(
        &app,
        "POST",
        "/api/v1/rfqs",
        Some(serde_json::json!({
            "query": "solana priority fee forecast api",
            "product": "p50/p90 forecast per program id",
            "competition": ["helius fee api"],
            "budget_ceiling": { "amount": 250_000_000, "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" },
            "buyer_npub": GOOD_NPUB
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("id assigned");
    assert!(created["created_at"].is_string(), "created_at assigned");

    let (status, fetched) = send(&app, "GET", &format!("/api/v1/rfqs/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, created, "GET returns exactly what POST created");
}

#[tokio::test]
async fn invalid_rfq_gets_422_with_field_errors() {
    let app = app().await;

    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/rfqs",
        Some(serde_json::json!({ "query": "  ", "buyer_npub": "not-an-npub" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields: Vec<&str> = body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["query", "buyer_npub"]);
}

#[tokio::test]
async fn unknown_field_gets_422_not_silent_drop() {
    let app = app().await;
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/rfqs",
        Some(serde_json::json!({ "query": "x", "buyer_npub": GOOD_NPUB, "surprise": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["errors"][0]["field"], "body");
}

#[tokio::test]
async fn missing_rfq_is_404() {
    let app = app().await;
    let (status, _) = send(&app, "GET", "/api/v1/rfqs/definitely-not-there", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_supports_since_filter() {
    let app = app().await;
    for query in ["first", "second"] {
        let (status, _) = send(
            &app,
            "POST",
            "/api/v1/rfqs",
            Some(serde_json::json!({ "query": query, "buyer_npub": GOOD_NPUB })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, body) = send(&app, "GET", "/api/v1/rfqs", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rfqs"].as_array().unwrap().len(), 2);

    let (status, body) = send(&app, "GET", "/api/v1/rfqs?since=2099-01-01T00:00:00Z", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rfqs"].as_array().unwrap().len(), 0);

    let (status, body) = send(&app, "GET", "/api/v1/rfqs?since=yesterday-ish", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"][0]["field"], "since");
}

#[tokio::test]
async fn api_index_lists_every_route_and_schema() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/api/v1", None).await;
    assert_eq!(status, StatusCode::OK);

    let paths: Vec<&str> = body["endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    for expected in [
        "/api/v1",
        "/api/v1/schemas/{name}",
        "/api/v1/rfqs",
        "/api/v1/rfqs/{id}",
    ] {
        assert!(paths.contains(&expected), "index missing {expected}");
    }
    assert_eq!(body["schemas"][0]["name"], "rfq");
}

#[tokio::test]
async fn schemas_endpoint_serves_generated_contract() {
    let app = app().await;
    let (status, body) = send(&app, "GET", "/api/v1/schemas/rfq", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, studio_types::schemas::rfq());

    let (status, body) = send(&app, "GET", "/api/v1/schemas/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["available"][0], "rfq", "404 names what exists: {body}");
}

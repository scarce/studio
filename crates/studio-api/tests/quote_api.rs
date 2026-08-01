//! M2 contract tests: quote issuance (studio-authenticated), the buyer's
//! free read, and RFQ→QUOTED→LAPSED observable via the API (PLAN.md M2
//! done-when).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use studio_api::{router, AppState};
use tower::ServiceExt;

const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";
const TOKEN: &str = "test-studio-token";

async fn app_with_token(token: Option<&str>) -> axum::Router {
    let db = studio_store::open("sqlite::memory:").await.unwrap();
    router(Arc::new(AppState {
        db,
        studio_token: token.map(String::from),
        lifecycle: None,
    }))
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(json) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn capture_rfq(app: &axum::Router) -> String {
    let (status, created) = send(
        app,
        "POST",
        "/api/v1/rfqs",
        None,
        Some(serde_json::json!({
            "query": "solana priority fee forecast api",
            "buyer_npub": GOOD_NPUB
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_string()
}

fn quote_body(expires_at: &str) -> serde_json::Value {
    serde_json::json!({
        "price": { "amount": 250_000_000, "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" },
        "milestones": [
            { "title": "Forecast model", "description": "p50/p90 per program id", "amount": 150_000_000 },
            { "title": "Gated endpoint", "description": "pay.sh-gated REST endpoint", "amount": 100_000_000 }
        ],
        "timeline": "2 weeks, weekly demos",
        "payout_destination": { "kind": "splits", "splits": [
            { "recipient": "CrewAgentA111111111111111111111111111111111", "bps": 10000 }
        ]},
        "channel": { "idle_timeout_seconds": 604_800 },
        "expires_at": expires_at
    })
}

const FAR_FUTURE: &str = "2199-01-01T00:00:00Z";

#[tokio::test]
async fn quote_round_trip_with_defaults_applied() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;

    let (status, created) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["status"], "QUOTED");
    assert_eq!(created["rfq_id"], serde_json::json!(rfq_id));
    assert_eq!(created["channel"]["grace_seconds"], 172_800);
    assert_eq!(
        created["policy_hash"].as_str().map(str::len),
        Some(64),
        "policy hash recorded at issue"
    );
    // the defaulted studio policy rides in the quote, hash-committed
    assert!(created["gate_policy"]["edges"]["QUOTED->FUNDED"].is_array());
    assert!(created["created_at"].is_string());
    assert_eq!(created["lapsed_at"], serde_json::Value::Null);

    // buyer read is free (no bearer) and identical
    let (status, fetched) = send(
        &app,
        "GET",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched, created);
}

#[tokio::test]
async fn expired_quote_reads_lapsed_with_timestamps() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;

    // expires_at must be in the future at issue; one second is enough to
    // land in the past by read time without slowing the suite.
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
    let (status, created) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(quote_body(&expires_at)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["status"], "QUOTED");

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // RFQ→QUOTED→LAPSED observable with timestamps: no sweep ran in this
    // test, so this also proves the read side is fail-closed on its own.
    let (status, fetched) = send(
        &app,
        "GET",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["status"], "LAPSED");
    assert_eq!(fetched["lapsed_at"], fetched["expires_at"]);
    assert_eq!(fetched["created_at"], created["created_at"]);
}

#[tokio::test]
async fn second_quote_conflicts() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;
    let uri = format!("/api/v1/rfqs/{rfq_id}/quote");

    let (status, _) = send(
        &app,
        "POST",
        &uri,
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = send(
        &app,
        "POST",
        &uri,
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn quoting_a_missing_rfq_is_404() {
    let app = app_with_token(Some(TOKEN)).await;
    let (status, _) = send(
        &app,
        "POST",
        "/api/v1/rfqs/definitely-not-there/quote",
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_quote_gets_422_with_field_errors() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;

    let mut body = quote_body(FAR_FUTURE);
    body["milestones"][1]["amount"] = serde_json::json!(1); // sum ≠ price
    let (status, response) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response["errors"][0]["field"], "milestones");
}

#[tokio::test]
async fn issuance_requires_the_right_bearer_token() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;
    let uri = format!("/api/v1/rfqs/{rfq_id}/quote");

    for bad in [None, Some("wrong-token")] {
        let (status, _) = send(&app, "POST", &uri, bad, Some(quote_body(FAR_FUTURE))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "bearer: {bad:?}");
    }
}

#[tokio::test]
async fn issuance_is_disabled_without_a_configured_token() {
    // Fail-closed: no token in config can never mean "no auth required".
    let app = app_with_token(None).await;
    let rfq_id = capture_rfq(&app).await;
    let (status, body) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some("anything"),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}

#[tokio::test]
async fn missing_quote_is_404_and_reads_stay_free() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn accept_flow_once_free_and_conflict_after() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // buyer accepts — free, no bearer
    let accept_uri = format!("/api/v1/rfqs/{rfq_id}/quote/accept");
    let (status, accepted) = send(&app, "POST", &accept_uri, None, None).await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["status"], "ACCEPTED");
    assert!(accepted["accepted_at"].is_string());

    // second accept conflicts, with the reason named
    let (status, body) = send(&app, "POST", &accept_uri, None, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("already accepted"),
        "{body}"
    );

    // the read reflects acceptance and never lapses it
    let (status, read) = send(
        &app,
        "GET",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["status"], "ACCEPTED");
}

#[tokio::test]
async fn accepting_a_missing_or_lapsed_quote_refuses() {
    let app = app_with_token(Some(TOKEN)).await;
    let rfq_id = capture_rfq(&app).await;

    // no quote yet
    let accept_uri = format!("/api/v1/rfqs/{rfq_id}/quote/accept");
    let (status, _) = send(&app, "POST", &accept_uri, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // issue a quote that expires almost immediately, then let it pass
    let expires = (chrono::Utc::now() + chrono::Duration::milliseconds(50)).to_rfc3339();
    let (status, _) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(quote_body(&expires)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let (status, body) = send(&app, "POST", &accept_uri, None, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["error"].as_str().unwrap().contains("lapsed"), "{body}");
}

#[tokio::test]
async fn lifecycle_beats_are_emitted_in_order() {
    let db = studio_store::open("sqlite::memory:").await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let app = router(Arc::new(AppState {
        db,
        studio_token: Some(TOKEN.into()),
        lifecycle: Some(tx),
    }));

    let rfq_id = capture_rfq(&app).await;
    send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(quote_body(FAR_FUTURE)),
    )
    .await;
    send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote/accept"),
        None,
        None,
    )
    .await;

    use studio_api::LifecycleBeat;
    assert!(matches!(
        rx.try_recv().unwrap(),
        LifecycleBeat::DemandCaptured { rfq } if rfq.id == rfq_id
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        LifecycleBeat::QuoteIssued { quote } if quote.rfq_id == rfq_id
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        LifecycleBeat::QuoteAccepted { rfq, quote }
            if rfq.id == rfq_id && quote.accepted_at.is_some()
    ));
    assert!(rx.try_recv().is_err(), "no extra beats");
}

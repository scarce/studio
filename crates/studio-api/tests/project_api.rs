//! The public project surface: `GET /api/v1/projects/{id}` (commercial-free
//! JSON) and the embedded page + assets that render it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use studio_api::{router, AppState};
use tower::ServiceExt;

const GOOD_NPUB: &str = "npub1cscv4empnwmfyurd6utlwmq3h3dzpesjyhtttt6rk69hndk9w0nqr65xpy";
const TOKEN: &str = "test-studio-token";

async fn app() -> (axum::Router, sqlx::SqlitePool) {
    let db = studio_store::open("sqlite::memory:").await.unwrap();
    let invite_url = std::sync::Arc::new(std::sync::RwLock::new(Some(
        "https://scarce.communities.buzz.xyz/invite/v2.test".to_string(),
    )));
    let app = router(Arc::new(AppState {
        db: db.clone(),
        studio_token: Some(TOKEN.into()),
        lifecycle: None,
        public_url: "https://scarce.sh".into(),
        community_web_url: Some("https://scarce.communities.buzz.xyz".into()),
        invite_url,
    }));
    (app, db)
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

/// Drive the whole ledger flow through the API: capture → quote → accept.
async fn contract(app: &axum::Router) -> String {
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
    let rfq_id = created["id"].as_str().unwrap().to_string();

    let (status, quote) = send(
        app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote"),
        Some(TOKEN),
        Some(serde_json::json!({
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
            "engagement_endpoint": "https://scarce.sh/api/v1/engagements/rfq-1",
            "expires_at": "2099-01-01T00:00:00Z"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{quote}");
    rfq_id
}

#[tokio::test]
async fn accept_returns_the_shareable_project_url() {
    let (app, _db) = app().await;
    let rfq_id = contract(&app).await;

    let (status, accepted) = send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote/accept"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{accepted}");
    assert_eq!(accepted["status"], "ACCEPTED");
    assert_eq!(
        accepted["project_url"],
        format!("https://scarce.sh/project/{rfq_id}")
    );
}

#[tokio::test]
async fn project_view_walks_the_state_ladder_and_leaks_no_money() {
    let (app, db) = app().await;
    let rfq_id = contract(&app).await;

    // Quote issued, not yet accepted.
    let (status, project) = send(
        &app,
        "GET",
        &format!("/api/v1/projects/{rfq_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["state"], "QUOTED");
    assert_eq!(project["title"], "solana priority fee forecast api");
    assert_eq!(project["quote"]["milestones"][0]["title"], "Forecast model");
    assert_eq!(
        project["links"]["community_web"],
        "https://scarce.communities.buzz.xyz"
    );
    // The invite CTA links the relay's own onboarding landing page.
    assert_eq!(
        project["links"]["invite"],
        "https://scarce.communities.buzz.xyz/invite/v2.test"
    );

    // Accepted, workroom not yet provisioned (the mirror is async).
    send(
        &app,
        "POST",
        &format!("/api/v1/rfqs/{rfq_id}/quote/accept"),
        None,
        None,
    )
    .await;
    let (_, project) = send(
        &app,
        "GET",
        &format!("/api/v1/projects/{rfq_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(project["state"], "FUNDED");

    // Workroom row lands (what the mirror records) → WORKROOM_ACTIVE.
    studio_store::workrooms::record(
        &db,
        &studio_store::workrooms::Workroom {
            rfq_id: rfq_id.clone(),
            channel_id: "0b5b7a86-6a45-4f7f-9207-3e069b7f0b0e".into(),
            create_event_id: "57fc8b6149f1c5d3ba5f3e801fc2219f92159311062c5876a4403d24ff98c431"
                .into(),
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .unwrap();
    let (_, project) = send(
        &app,
        "GET",
        &format!("/api/v1/projects/{rfq_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(project["state"], "WORKROOM_ACTIVE");
    assert!(project["workroom"]["name"]
        .as_str()
        .unwrap()
        .starts_with("proj-solana-priority"));

    // The public JSON must carry no commercial detail from the quote or rfq.
    let json = project.to_string();
    for leak in [
        "price",
        "amount",
        "250000000",
        "payout",
        "splits",
        "bps",
        "npub",
        "policy",
    ] {
        assert!(
            !json.contains(leak),
            "public project JSON leaks `{leak}`: {json}"
        );
    }

    let (status, body) = send(&app, "GET", "/api/v1/projects/nope", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn page_and_assets_are_embedded() {
    let (app, _db) = app().await;

    for (uri, content_type, marker) in [
        ("/project/anything", "text/html", "scarce"),
        // `:root` marks the design-token block without pinning any one palette
        ("/assets/style.css", "text/css", ":root"),
        // mime db calls it text/ or application/javascript depending on rev
        ("/assets/app.js", "javascript", "STATE_COPY"),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.contains(content_type), "{uri}: {ct}");
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            String::from_utf8_lossy(&body).contains(marker),
            "{uri} missing `{marker}`"
        );
    }

    // The logo ships in the binary too.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/logo.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/assets/nope.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

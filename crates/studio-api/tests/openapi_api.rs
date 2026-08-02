//! `/openapi.json` drift guards. The components cannot drift from the wire
//! types (they are assembled from `studio_types::schemas` at request time);
//! what CAN drift is the path table vs the router and the index — these
//! tests pin both directions we can observe.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use studio_api::{router, AppState};
use tower::ServiceExt;

const PUBLIC_URL: &str = "https://scarce.sh";

async fn app() -> axum::Router {
    let db = studio_store::open("sqlite::memory:").await.unwrap();
    router(Arc::new(AppState {
        db,
        studio_token: None,
        lifecycle: None,
        public_url: PUBLIC_URL.into(),
        community_web_url: Some("https://scarce.communities.buzz.xyz".into()),
        invite_url: Default::default(),
    }))
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, Bytes) {
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
    (status, bytes)
}

async fn document(app: &axum::Router) -> serde_json::Value {
    let (status, bytes) = send(app, "GET", "/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn serves_a_well_formed_document() {
    let app = app().await;
    let doc = document(&app).await;

    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["info"]["title"], "scarce-studio");
    assert_eq!(doc["info"]["version"], env!("CARGO_PKG_VERSION"));
    // Addressable as deployed: servers comes from the configured public_url.
    assert_eq!(doc["servers"][0]["url"], PUBLIC_URL);
}

/// Every operation the document claims must actually be routed. An unmatched
/// path falls through to axum's bare fallback (404, empty body) and a wrong
/// method yields 405 — any documented operation producing either is a lie.
#[tokio::test]
async fn every_documented_operation_is_routed() {
    let app = app().await;
    let doc = document(&app).await;

    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, _) in item.as_object().unwrap() {
            let uri = path
                .replace("{name}", "rfq")
                .replace("{id}", "no-such-id")
                .replace("{file}", "style.css");
            let body = (method.as_str() == "post").then(|| serde_json::json!({}));
            let (status, bytes) = send(&app, &method.to_uppercase(), &uri, body).await;

            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path} is documented but the router rejects the method"
            );
            assert!(
                status != StatusCode::NOT_FOUND || !bytes.is_empty(),
                "{method} {path} is documented but hit the router's bare fallback"
            );
        }
    }
}

/// The other direction we can observe: everything `GET /api/v1` advertises
/// (the hand-maintained index, updated whenever routes accrete) must appear
/// in the document.
#[tokio::test]
async fn documents_every_advertised_endpoint() {
    let app = app().await;
    let doc = document(&app).await;

    let (status, bytes) = send(&app, "GET", "/api/v1", None).await;
    assert_eq!(status, StatusCode::OK);
    let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    for endpoint in index["endpoints"].as_array().unwrap() {
        let path = endpoint["path"].as_str().unwrap();
        let method = endpoint["method"].as_str().unwrap().to_lowercase();
        assert!(
            doc["paths"][path][&method].is_object(),
            "index advertises {method} {path} but /openapi.json does not document it"
        );
    }
}

/// Every `$ref` in the document points at an existing component — the
/// `$defs`-hoisting transform must leave no dangling pointer.
#[tokio::test]
async fn every_ref_resolves() {
    let app = app().await;
    let doc = document(&app).await;
    let components = doc["components"]["schemas"].as_object().unwrap();
    assert!(!components.is_empty());

    let mut refs = Vec::new();
    collect_refs(&doc, &mut refs);
    assert!(!refs.is_empty());
    for reference in refs {
        let target = reference
            .strip_prefix("#/components/schemas/")
            .unwrap_or_else(|| panic!("non-component ref survived hoisting: {reference}"));
        assert!(
            components.contains_key(target),
            "dangling $ref: {reference}"
        );
    }
}

/// The whole published registry is embedded: anything served at
/// `GET /api/v1/schemas/{name}` is also a named component of the document.
#[tokio::test]
async fn components_include_the_published_registry() {
    let app = app().await;
    let doc = document(&app).await;
    let components = doc["components"]["schemas"].as_object().unwrap();

    for (name, _) in studio_types::schemas::all() {
        assert!(
            components.contains_key(name),
            "published schema {name:?} missing from components"
        );
    }
}

fn collect_refs(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, entry) in object {
                if key == "$ref" {
                    if let Some(reference) = entry.as_str() {
                        out.push(reference.to_string());
                        continue;
                    }
                }
                collect_refs(entry, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

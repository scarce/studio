//! The embedded project page — a super-light web app compiled into the
//! binary (same `include_dir` mechanism as pay's web-ui, minus the node
//! toolchain: the assets under `web/` are checked-in vanilla HTML/CSS/JS,
//! no build step). `/project/{id}` serves the shell; the shell fetches
//! `GET /api/v1/projects/{id}` and renders client-side.

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};
use include_dir::{include_dir, Dir};

static WEB: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web");

/// `GET /project/{id}` — the page shell. The id is client-side routing;
/// existence is the API's answer, so unknown ids render the page's own
/// not-found state (a link is shareable before and after its project
/// finishes).
pub async fn project_page() -> impl IntoResponse {
    serve("index.html")
}

/// `GET /assets/{file}` — css/js/logo, embedded at compile time.
pub async fn asset(Path(file): Path<String>) -> impl IntoResponse {
    // include_dir paths never contain `..`; a traversal attempt simply
    // fails the lookup.
    serve(&format!("assets/{file}"))
}

fn serve(path: &str) -> impl IntoResponse {
    match WEB.get_file(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    // Short cache: assets are versionless; five minutes keeps
                    // reloads cheap without wedging a stale page after deploys.
                    (header::CACHE_CONTROL, "public, max-age=300".to_string()),
                ],
                file.contents(),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            [
                (header::CONTENT_TYPE, "text/plain".to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            b"not found".as_slice(),
        ),
    }
}

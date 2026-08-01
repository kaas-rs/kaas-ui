//! The embedded frontend.
//!
//! `rust-embed` pulls `web/dist` in at compile time, which is why the frontend
//! stage of the container build must run *before* the Rust stage. Getting that
//! order backwards produces an image that builds cleanly and serves 404s.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

/// Everything under `web/dist`.
#[derive(Debug, Embed)]
#[folder = "../../web/dist"]
struct Assets;

/// Serve a static asset, falling back to `index.html`.
///
/// The fallback is what makes a client-side router work on a hard refresh:
/// `/clusters/kaas/topics/orders` is not a file, and the browser must still
/// get the application rather than a 404.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(response) = file(path) {
        return response;
    }

    // Anything that looks like a file and is not one is genuinely missing.
    // Falling back to index.html for a `.js` request turns a bad asset URL
    // into a blank page and a console error about a MIME type, which is a
    // much worse thing to debug than a 404.
    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    file("index.html").unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "no frontend was built into this binary",
        )
            .into_response()
    })
}

fn file(path: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();

    // Vite fingerprints everything under /assets/, so those are immutable.
    // index.html never is: it is the thing that names the current fingerprints,
    // and caching it is how a browser ends up asking for a bundle that was
    // deleted two deploys ago.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    Some(
        Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::CACHE_CONTROL, cache_control)
            .body(Body::from(asset.data.into_owned()))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    )
}

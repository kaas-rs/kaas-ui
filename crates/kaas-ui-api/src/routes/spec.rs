//! The OpenAPI document, served.
//!
//! Built once and handed out as bytes. The document is not small — the
//! broker's own config documentation is in the schemas — and rebuilding it per
//! request would be a surprising amount of work for a page someone opens twice
//! a month.

use std::sync::OnceLock;

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// `GET /api/openapi.json`
///
/// A GET, like everything else. The document it returns describes an API with
/// no other verb in it, which the document itself is the easiest way to check.
#[utoipa::path(
    get,
    path = "/api/openapi.json",
    responses((status = 200, description = "The OpenAPI document", content_type = "application/json")),
    tag = "meta",
)]
pub async fn spec() -> Response {
    static DOCUMENT: OnceLock<Result<String, String>> = OnceLock::new();

    match DOCUMENT.get_or_init(|| crate::openapi::spec_json().map_err(|error| error.to_string())) {
        Ok(json) => ([(header::CONTENT_TYPE, "application/json")], json.clone()).into_response(),
        // Unreachable in practice — a unit test builds the same document —
        // but a panic here would take down a server hosting a dozen clusters
        // because someone opened the docs page.
        Err(error) => {
            tracing::error!(%error, "the OpenAPI document could not be built");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "the OpenAPI document could not be built",
            )
                .into_response()
        }
    }
}

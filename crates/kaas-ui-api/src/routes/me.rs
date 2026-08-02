//! Who the caller is, as the caller sees themselves.

use axum::Json;

use crate::AppState;
use crate::Caller;
use axum::extract::State;
use kaas_ui_core::dto::Identity;

/// `GET /api/me`
///
/// The identity behind this request and the roles it resolved to.
///
/// Deliberately not the place to ask *what may I do here* — that is per
/// cluster, and it rides on each cluster's card as `grants`. Splitting them
/// would give the frontend two sources for one answer, and the one it did not
/// re-fetch would be the stale one.
///
/// Answers for anonymous callers too. A deployment with no identity provider
/// configured has exactly one caller, and saying so plainly is what lets the
/// frontend render "not signed in" rather than an error.
#[utoipa::path(
    get,
    path = "/api/me",
    responses((status = 200, description = "The caller", body = Identity)),
    tag = "auth",
)]
pub async fn me(State(state): State<AppState>, caller: Caller) -> Json<Identity> {
    Json(Identity::of(
        caller.principal(),
        caller.access(),
        state.policy().is_enforcing(),
    ))
}

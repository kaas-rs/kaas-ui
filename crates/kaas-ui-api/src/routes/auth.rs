//! Login, callback, logout.
//!
//! The three routes that turn a browser into a [`Caller`](crate::Caller). They
//! exist only when an identity provider is configured; a deployment without
//! one — every development instance, and this cluster until today — has no
//! `/auth` routes at all, and the frontend has nothing to offer.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::PrivateCookieJar;
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiError;
use crate::session;

/// What the provider sends back.
#[derive(Debug, Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    /// Present instead of `code` when the provider refused — the user pressed
    /// cancel, or the app is not approved for an organisation.
    error: Option<String>,
    error_description: Option<String>,
}

/// `GET /auth/login`
///
/// Start a login: generate the PKCE challenge, `state` and `nonce`, remember
/// them in an encrypted cookie, and send the browser to the provider.
///
/// A `GET` because it is a link somebody clicks, and it changes nothing on
/// this side that a second click would not simply replace.
pub async fn login(State(state): State<AppState>, jar: PrivateCookieJar) -> Response {
    let Some(provider) = state.auth() else {
        return no_provider();
    };

    match provider.start_login() {
        Ok((url, pending)) => (session::stash(jar, &pending), Redirect::to(&url)).into_response(),
        Err(error) => {
            tracing::error!(%error, "could not start a login");
            ApiError::bad_gateway_login(&error.to_string()).into_response()
        }
    }
}

/// `GET /auth/callback`
///
/// Finish a login. The provider redirects a browser here with a `code` and the
/// `state` it was given; everything is verified against the pending cookie
/// before a session exists.
pub async fn callback(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(query): Query<Callback>,
) -> Response {
    let Some(provider) = state.auth() else {
        return no_provider();
    };

    // Taken first and unconditionally: one pending login, one attempt.
    let (pending, jar) = session::take(jar);

    if let Some(error) = query.error {
        let detail = query.error_description.unwrap_or_else(|| error.clone());
        tracing::warn!(%error, %detail, "the provider refused a login");
        return (
            jar,
            ApiError::forbidden(format!("the login provider refused: {detail}")),
        )
            .into_response();
    }

    let (Some(code), Some(returned_state), Some(pending)) = (query.code, query.state, pending)
    else {
        // Either the browser arrived here without having started a login, or
        // the pending cookie expired underneath it. Both are "start again",
        // and neither is worth an error page with a stack of jargon.
        return (jar, Redirect::to("/")).into_response();
    };

    match provider
        .finish_login(&code, &returned_state, &pending)
        .await
    {
        Ok(principal) => {
            // The roles are resolved once, here, and their names go in the
            // cookie — see `Policy::access_for_roles`.
            let access = state.policy().access(&principal);
            let roles: Vec<String> = access.role_names().map(str::to_owned).collect();

            // **The groups and the roles together, or this line is not worth
            // writing.** A role naming a group that never arrives grants
            // nothing, and the symptom is an empty fleet — indistinguishable
            // from a role that matched and permits nothing. The claim is only
            // in hand here: it is not in the session cookie, so no later
            // request can report it. Seeing `groups=["Platform Team"]` next to
            // `roles=[]` names the mismatch, which on Entra is usually a group
            // renamed out from under a `subjects` entry.
            let groups: Vec<&str> = principal.groups().collect();
            tracing::info!(
                subject = %principal.subject(),
                name = %principal.display_name(),
                ?groups,
                ?roles,
                "signed in"
            );

            let jar = session::issue(
                jar,
                principal.subject().to_owned(),
                principal.display_name().to_owned(),
                roles,
                provider.session_ttl(),
            );
            (jar, Redirect::to("/")).into_response()
        }
        Err(error) => {
            // Deliberately loud in the log and vague in the response: which of
            // state, nonce or signature failed is useful to an operator and a
            // hint to whoever was trying it.
            tracing::warn!(%error, "a login did not verify");
            (jar, ApiError::forbidden("that login could not be verified")).into_response()
        }
    }
}

/// `POST /auth/logout`
///
/// Drop the session. A `POST` so that a link on another site cannot sign
/// somebody out by being loaded — `SameSite=Lax` sends the cookie on a
/// top-level `GET` navigation, which is exactly the case a logout link would
/// be abused through.
pub async fn logout(jar: PrivateCookieJar) -> Response {
    (
        session::clear(jar, session::SESSION_COOKIE),
        Redirect::to("/"),
    )
        .into_response()
}

/// What every route here answers when no provider is configured.
///
/// `404`, not `501`: on a deployment with no identity provider these routes do
/// not conceptually exist, and the frontend never offers them.
fn no_provider() -> Response {
    ApiError::not_found("this deployment has no identity provider configured").into_response()
}

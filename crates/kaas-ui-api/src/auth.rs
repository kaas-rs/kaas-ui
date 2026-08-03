//! Who the request is from, as an extractor.
//!
//! [`Caller`] is the parameter every handler that touches a cluster takes, and
//! the reason it is an extractor rather than something a handler could forget
//! to consult: [`AppState::cluster`](crate::AppState::cluster) will not compile
//! without one.
//!
//! # Where the caller comes from
//!
//! An encrypted session cookie, or nobody. No cookie, an expired one, or one
//! this process cannot decrypt — a restart rotates the key — all read as the
//! anonymous caller, because "signed out" is what all three mean to whoever is
//! looking at the screen.
//!
//! The cookie carries the *role names* a login resolved to rather than the
//! groups claim behind them. See `Policy::access_for_roles` for what that
//! trades away.

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum_extra::extract::PrivateCookieJar;
use axum_extra::extract::cookie::Key;
use kaas_ui_auth::{Access, Grant, Principal};

use crate::AppState;
use crate::error::ApiError;
use crate::session;

/// The caller, and what they may do.
#[derive(Debug, Clone)]
pub struct Caller {
    principal: Principal,
    access: Access,
}

impl Caller {
    /// Build one from a resolved identity.
    ///
    /// Crate-visible on purpose. The extractor below is the only way a handler
    /// gets a caller, and a public constructor would let one be conjured with
    /// [`Access::unrestricted`] inside a route — which is precisely the bug
    /// this whole arrangement exists to make impossible.
    pub(crate) fn new(principal: Principal, access: Access) -> Self {
        Self { principal, access }
    }

    /// Who is asking.
    #[must_use]
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// What they may see and do.
    #[must_use]
    pub fn access(&self) -> &Access {
        &self.access
    }

    /// Require a grant on a cluster, or fail the request.
    ///
    /// Takes the labels rather than the id because that is what a role selects
    /// on, and takes the id only to say which cluster in the message.
    ///
    /// # Errors
    ///
    /// `403` when the caller holds no role granting this on that cluster.
    /// Never `404` — reaching this point means the cluster was already visible
    /// through the registry lookup, so its existence is not a secret from this
    /// caller.
    pub fn require(
        &self,
        cluster: &str,
        labels: &std::collections::BTreeMap<String, String>,
        grant: Grant,
    ) -> Result<(), ApiError> {
        if self.access.may(labels, grant) {
            return Ok(());
        }
        Err(ApiError::forbidden(match grant {
            Grant::Messages => format!(
                "reading message payloads on cluster {cluster} needs the `messages` grant, which \
                 no role of yours holds there"
            ),
            Grant::Metadata => {
                format!("browsing cluster {cluster} needs the `metadata` grant")
            }
        }))
    }

    /// Require payload access to one named topic.
    ///
    /// The grant *and* the role's topic patterns, asked as one question —
    /// see [`Access::may_read_topic`].
    ///
    /// # Errors
    ///
    /// `403` when the caller may not read this topic's payloads.
    pub fn require_topic(
        &self,
        cluster: &str,
        labels: &std::collections::BTreeMap<String, String>,
        topic: &str,
    ) -> Result<(), ApiError> {
        if self.access.may_read_topic(labels, topic) {
            return Ok(());
        }
        Err(ApiError::forbidden(format!(
            "reading payloads from {topic:?} on cluster {cluster} is outside the topics your \
             roles grant `messages` on"
        )))
    }
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = PrivateCookieJar::from_headers(&parts.headers, Key::from_ref(state));

        let Some(found) = session::read(&jar) else {
            // No session is not an error. An open deployment makes this
            // caller unrestricted; an enforcing one makes them nobody, which
            // is the safe direction for the gap to fall.
            let principal = Principal::anonymous();
            let access = state.policy().access(&principal);
            return Ok(Self::new(principal, access));
        };

        let principal = Principal::new(found.subject, Some(found.name), []);
        let access = state.policy().access_for_roles(&found.roles);
        Ok(Self::new(principal, access))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use kaas_ui_auth::{Policy, Role};
    use kaas_ui_core::config::Config;
    use kaas_ui_core::registry::Registry;

    use super::*;

    fn state(policy: Policy) -> AppState {
        let config = Config::from_yaml(
            r"
clusters:
  - id: dev
    bootstrap: ['localhost:9092']
    labels: { env: dev }
",
        )
        .expect("the fixture config parses");
        let registry = Arc::new(ArcSwap::from_pointee(Registry::from_config(&config)));
        AppState::new(registry, policy)
    }

    fn labels() -> BTreeMap<String, String> {
        [("env".to_owned(), "dev".to_owned())].into_iter().collect()
    }

    async fn caller_for(policy: Policy) -> Caller {
        let state = state(policy);
        let (mut parts, ()) = axum::http::Request::builder()
            .uri("/api/clusters")
            .body(())
            .expect("the request builds")
            .into_parts();
        Caller::from_request_parts(&mut parts, &state)
            .await
            .expect("an anonymous caller is always resolvable")
    }

    #[tokio::test]
    async fn an_open_policy_leaves_the_anonymous_caller_unrestricted() {
        let caller = caller_for(Policy::open()).await;

        assert!(!caller.principal().is_authenticated());
        assert!(caller.access().is_unrestricted());
        assert!(caller.require("dev", &labels(), Grant::Messages).is_ok());
        assert!(caller.require_topic("dev", &labels(), "payments").is_ok());
    }

    #[tokio::test]
    async fn an_enforcing_policy_gives_the_anonymous_caller_nothing() {
        // Not a hypothetical: this is what every request looks like the moment
        // roles are configured and before the OIDC slice lands, so it had
        // better fail closed rather than open.
        let policy = Policy::enforcing(vec![Role {
            name: "dev".to_owned(),
            subjects: vec!["kaas-rs".to_owned()],
            grants: [Grant::Metadata, Grant::Messages].into_iter().collect(),
            ..Role::default()
        }]);
        let caller = caller_for(policy).await;

        assert!(!caller.access().is_unrestricted());
        assert!(!caller.access().sees(&labels()));
        assert_eq!(
            caller
                .require("dev", &labels(), Grant::Messages)
                .expect_err("no role covers an anonymous caller")
                .status(),
            axum::http::StatusCode::FORBIDDEN
        );
    }
}

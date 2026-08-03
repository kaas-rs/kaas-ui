//! The exchange: browser to Dex to GitHub and back, ending in a [`Principal`].
//!
//! Three things are mandatory and none of them are optional flags:
//!
//! * **PKCE with `S256`.** Dex accepts a flow with no challenge at all, and
//!   accepts `plain` if asked — both read from its source. kaas-ui is a public
//!   client with no client secret, so the challenge is the *only* thing
//!   proving that whoever redeems the code is whoever started the flow. It is
//!   generated with the request rather than attached to it, so there is no
//!   path through this module that omits it.
//! * **`state`**, compared on return. Without it a third party can hand
//!   somebody a callback URL and log them in as the wrong person.
//! * **`nonce`**, verified inside the `id_token`. Without it a token minted
//!   for another flow can be replayed into this one.
//!
//! The `id_token`'s signature is verified against the provider's JWKS — the
//! whole reason Dex is in front of GitHub, which signs nothing.
//!
//! # What is not here
//!
//! **No refresh tokens.** A session lasts as long as it lasts and then asks
//! for a login again; `offline_access` is never requested. This is a read-only
//! browser tool people keep open for twenty minutes, and a refresh token is a
//! long-lived credential to store, protect and revoke in exchange for saving
//! them one redirect a day. Decided rather than drifted into, which is what
//! `docs/05-phase-4-auth.md` asks for.

use std::time::Duration;

use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreProviderMetadata,
};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, CsrfToken, EmptyAdditionalClaims, IdTokenClaims, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
};
use serde::{Deserialize, Serialize};

use crate::identity::Principal;

/// How long discovery and the token exchange may take.
///
/// Both are a login-time hop to Dex. Long enough for a provider having a bad
/// moment, short enough that a browser is not left on a white page.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// What the config file says about the provider.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct OidcConfig {
    /// The issuer, exactly as it appears in `iss`.
    ///
    /// `https://kaas.smeding.cloud/dex` — the public URL, even though Dex is a
    /// Service one network hop away, because this string is compared against
    /// the token's claim and advertised to the browser.
    pub issuer: String,
    /// The client id registered with the provider. `kaas-ui`.
    pub client_id: String,
    /// Where the provider sends the browser back. Must match the provider's
    /// registered redirect URI character for character.
    pub redirect_url: String,
    /// What to ask for. `groups` is what a role selector matches on.
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    /// How long a session lasts before a login is asked for again.
    #[serde(default = "default_session_ttl", with = "humantime_serde")]
    pub session_ttl: Duration,
}

fn default_scopes() -> Vec<String> {
    ["openid", "profile", "email", "groups"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn default_session_ttl() -> Duration {
    Duration::from_secs(8 * 60 * 60)
}

/// Anything that can go wrong between the button and the session.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    /// The configuration does not describe a provider.
    #[error("{0}")]
    Config(String),
    /// The provider could not be reached or did not describe itself.
    #[error("the login provider could not be reached: {0}")]
    Discovery(String),
    /// The browser came back with something that does not belong to a flow
    /// this process started.
    #[error("{0}")]
    Rejected(String),
    /// The provider answered the exchange with an error.
    #[error("the login provider refused the exchange: {0}")]
    Exchange(String),
}

/// A login in progress.
///
/// Rides in a short-lived encrypted cookie between the redirect and the
/// callback. All three fields are secrets in the sense that leaking them
/// breaks the guarantee they exist for, which is why the cookie is encrypted
/// rather than merely signed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pending {
    /// Compared against the `state` the provider returns.
    pub state: String,
    /// Checked inside the `id_token`.
    pub nonce: String,
    /// The PKCE verifier, whose challenge went out with the redirect.
    pub verifier: String,
}

/// A discovered provider, ready to start and finish logins.
#[derive(Debug, Clone)]
pub struct Provider {
    metadata: CoreProviderMetadata,
    config: OidcConfig,
    http: reqwest::Client,
}

impl Provider {
    /// Read the provider's discovery document.
    ///
    /// Done once at startup so a broken `issuer` is a failure to boot rather
    /// than a failure at somebody's first login, and so the JWKS is in hand
    /// before it is needed.
    ///
    /// # Errors
    ///
    /// If the issuer is not a URL, or the provider cannot be reached, or its
    /// document names a different issuer than the one configured — that last
    /// one being the check that makes the rest of this meaningful.
    pub async fn discover(config: OidcConfig) -> Result<Self, OidcError> {
        let issuer = IssuerUrl::new(config.issuer.clone())
            .map_err(|error| OidcError::Config(format!("auth.issuer is not a URL: {error}")))?;
        RedirectUrl::new(config.redirect_url.clone()).map_err(|error| {
            OidcError::Config(format!("auth.redirect_url is not a URL: {error}"))
        })?;

        let http = reqwest::ClientBuilder::new()
            .timeout(HTTP_TIMEOUT)
            // A redirect during discovery means the issuer is not where the
            // config says it is, and following it silently would verify
            // tokens against whatever answered.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| OidcError::Discovery(error.to_string()))?;

        let metadata = CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|error| OidcError::Discovery(error.to_string()))?;

        tracing::info!(
            issuer = %config.issuer,
            client_id = %config.client_id,
            "login provider discovered"
        );

        Ok(Self {
            metadata,
            config,
            http,
        })
    }

    /// How long a session from this provider lasts.
    #[must_use]
    pub fn session_ttl(&self) -> Duration {
        self.config.session_ttl
    }

    fn client(
        &self,
    ) -> Result<
        CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
        OidcError,
    > {
        let redirect = RedirectUrl::new(self.config.redirect_url.clone())
            .map_err(|error| OidcError::Config(error.to_string()))?;
        Ok(CoreClient::from_provider_metadata(
            self.metadata.clone(),
            ClientId::new(self.config.client_id.clone()),
            // No client secret. This is a public client, and PKCE is what
            // stands in for one — see the module docs.
            None,
        )
        .set_redirect_uri(redirect))
    }

    /// Where to send the browser, and what to remember while it is away.
    ///
    /// # Errors
    ///
    /// Only if the configuration stopped being a URL since startup.
    pub fn start_login(&self) -> Result<(String, Pending), OidcError> {
        // S256, always. `PkceCodeChallenge::new_random_sha256` is the only
        // constructor used here; the `plain` method that Dex would also accept
        // is never spelled anywhere in this workspace.
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

        let client = self.client()?;
        let mut request = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);

        for scope in &self.config.scopes {
            // `openid` is added by the flow itself, so adding it again would
            // put it in the URL twice — legal, and the sort of thing that
            // makes someone reading a redirect wonder what else is wrong.
            if scope != "openid" {
                request = request.add_scope(Scope::new(scope.clone()));
            }
        }

        let (url, state, nonce) = request.url();

        Ok((
            url.to_string(),
            Pending {
                state: state.secret().clone(),
                nonce: nonce.secret().clone(),
                verifier: verifier.into_secret(),
            },
        ))
    }

    /// Turn a callback into a principal.
    ///
    /// # Errors
    ///
    /// [`OidcError::Rejected`] when `state` does not match the pending login,
    /// or the provider returned no `id_token`, or the token's signature,
    /// issuer, audience, expiry or nonce do not check out.
    pub async fn finish_login(
        &self,
        code: &str,
        state: &str,
        pending: &Pending,
    ) -> Result<Principal, OidcError> {
        // Before anything is sent anywhere: this callback must belong to the
        // flow this browser started.
        if state != pending.state {
            return Err(OidcError::Rejected(
                "this login did not start here: the state does not match".to_owned(),
            ));
        }

        let client = self.client()?;
        let tokens = client
            .exchange_code(AuthorizationCode::new(code.to_owned()))
            .map_err(|error| OidcError::Exchange(error.to_string()))?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.verifier.clone()))
            .request_async(&self.http)
            .await
            .map_err(|error| OidcError::Exchange(error.to_string()))?;

        let id_token = tokens.extra_fields().id_token().ok_or_else(|| {
            OidcError::Rejected(
                "the provider returned no id_token, so there is nothing to verify".to_owned(),
            )
        })?;

        let claims = id_token
            .claims(
                &client.id_token_verifier(),
                &Nonce::new(pending.nonce.clone()),
            )
            .map_err(|error| {
                OidcError::Rejected(format!("the id_token did not verify: {error}"))
            })?;

        Ok(principal_of(claims))
    }
}

/// What the claims say, in this workspace's terms.
///
/// The subject is `sub` — Dex's opaque `(connector, user id)` pair, stable and
/// unreadable. A role naming a person will be matching `preferred_username` or
/// `email` instead, which is why [`Principal::identifiers`] covers all three
/// and why this hands them over rather than picking one.
fn principal_of(claims: &IdTokenClaims<EmptyAdditionalClaims, CoreGenderClaim>) -> Principal {
    let name = claims
        .preferred_username()
        .map(|name| name.as_str().to_owned())
        .or_else(|| {
            claims
                .name()
                .and_then(|name| name.get(None))
                .map(|name| name.as_str().to_owned())
        })
        .or_else(|| claims.email().map(|email| email.as_str().to_owned()));

    let mut identities = Vec::new();
    if let Some(username) = claims.preferred_username() {
        identities.push(username.as_str().to_owned());
    }
    if let Some(email) = claims.email() {
        identities.push(email.as_str().to_owned());
    }

    Principal::new(claims.subject().to_string(), name, identities)
}

impl Provider {
    /// Build one from a discovery document that is already in hand.
    ///
    /// For tests: the flow below is worth asserting on without a provider to
    /// talk to, and `discover` is otherwise the only way to get here.
    #[cfg(test)]
    fn from_metadata(metadata: CoreProviderMetadata, config: OidcConfig) -> Self {
        Self {
            metadata,
            config,
            http: reqwest::Client::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_ask_for_groups_and_last_eight_hours() {
        // `groups` is what a role selector matches on, so its absence would
        // make every group-based role silently never apply.
        assert!(default_scopes().contains(&"groups".to_owned()));
        assert_eq!(default_session_ttl(), Duration::from_secs(8 * 60 * 60));
    }

    /// A discovery document shaped like the one Dex serves, minus everything
    /// this module does not read.
    fn recorded_metadata() -> CoreProviderMetadata {
        serde_json::from_str(
            r#"{
                "issuer": "https://kaas.smeding.cloud/dex",
                "authorization_endpoint": "https://kaas.smeding.cloud/dex/auth",
                "token_endpoint": "https://kaas.smeding.cloud/dex/token",
                "jwks_uri": "https://kaas.smeding.cloud/dex/keys",
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"]
            }"#,
        )
        .expect("the fixture parses")
    }

    fn recorded_config() -> OidcConfig {
        OidcConfig {
            issuer: "https://kaas.smeding.cloud/dex".to_owned(),
            client_id: "kaas-ui".to_owned(),
            redirect_url: "https://kaas.smeding.cloud/auth/callback".to_owned(),
            scopes: default_scopes(),
            session_ttl: default_session_ttl(),
        }
    }

    #[test]
    fn every_authorize_url_carries_pkce_state_and_nonce() {
        // The acceptance criterion, and the property the whole public-client
        // decision rests on: Dex will serve a flow with no challenge at all,
        // so nothing but this guarantees one is sent.
        let provider = Provider::from_metadata(recorded_metadata(), recorded_config());
        let (url, pending) = provider.start_login().expect("the fixture is valid");

        let query: std::collections::HashMap<_, _> = url::Url::parse(&url)
            .expect("a URL")
            .query_pairs()
            .into_owned()
            .collect();

        assert_eq!(
            query.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert!(query.contains_key("code_challenge"));
        assert_eq!(query.get("state"), Some(&pending.state));
        assert_eq!(query.get("nonce"), Some(&pending.nonce));
        assert_eq!(
            query.get("redirect_uri").map(String::as_str),
            Some("https://kaas.smeding.cloud/auth/callback")
        );
        // `groups` is what a role selector matches on, and `openid` appears
        // once rather than twice.
        let scope = query.get("scope").cloned().unwrap_or_default();
        assert!(scope.contains("groups"), "{scope}");
        assert_eq!(scope.matches("openid").count(), 1, "{scope}");
    }

    #[test]
    fn two_logins_never_share_a_challenge() {
        let provider = Provider::from_metadata(recorded_metadata(), recorded_config());
        let (_, first) = provider.start_login().expect("valid");
        let (_, second) = provider.start_login().expect("valid");

        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, second.state);
        assert_ne!(first.nonce, second.nonce);
    }

    #[tokio::test]
    async fn an_issuer_that_is_not_a_url_fails_at_startup() {
        let config = OidcConfig {
            issuer: "not a url".to_owned(),
            client_id: "kaas-ui".to_owned(),
            redirect_url: "https://example.test/auth/callback".to_owned(),
            scopes: default_scopes(),
            session_ttl: default_session_ttl(),
        };
        let error = Provider::discover(config)
            .await
            .expect_err("a bare string is not an issuer");
        assert!(matches!(error, OidcError::Config(_)), "{error}");
    }

    #[tokio::test]
    async fn a_redirect_url_that_is_not_a_url_fails_at_startup() {
        // Caught here rather than at the first login, when whoever typed it is
        // long gone and the symptom is a provider-side error page.
        let config = OidcConfig {
            issuer: "https://example.test/dex".to_owned(),
            client_id: "kaas-ui".to_owned(),
            redirect_url: "/auth/callback".to_owned(),
            scopes: default_scopes(),
            session_ttl: default_session_ttl(),
        };
        let error = Provider::discover(config)
            .await
            .expect_err("a relative path is not a redirect URL");
        assert!(matches!(error, OidcError::Config(_)), "{error}");
    }
}

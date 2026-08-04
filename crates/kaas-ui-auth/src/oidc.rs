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

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreJsonWebKeySet, CoreProviderMetadata,
};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClaimsVerificationError, ClientId, CsrfToken, EmptyAdditionalClaims,
    IdTokenClaims, IssuerUrl, JsonWebKeySetUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenUrl,
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
    /// The same provider, addressed from inside the cluster, as in
    /// `http://dex.dex.svc.cluster.local:5556/dex`.
    ///
    /// **Set this whenever the issuer's hostname resolves back to kaas-ui.**
    /// It is what ArgoCD does with `--dex-server`: the browser hops of a login
    /// go to the public issuer, and the calls this process makes on its own —
    /// discovery, the token exchange, the key set — go straight to the Service.
    ///
    /// Absent, every one of those leaves the cluster and comes back through
    /// whatever fronts kaas-ui. Where that front routes the issuer's hostname
    /// *to kaas-ui*, and kaas-ui proxies `/dex`, discovery at startup is then a
    /// request to a server that is not listening yet, and the process cannot
    /// boot at all — it needs a running instance of itself to answer. The
    /// deployment in `Woestebanaan/k3s-cluster` is exactly that shape.
    ///
    /// Only the address changes. The document served here has to name [the
    /// issuer above](Self::issuer), or discovery fails, and `iss` is still
    /// checked against that public string on every login.
    #[serde(default)]
    pub internal_url: Option<String>,
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
///
/// The metadata sits behind an [`ArcSwap`] because one part of it — the key
/// set — **expires while the process runs**. See [`Provider::refresh_keys`].
#[derive(Debug)]
pub struct Provider {
    metadata: ArcSwap<CoreProviderMetadata>,
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
    /// Over [`internal_url`](OidcConfig::internal_url) when it is set, and over
    /// the issuer itself when it is not.
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

        let metadata = match config.internal_url.as_deref() {
            Some(internal) => discover_internally(&http, &config.issuer, internal).await?,
            None => CoreProviderMetadata::discover_async(issuer, &http)
                .await
                .map_err(|error| OidcError::Discovery(error.to_string()))?,
        };

        tracing::info!(
            issuer = %config.issuer,
            over = config.internal_url.as_deref().unwrap_or("the issuer"),
            client_id = %config.client_id,
            "login provider discovered"
        );

        Ok(Self {
            metadata: ArcSwap::from_pointee(metadata),
            config,
            http,
        })
    }

    /// Re-read the provider's signing keys.
    ///
    /// **A provider rotates its keys, and discovery is done once.** Dex mints
    /// a new signing key on a schedule and serves the previous one alongside
    /// it for a while; a client that cached the key set at startup keeps
    /// verifying against a key that is no longer used, and every login fails
    /// with `Signature verification failed` from the moment of the first
    /// rotation until the process restarts. It does not recover on its own,
    /// which is what makes it worth a method rather than a comment.
    ///
    /// Only the key set is re-fetched. The rest of the discovery document
    /// describes endpoints, and an endpoint that moved is a reconfiguration
    /// rather than something to follow silently at login time.
    ///
    /// # Errors
    ///
    /// [`OidcError::Discovery`] if the key set could not be fetched. The
    /// caller keeps the keys it had — a provider having a bad moment must not
    /// leave this with none.
    pub async fn refresh_keys(&self) -> Result<(), OidcError> {
        let current = self.metadata.load();
        let jwks = CoreJsonWebKeySet::fetch_async(current.jwks_uri(), &self.http)
            .await
            .map_err(|error| OidcError::Discovery(error.to_string()))?;

        let refreshed = current.as_ref().clone().set_jwks(jwks);
        self.metadata.store(Arc::new(refreshed));
        tracing::info!(issuer = %self.config.issuer, "re-read the provider's signing keys");
        Ok(())
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
            self.metadata.load().as_ref().clone(),
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

        let nonce = Nonce::new(pending.nonce.clone());

        // The happy path, and the only path on a provider that has not
        // rotated since this process started.
        match id_token.claims(&client.id_token_verifier(), &nonce) {
            Ok(claims) => return Ok(principal_of(claims)),
            Err(ClaimsVerificationError::SignatureVerification(_)) => {}
            Err(error) => {
                return Err(OidcError::Rejected(format!(
                    "the id_token did not verify: {error}"
                )));
            }
        }

        // A signature this process cannot check is the one verification
        // failure that is plausibly **our** fault rather than the caller's:
        // the key that signed it may simply be newer than the set held here.
        // Re-read the keys and give the token exactly one more chance.
        //
        // Only the verification is retried. The code was already spent above
        // and is single-use — exchanging it again answers `invalid_grant` and
        // turns a recoverable login into a failed one.
        tracing::info!("an id_token was signed by an unknown key; re-reading the provider's keys");
        self.refresh_keys().await?;

        let client = self.client()?;
        let claims = id_token
            .claims(&client.id_token_verifier(), &nonce)
            .map_err(|error| {
                OidcError::Rejected(format!("the id_token did not verify: {error}"))
            })?;

        Ok(principal_of(claims))
    }
}

/// Read the discovery document over the in-cluster address.
///
/// [`CoreProviderMetadata::discover_async`] cannot be pointed here: it fetches
/// `{url}/.well-known/openid-configuration` and then requires the document to
/// name `url` as its issuer. Dex names the *public* issuer, as it must — that
/// string ends up in `iss` — so discovery over any other address is a
/// validation failure by construction. Hence the fetch by hand, with the same
/// check made against the issuer that was configured rather than the address
/// that served it.
///
/// # Errors
///
/// [`OidcError::Discovery`] if the address cannot be reached, answers anything
/// but `200`, serves something that is not a discovery document, or serves one
/// belonging to a different issuer.
async fn discover_internally(
    http: &reqwest::Client,
    issuer: &str,
    internal: &str,
) -> Result<CoreProviderMetadata, OidcError> {
    let issuer = issuer.trim_end_matches('/');
    let internal = internal.trim_end_matches('/');
    let url = format!("{internal}/.well-known/openid-configuration");

    let response = http
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| OidcError::Discovery(format!("{url}: {error}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(OidcError::Discovery(format!(
            "HTTP status code {status} at {url}"
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|error| OidcError::Discovery(format!("{url}: {error}")))?;

    let metadata: CoreProviderMetadata = serde_json::from_str(&body).map_err(|error| {
        OidcError::Discovery(format!("{url} is not a discovery document: {error}"))
    })?;

    // The check `discover_async` would have made, and what keeps this
    // discovery rather than assertion: whatever answered privately has to be
    // the provider whose name the tokens will carry.
    if metadata.issuer().as_str().trim_end_matches('/') != issuer {
        return Err(OidcError::Discovery(format!(
            "unexpected issuer URI `{}` (expected `{issuer}`) at {url}",
            metadata.issuer().as_str()
        )));
    }

    let metadata = internalise(metadata, issuer, internal)?;

    // `jwks` is `#[serde(skip)]`: a document that was parsed rather than
    // discovered arrives with an empty key set, and every signature check
    // would fail against it. `discover_async` does this second fetch too.
    let jwks = CoreJsonWebKeySet::fetch_async(metadata.jwks_uri(), http)
        .await
        .map_err(|error| OidcError::Discovery(error.to_string()))?;

    Ok(metadata.set_jwks(jwks))
}

/// Point the endpoints *this process* calls at the in-cluster address.
///
/// Dex builds every URL it advertises from the issuer, so a document fetched
/// privately still reads `https://kaas.smeding.cloud/dex/token`. Two of those
/// endpoints are dialled by kaas-ui — the token exchange and the key set — and
/// they are rewritten here.
///
/// **`authorization_endpoint` is deliberately left public.** The party that
/// fetches it is the browser, and no browser resolves
/// `dex.dex.svc.cluster.local`. Rewriting all four endpoints uniformly is the
/// tempting simplification and it breaks login in a way no test that does not
/// drive a browser would notice: the redirect goes out to a name only the
/// cluster can resolve. `issuer` is left alone for the same class of reason —
/// it is an identity that is compared, not an address that is dialled.
///
/// # Errors
///
/// [`OidcError::Discovery`] if a rewritten endpoint stops being a URL.
fn internalise(
    metadata: CoreProviderMetadata,
    issuer: &str,
    internal: &str,
) -> Result<CoreProviderMetadata, OidcError> {
    let jwks_uri =
        JsonWebKeySetUrl::new(swap_prefix(metadata.jwks_uri().as_str(), issuer, internal))
            .map_err(|error| OidcError::Discovery(format!("jwks_uri is not a URL: {error}")))?;

    let token_endpoint = metadata
        .token_endpoint()
        .map(|endpoint| {
            TokenUrl::new(swap_prefix(endpoint.as_str(), issuer, internal)).map_err(|error| {
                OidcError::Discovery(format!("token_endpoint is not a URL: {error}"))
            })
        })
        .transpose()?;

    Ok(metadata
        .set_jwks_uri(jwks_uri)
        .set_token_endpoint(token_endpoint))
}

/// `endpoint` with the issuer swapped for the in-cluster address.
///
/// Unchanged, and said out loud, if it does not begin with the issuer. Dex
/// serves everything under its issuer's path so in practice every endpoint
/// matches; one that does not is somewhere this rewrite has no business
/// pointing, and guessing would send a token exchange to the wrong host.
fn swap_prefix(endpoint: &str, issuer: &str, internal: &str) -> String {
    match endpoint.strip_prefix(issuer) {
        Some(rest) => format!("{internal}{rest}"),
        None => {
            tracing::warn!(
                %endpoint,
                %issuer,
                "the provider advertises an endpoint outside its issuer; leaving it public"
            );
            endpoint.to_owned()
        }
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
            metadata: ArcSwap::from_pointee(metadata),
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
            internal_url: None,
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
    fn a_login_verifies_against_the_current_keys_not_the_ones_discovered_at_startup() {
        // The regression this guards is a live one: Dex rotates its signing
        // key on a schedule, and a process that pinned the key set at
        // discovery fails **every** login from the first rotation onward with
        // `Signature verification failed`, without recovering. Collapsing the
        // ArcSwap back into a plain field compiles, passes every other test in
        // this module, and breaks logins some hours after each deploy.
        //
        // Asserted through the authorization endpoint because that is the part
        // of the metadata a unit test can see; what matters is that `client()`
        // reads what is in the cell now rather than a snapshot beside it.
        let provider = Provider::from_metadata(recorded_metadata(), recorded_config());
        let (before, _) = provider.start_login().expect("the fixture is valid");
        assert!(
            before.starts_with("https://kaas.smeding.cloud/dex/auth?"),
            "{before}"
        );

        let rotated: CoreProviderMetadata = serde_json::from_str(
            r#"{
                "issuer": "https://kaas.smeding.cloud/dex",
                "authorization_endpoint": "https://kaas.smeding.cloud/dex/rotated",
                "token_endpoint": "https://kaas.smeding.cloud/dex/token",
                "jwks_uri": "https://kaas.smeding.cloud/dex/keys",
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"]
            }"#,
        )
        .expect("the fixture parses");
        provider.metadata.store(Arc::new(rotated));

        let (after, _) = provider.start_login().expect("the fixture is valid");
        assert!(
            after.starts_with("https://kaas.smeding.cloud/dex/rotated?"),
            "the client is pinned to the metadata it booted with: {after}"
        );
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
            internal_url: None,
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
            internal_url: None,
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

    const INTERNAL: &str = "http://dex.dex.svc.cluster.local:5556/dex";
    const ISSUER: &str = "https://kaas.smeding.cloud/dex";

    #[test]
    fn the_browser_hop_stays_public_while_the_server_hops_go_in_cluster() {
        // The whole point of the split, and the one way to get it wrong that
        // no test short of driving a browser would catch: rewriting the
        // authorization endpoint too sends the *user* to a name only the
        // cluster can resolve. Login would fail at the first redirect, on the
        // deployed instance only.
        let metadata = internalise(recorded_metadata(), ISSUER, INTERNAL)
            .expect("the fixture's endpoints stay URLs");

        assert_eq!(
            metadata.authorization_endpoint().as_str(),
            "https://kaas.smeding.cloud/dex/auth",
            "the browser cannot resolve a Service DNS name"
        );
        assert_eq!(
            metadata.token_endpoint().map(|url| url.as_str()),
            Some("http://dex.dex.svc.cluster.local:5556/dex/token")
        );
        assert_eq!(
            metadata.jwks_uri().as_str(),
            "http://dex.dex.svc.cluster.local:5556/dex/keys"
        );
        // An identity, not an address. It is what `iss` is checked against.
        assert_eq!(metadata.issuer().as_str(), ISSUER);
    }

    #[test]
    fn a_login_started_over_the_internal_address_still_redirects_publicly() {
        // The property above, asserted where it actually bites: the URL handed
        // to the browser.
        let metadata =
            internalise(recorded_metadata(), ISSUER, INTERNAL).expect("the fixture internalises");
        let provider = Provider::from_metadata(metadata, recorded_config());

        let (url, _) = provider.start_login().expect("the fixture is valid");
        assert!(
            url.starts_with("https://kaas.smeding.cloud/dex/auth?"),
            "{url}"
        );
    }

    #[test]
    fn an_endpoint_outside_the_issuer_is_left_alone() {
        // Guessing would point a token exchange at a host the provider never
        // named. Dex does not do this; something else might.
        assert_eq!(
            swap_prefix("https://elsewhere.test/token", ISSUER, INTERNAL),
            "https://elsewhere.test/token"
        );
    }

    #[test]
    fn a_trailing_slash_on_either_address_does_not_double_up() {
        // Both strings are hand-written in a ConfigMap, and `.../dex/` is the
        // spelling somebody eventually uses.
        assert_eq!(
            swap_prefix(
                "https://kaas.smeding.cloud/dex/token",
                ISSUER.trim_end_matches('/'),
                "http://dex.dex.svc.cluster.local:5556/dex/".trim_end_matches('/')
            ),
            "http://dex.dex.svc.cluster.local:5556/dex/token"
        );
    }
}

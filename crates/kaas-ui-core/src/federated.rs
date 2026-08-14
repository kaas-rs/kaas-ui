//! Workload-identity federation: a token bought with an assertion, not a secret.
//!
//! The same `client_credentials` exchange
//! [`OidcTokenProvider`](kafka_conn::OidcTokenProvider) runs, with the one
//! difference that matters operationally: what kaas-ui presents to the issuer
//! is not a shared secret it holds forever but a short-lived JWT signed by
//! something else — a SPIFFE JWT-SVID written to a file by the SPIRE agent,
//! which is what a Kubernetes projected token is on the managed clouds. There
//! is no credential in a Secret, nothing to rotate, and nothing that keeps
//! working after the pod is gone.
//!
//! Three things about it are easy to get wrong, and each shows up as an
//! `invalid_client` that looks like a typo:
//!
//! * **The assertion is re-read on every fetch.** It expires — SPIRE mints
//!   JWT-SVIDs with a five-minute default TTL here, an order of magnitude
//!   shorter than the access token it buys — and the helper rewrites the file
//!   underneath us. Reading it once at startup produces a process that
//!   authenticates all afternoon and then cannot, which is precisely the
//!   failure [`OauthCredentials`](crate::config::OauthCredentials) exists to
//!   avoid on the other side of the exchange.
//! * **The audience is the issuer's, not ours.** Entra rejects an assertion
//!   whose `aud` is anything but `api://AzureADTokenExchange`, and that is a
//!   property of the *file*: it is spiffe-helper's `jwt_audience`, decided in
//!   the deployment, and nothing here can correct it.
//! * **The subject is the SPIFFE ID**, so the federated credential in the
//!   identity provider is bound to a namespace and a service account. Renaming
//!   either — in the Deployment, not here — stops the exchange dead.
//!
//! This lives in kaas-ui rather than in kaas-lib because kaas-lib's
//! [`TokenProvider`] is explicitly a caller-supplied trait, and a client
//! library gaining an opinion about where a file of JWTs comes from is a
//! bigger claim than a UI needing one. `docs/reference/upstream-asks.md`
//! carries the ask to move it down.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kafka_conn::{Error, Result, TokenFuture, TokenProvider};
use tokio::sync::Mutex;

/// RFC 7523's assertion type, and the only value Entra, Keycloak or Auth0
/// accept. Named rather than inlined because it is the one string in the form
/// body that is not configuration.
const ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

/// The longest lifetime a token response is believed to claim.
///
/// Same cap, and same reason, as kaas-lib's: it keeps the expiry arithmetic
/// below out of overflow territory by construction. A day is far above what
/// any issuer hands out — Entra's is an hour — and far below anything that can
/// wrap.
const MAX_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// How much of a failure body to quote back. Entra's `error_description` opens
/// with the AADSTS code, which is the actionable half; the rest is a trace id
/// and a timestamp.
const MAX_DETAIL_CHARS: usize = 400;

/// How to buy an access token with a signed assertion.
///
/// Mirrors [`OidcConfig`](kafka_conn::OidcConfig)'s surface deliberately: the
/// two differ in one field — a file path where the other has a secret — and a
/// reader comparing them should see that and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FederatedOauth {
    token_endpoint: String,
    client_id: String,
    assertion_file: PathBuf,
    scope: Option<String>,
    audience: Option<String>,
    refresh_margin: Duration,
    timeout: Duration,
    allow_plaintext_endpoint: bool,
}

impl FederatedOauth {
    /// Everything the exchange cannot be attempted without.
    ///
    /// No `Debug` care is needed on this type, unlike its secret-bearing
    /// sibling: a path is not a credential, and the credential itself is never
    /// held — it is read, sent, and dropped.
    #[must_use]
    pub fn new(
        token_endpoint: impl Into<String>,
        client_id: impl Into<String>,
        assertion_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            token_endpoint: token_endpoint.into(),
            client_id: client_id.into(),
            assertion_file: assertion_file.into(),
            scope: None,
            audience: None,
            refresh_margin: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            allow_plaintext_endpoint: false,
        }
    }

    /// Request a scope. Entra wants one; Keycloak usually wants nothing.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Request a scope, if there is one to request.
    #[must_use]
    pub fn with_maybe_scope(mut self, scope: Option<impl Into<String>>) -> Self {
        self.scope = scope.map(Into::into);
        self
    }

    /// Request an `audience` — the *token's* audience, which is Auth0's and
    /// Keycloak's way of saying what the access token is for. Not the
    /// assertion's audience, which is fixed by whoever wrote the file.
    #[must_use]
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Request an `audience`, if there is one to request.
    #[must_use]
    pub fn with_maybe_audience(mut self, audience: Option<impl Into<String>>) -> Self {
        self.audience = audience.map(Into::into);
        self
    }

    /// Refresh this long before the token expires.
    #[must_use]
    pub fn with_refresh_margin(mut self, margin: Duration) -> Self {
        self.refresh_margin = margin;
        self
    }

    /// Deadline for one token fetch.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Permit an `http://` token endpoint — a local issuer in development.
    ///
    /// Off by default for a reason that survives the move away from a client
    /// secret: the assertion on the wire *is* the credential, and one lifted
    /// off a plaintext hop can be spent by whoever lifted it until it expires.
    #[must_use]
    pub fn with_allow_plaintext_endpoint(mut self) -> Self {
        self.allow_plaintext_endpoint = true;
        self
    }
}

/// One cached access token, and the two instants that govern it.
struct Cached {
    token: String,
    /// When to try for a fresh one — early, by design.
    refresh_after: Instant,
    /// When the one we hold is genuinely no good.
    expires_at: Instant,
}

/// A [`TokenProvider`] that exchanges a file of JWT for an access token.
///
/// Caches, refreshes early, and — like kaas-lib's — presents a still-valid
/// cached token when a refresh fails, because the refresh being early is what
/// makes that safe.
pub struct FederatedTokenProvider {
    config: FederatedOauth,
    endpoint: reqwest::Url,
    http: reqwest::Client,
    cached: Mutex<Option<Cached>>,
}

impl std::fmt::Debug for FederatedTokenProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FederatedTokenProvider")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl FederatedTokenProvider {
    /// Build a provider. Reads nothing and fetches nothing.
    ///
    /// Which is what makes the checks it *does* run worth having at startup: a
    /// token endpoint that is not a URL, or is `http://` without the opt-in, is
    /// a configuration mistake, and a configuration mistake should not wait for
    /// a broker to be reachable to be reported. The assertion file is
    /// deliberately not checked here — it is written by a sidecar that may
    /// legitimately be a few seconds behind us, and an absent file at startup
    /// is a retry, not an error.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidRequest`] when the endpoint is unusable, and
    /// [`Error::TokenEndpoint`] when the HTTP client cannot be built at all.
    pub fn new(config: FederatedOauth) -> Result<Self> {
        let endpoint: reqwest::Url = config.token_endpoint.parse().map_err(|e| {
            Error::InvalidRequest(format!(
                "token endpoint {:?} is not a valid url: {e}",
                config.token_endpoint
            ))
        })?;

        match endpoint.scheme() {
            "https" => {}
            "http" if config.allow_plaintext_endpoint => {
                tracing::warn!(
                    endpoint = %endpoint,
                    "exchanging assertions over http; the assertion is on the wire in the clear"
                );
            }
            "http" => {
                return Err(Error::InvalidRequest(format!(
                    "token endpoint {endpoint} is http, which would send the client assertion \
                     in the clear; use https or opt in with \
                     FederatedOauth::with_allow_plaintext_endpoint"
                )));
            }
            other => {
                return Err(Error::InvalidRequest(format!(
                    "token endpoint {endpoint} has scheme {other:?}, which is not http(s)"
                )));
            }
        }

        // `reqwest` is built with `rustls-no-provider` for the musl image's
        // sake, so somebody has to install one before the first HTTPS client
        // exists. kaas-ui-serde is that somebody and this is the second crate
        // to need it — see the note on the function.
        kaas_ui_serde::install_crypto_provider();

        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            // Belt as well as braces: the scheme gate above already refused
            // http, and a redirect to it would be a way around that gate.
            .https_only(!config.allow_plaintext_endpoint)
            .build()
            .map_err(|e| Error::TokenEndpoint {
                endpoint: config.token_endpoint.clone(),
                status: None,
                detail: format!("could not be given an http client: {e}"),
            })?;

        Ok(Self {
            config,
            endpoint,
            http,
            cached: Mutex::new(None),
        })
    }

    /// The current access token, fetching or refreshing if it is time.
    ///
    /// # Errors
    ///
    /// [`Error::TokenEndpoint`] when the assertion cannot be read or the issuer
    /// will not exchange it, and there is no unexpired token to fall back to.
    pub async fn current_token(&self) -> Result<String> {
        let mut cached = self.cached.lock().await;
        if let Some(current) = cached.as_ref()
            && Instant::now() < current.refresh_after
        {
            return Ok(current.token.clone());
        }

        match self.fetch().await {
            Ok(fresh) => {
                let token = fresh.token.clone();
                *cached = Some(fresh);
                Ok(token)
            }
            // The refresh is early, so a failed one does not mean the token in
            // hand is unusable. Presenting it beats failing a connection over a
            // sidecar that is mid-renewal, and the warning is what makes the
            // eventual hard failure explicable.
            Err(error) => match cached.as_ref().filter(|c| Instant::now() < c.expires_at) {
                Some(current) => {
                    tracing::warn!(
                        endpoint = %self.endpoint,
                        %error,
                        expires_in_s = current
                            .expires_at
                            .saturating_duration_since(Instant::now())
                            .as_secs(),
                        "token refresh failed; presenting the cached token"
                    );
                    Ok(current.token.clone())
                }
                None => Err(error),
            },
        }
    }

    /// One exchange: read the assertion, post it, believe the answer.
    async fn fetch(&self) -> Result<Cached> {
        let assertion = self.assertion().await?;

        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "client_credentials"),
            ("client_id", &self.config.client_id),
            ("client_assertion_type", ASSERTION_TYPE),
            ("client_assertion", &assertion),
        ];
        if let Some(scope) = &self.config.scope {
            form.push(("scope", scope));
        }
        if let Some(audience) = &self.config.audience {
            form.push(("audience", audience));
        }

        let response = self
            .http
            .post(self.endpoint.clone())
            .form(&form)
            .send()
            .await
            .map_err(|e| self.unreachable(format!("could not be reached: {e}")))?;

        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| self.unreachable(format!("answered {status} and then stopped: {e}")))?;

        if !status.is_success() {
            return Err(Error::TokenEndpoint {
                endpoint: self.endpoint.to_string(),
                status: Some(status.as_u16()),
                detail: format!("refused the assertion: {}", describe_failure(&body)),
            });
        }

        let (token, lifetime) = parse_token_response(self.endpoint.as_ref(), &body)?;
        let usable = usable_lifetime(lifetime, self.config.refresh_margin);
        let now = Instant::now();

        tracing::debug!(
            endpoint = %self.endpoint,
            lifetime_s = lifetime.as_secs(),
            refresh_in_s = usable.as_secs(),
            "exchanged a client assertion for an access token"
        );

        Ok(Cached {
            token,
            refresh_after: now + usable,
            expires_at: now + lifetime,
        })
    }

    /// The assertion, as of now.
    ///
    /// Read every time. The file is rewritten by the sidecar on its own
    /// schedule and holding the contents would be holding a credential that
    /// silently goes stale.
    async fn assertion(&self) -> Result<String> {
        let path = &self.config.assertion_file;
        let raw = tokio::fs::read_to_string(path).await.map_err(|e| {
            self.unreachable(format!(
                "could not be asked: the client assertion at {} could not be read: {e}",
                path.display()
            ))
        })?;

        // Trailing newlines are what a file written by a helper has, and a JWT
        // with one is rejected as malformed by some issuers and accepted by
        // others — the kind of difference that makes a working setup break on
        // the day the issuer changes.
        let assertion = raw.trim().to_owned();
        if assertion.is_empty() {
            return Err(self.unreachable(format!(
                "could not be asked: the client assertion at {} is empty",
                path.display()
            )));
        }
        Ok(assertion)
    }

    /// An endpoint that was never reached, or never asked.
    ///
    /// `status: None` is not cosmetic — it is what makes kaas-lib treat the
    /// failure as retriable, which is right for a sidecar that has not written
    /// the file yet and wrong for an issuer that refused.
    fn unreachable(&self, detail: String) -> Error {
        Error::TokenEndpoint {
            endpoint: self.endpoint.to_string(),
            status: None,
            detail,
        }
    }

    /// The file the assertion is read from, for a caller that wants to say so.
    #[must_use]
    pub fn assertion_file(&self) -> &Path {
        &self.config.assertion_file
    }
}

impl TokenProvider for FederatedTokenProvider {
    fn token(&self) -> TokenFuture<'_> {
        Box::pin(self.current_token())
    }
}

/// What an issuer said when it refused, in as few characters as carry meaning.
///
/// OAuth's failure body is JSON with `error` and `error_description`, and
/// Entra's description opens with the AADSTS code that names the actual
/// problem — a missing federated credential, a subject that does not match, an
/// audience the assertion was not minted for. Quoting the raw body is the
/// fallback, because an issuer behind a proxy answers with HTML and saying so
/// is more useful than saying "not JSON".
fn describe_failure(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let json: Option<serde_json::Value> = serde_json::from_str(&text).ok();

    let described = json.as_ref().map(|json| {
        let code = json.get("error").and_then(serde_json::Value::as_str);
        let description = json
            .get("error_description")
            .and_then(serde_json::Value::as_str);
        match (code, description) {
            (Some(code), Some(description)) => format!("{code}: {description}"),
            (Some(code), None) => code.to_owned(),
            (None, Some(description)) => description.to_owned(),
            (None, None) => text.to_string(),
        }
    });

    let full = described.unwrap_or_else(|| text.to_string());
    truncate(full.trim(), MAX_DETAIL_CHARS)
}

/// Cut a message to a character budget, saying that it was cut.
fn truncate(message: &str, budget: usize) -> String {
    if message.chars().count() <= budget {
        return message.to_owned();
    }
    let kept: String = message.chars().take(budget).collect();
    format!("{kept}…")
}

/// How much of a token's lifetime to use before refreshing.
///
/// 80% of the window, never closer to expiry than the margin, and never less
/// than half the lifetime. The same three rules kaas-lib's provider uses, and
/// the floor matters for the same reason: a 60-second margin against a
/// 30-second test token would otherwise mean "refresh on every single call".
fn usable_lifetime(lifetime: Duration, margin: Duration) -> Duration {
    let millis = u64::try_from(lifetime.as_millis()).unwrap_or(u64::MAX);
    let by_factor = Duration::from_millis(millis.saturating_mul(8) / 10);
    let by_margin = lifetime.saturating_sub(margin);
    by_factor.min(by_margin).max(lifetime / 2)
}

/// Read `access_token` and `expires_in` out of a token response.
fn parse_token_response(endpoint: &str, body: &[u8]) -> Result<(String, Duration)> {
    let refused = |detail: String| Error::TokenEndpoint {
        endpoint: endpoint.to_owned(),
        status: Some(200),
        detail,
    };

    let json: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| refused(format!("answered with something that is not JSON: {e}")))?;

    let token = json
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| refused("answered without an access_token".to_owned()))?
        .to_owned();

    // A number per RFC 6749, and a string often enough in the wild to be worth
    // accepting. An answer with neither is not an error: an issuer that does
    // not say gets the conservative assumption, which is that the token is
    // about to expire rather than that it lasts forever.
    let seconds = json
        .get("expires_in")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(60);

    Ok((token, Duration::from_secs(seconds).min(MAX_LIFETIME)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;

    /// One canned HTTP response, and the request that provoked it.
    ///
    /// A real socket rather than an injected client: the assertion travels in a
    /// form body, and what this whole module is for is that the *body* is
    /// right. A test against a mocked trait would prove the code calls itself
    /// correctly and nothing about what goes on the wire.
    async fn issuer(status: &str, body: &str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );

        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let read = socket.read(&mut request).await.unwrap();
            request.truncate(read);
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });

        (format!("http://{address}/token"), handle)
    }

    /// A file standing in for the one spiffe-helper writes.
    fn assertion_file(name: &str, contents: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("kaas-ui-assertion-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[tokio::test]
    async fn the_assertion_is_what_is_posted() {
        let (endpoint, request) = issuer(
            "200 OK",
            r#"{"access_token":"an-access-token","expires_in":3600}"#,
        )
        .await;
        let file = assertion_file("posted", "header.payload.signature\n");

        let provider = FederatedTokenProvider::new(
            FederatedOauth::new(endpoint, "the-client-id", &file)
                .with_scope("the-client-id/.default")
                .with_allow_plaintext_endpoint(),
        )
        .unwrap();

        assert_eq!(provider.current_token().await.unwrap(), "an-access-token");

        let sent = request.await.unwrap();
        assert!(sent.contains("grant_type=client_credentials"), "{sent}");
        assert!(sent.contains("client_id=the-client-id"), "{sent}");
        assert!(
            sent.contains(
                "client_assertion_type=urn%3Aietf%3Aparams%3Aoauth%3Aclient-assertion-type%3Ajwt-bearer"
            ),
            "{sent}"
        );
        // Trimmed: the newline the helper leaves behind is not part of the JWT.
        assert!(
            sent.contains("client_assertion=header.payload.signature&"),
            "{sent}"
        );
        assert!(sent.contains("scope=the-client-id%2F.default"), "{sent}");

        std::fs::remove_file(&file).unwrap();
    }

    #[tokio::test]
    async fn a_second_call_inside_the_window_does_not_ask_again() {
        // The issuer accepts exactly one connection, so a second exchange would
        // fail the call rather than quietly cost a round trip.
        let (endpoint, request) = issuer(
            "200 OK",
            r#"{"access_token":"an-access-token","expires_in":3600}"#,
        )
        .await;
        let file = assertion_file("cached", "header.payload.signature");

        let provider = FederatedTokenProvider::new(
            FederatedOauth::new(endpoint, "id", &file).with_allow_plaintext_endpoint(),
        )
        .unwrap();

        assert_eq!(provider.current_token().await.unwrap(), "an-access-token");
        assert_eq!(provider.current_token().await.unwrap(), "an-access-token");

        request.await.unwrap();
        std::fs::remove_file(&file).unwrap();
    }

    #[tokio::test]
    async fn an_absent_assertion_names_the_path_and_stays_retriable() {
        let provider = FederatedTokenProvider::new(
            FederatedOauth::new(
                "https://issuer/token",
                "id",
                "/var/run/spiffe/svid/azure-token",
            )
            .with_refresh_margin(Duration::from_secs(5)),
        )
        .unwrap();

        let error = provider.current_token().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("/var/run/spiffe/svid/azure-token"),
            "{error}"
        );
        // The sidecar may simply not have written it yet, which is a wait, not
        // a misconfiguration.
        assert!(error.retriable(), "{error}");
    }

    #[tokio::test]
    async fn a_refused_assertion_is_not_retried_and_says_why() {
        let (endpoint, request) = issuer(
            "400 Bad Request",
            r#"{"error":"invalid_client","error_description":"AADSTS700213: No matching federated identity record found."}"#,
        )
        .await;
        let file = assertion_file("refused", "header.payload.signature");

        let provider = FederatedTokenProvider::new(
            FederatedOauth::new(endpoint, "id", &file).with_allow_plaintext_endpoint(),
        )
        .unwrap();

        let error = provider.current_token().await.unwrap_err();
        assert!(error.to_string().contains("AADSTS700213"), "{error}");
        assert!(!error.retriable(), "{error}");

        request.await.unwrap();
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn eighty_percent_of_a_lifetime_unless_the_margin_says_sooner() {
        let hour = Duration::from_secs(3600);
        assert_eq!(
            usable_lifetime(hour, Duration::from_secs(60)),
            Duration::from_secs(2880)
        );
        // A margin larger than the token's own lifetime cannot mean "refresh
        // now, forever".
        assert_eq!(
            usable_lifetime(Duration::from_secs(30), Duration::from_secs(60)),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn expires_in_is_read_as_a_number_or_a_string() {
        let number =
            parse_token_response("i", br#"{"access_token":"t","expires_in":120}"#).unwrap();
        let string =
            parse_token_response("i", br#"{"access_token":"t","expires_in":"120"}"#).unwrap();
        assert_eq!(number.1, Duration::from_secs(120));
        assert_eq!(string.1, Duration::from_secs(120));

        // No claim at all is a minute, not forever.
        let silent = parse_token_response("i", br#"{"access_token":"t"}"#).unwrap();
        assert_eq!(silent.1, Duration::from_secs(60));
    }

    #[test]
    fn a_response_without_a_token_names_the_endpoint() {
        let error = parse_token_response("https://issuer/token", br#"{"token_type":"Bearer"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("https://issuer/token"), "{error}");
        assert!(error.contains("without an access_token"), "{error}");
    }

    #[test]
    fn a_refusal_is_quoted_back_with_its_code() {
        let described = describe_failure(
            br#"{"error":"invalid_client","error_description":"AADSTS700213: No matching federated identity record found."}"#,
        );
        assert!(described.contains("invalid_client"), "{described}");
        assert!(described.contains("AADSTS700213"), "{described}");
    }

    #[test]
    fn a_refusal_that_is_not_json_is_quoted_anyway() {
        let described = describe_failure(b"<html>502 Bad Gateway</html>");
        assert!(described.contains("502 Bad Gateway"), "{described}");
    }

    #[test]
    fn an_http_endpoint_is_refused_without_the_opt_in() {
        let error =
            FederatedTokenProvider::new(FederatedOauth::new("http://issuer/token", "id", "/f"))
                .unwrap_err()
                .to_string();
        assert!(error.contains("in the clear"), "{error}");
    }

    #[test]
    fn a_token_endpoint_that_is_not_a_url_is_refused() {
        let error = FederatedTokenProvider::new(FederatedOauth::new("issuer/token", "id", "/f"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a valid url"), "{error}");
    }
}

//! How to reach one Connect cluster.

use std::path::PathBuf;
use std::time::Duration;

/// The default per-call timeout: Redpanda Console's, which is what a Connect
/// worker answering a status call in a healthy cluster comfortably fits in.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(6);

/// The default TCP connect timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the expanded connector list is believed.
///
/// Short, because connector state is exactly what someone is watching change
/// — a task restarting, a connector failing — and long enough that a page and
/// the fleet card asking within the same second cost the worker one call.
pub const DEFAULT_CONNECTORS_TTL: Duration = Duration::from_secs(5);

/// How long a connector's active-topic list is believed, for the reverse
/// index a topic page reads. The connector page itself always reads fresh.
pub const DEFAULT_TOPICS_TTL: Duration = Duration::from_secs(30);

/// How long a plugin's config definition is believed.
///
/// Changes only when the worker's plugin path does, which is a restart.
pub const DEFAULT_PLUGIN_TTL: Duration = Duration::from_secs(600);

/// How to reach one Connect cluster.
///
/// Required data in [`ConnectSettings::new`], everything optional through a
/// consuming `with_*` builder — `STYLE.md` rule 8, no exceptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSettings {
    /// The configured id, unique within its environment.
    pub id: String,
    /// The name to render. Defaults to the id.
    pub name: String,
    /// The REST base url, as in `http://connect-api:8083`.
    pub url: String,
    /// Credentials, where the worker wants them.
    pub auth: Option<ConnectAuth>,
    /// TLS beyond the system roots.
    pub tls: Option<ConnectTls>,
    /// Per-call timeout.
    pub request_timeout: Duration,
    /// TCP connect timeout.
    pub connect_timeout: Duration,
    /// How long the expanded connector list is believed.
    pub connectors_ttl: Duration,
    /// How long a plugin's config definition is believed.
    pub plugin_ttl: Duration,
}

impl ConnectSettings {
    /// A Connect cluster at `url`, known by `id`.
    #[must_use]
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            url: url.into(),
            auth: None,
            tls: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            connectors_ttl: DEFAULT_CONNECTORS_TTL,
            plugin_ttl: DEFAULT_PLUGIN_TTL,
        }
    }

    /// The name to render instead of the id.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Credentials.
    #[must_use]
    pub fn with_auth(mut self, auth: ConnectAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// TLS beyond the system roots.
    #[must_use]
    pub fn with_tls(mut self, tls: ConnectTls) -> Self {
        self.tls = Some(tls);
        self
    }

    /// Per-call timeout.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// TCP connect timeout.
    #[must_use]
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// How long the expanded connector list is believed.
    #[must_use]
    pub fn with_connectors_ttl(mut self, ttl: Duration) -> Self {
        self.connectors_ttl = ttl;
        self
    }

    /// How long a plugin's config definition is believed.
    #[must_use]
    pub fn with_plugin_ttl(mut self, ttl: Duration) -> Self {
        self.plugin_ttl = ttl;
        self
    }

    /// The base url with any trailing slash removed, which is what every call
    /// joins a path onto.
    pub(crate) fn base(&self) -> &str {
        self.url.trim_end_matches('/')
    }
}

/// How a Connect worker wants to be authenticated.
///
/// Connect's own REST auth is a JAAS basic-auth extension; a bearer token is
/// what sits in front of it when something else — a gateway, Confluent's RBAC
/// — does the authenticating.
#[derive(Clone, PartialEq, Eq)]
pub enum ConnectAuth {
    /// HTTP basic.
    Basic {
        /// The principal.
        username: String,
        /// The secret.
        password: String,
    },
    /// A bearer token, sent verbatim.
    Bearer(String),
}

impl std::fmt::Debug for ConnectAuth {
    /// The leak that happens is `?settings` in a log line, not a deliberate
    /// print, so the secret redacts itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Basic { username, .. } => f
                .debug_struct("Basic")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            Self::Bearer(_) => f.debug_tuple("Bearer").field(&"<redacted>").finish(),
        }
    }
}

/// TLS settings for a Connect REST endpoint. Files, never inline PEM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectTls {
    /// A PEM bundle to trust in addition to the system roots.
    pub ca_file: Option<PathBuf>,
    /// Client certificate chain and key, for mTLS. Both or neither.
    pub client: Option<(PathBuf, PathBuf)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_keeps_its_shape_whatever_was_typed() {
        let settings = ConnectSettings::new("connect", "http://connect:8083/");
        assert_eq!(settings.base(), "http://connect:8083");
        assert_eq!(settings.url, "http://connect:8083/");
    }

    #[test]
    fn debug_never_prints_a_secret() {
        let basic = ConnectAuth::Basic {
            username: "ui".into(),
            password: "hunter2".into(),
        };
        let bearer = ConnectAuth::Bearer("t0ken".into());
        let printed = format!("{basic:?} {bearer:?}");
        assert!(!printed.contains("hunter2") && !printed.contains("t0ken"));
        assert!(printed.contains("ui"), "the username is the useful half");
    }
}

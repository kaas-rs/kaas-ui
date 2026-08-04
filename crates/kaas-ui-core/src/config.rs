//! The configuration file, and how it becomes a [`kafka_meta::ClusterConfig`].
//!
//! YAML plus an environment overlay, via figment. Unknown keys are rejected:
//! a config that silently ignores a block someone wrote is worse than one that
//! refuses to start, and the blocks a later phase will add (`schema_registry`,
//! `auth`) are exactly the ones someone would write early and expect to work.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use figment::Figment;
use figment::providers::{Env, Format, Yaml};
use kaas_ui_auth::{OidcConfig, Resource, Role};
use serde::{Deserialize, Serialize};

/// Everything kaas-ui reads at startup.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct Config {
    /// How the HTTP server binds.
    pub server: ServerConfig,
    /// The cluster registry.
    pub clusters: Vec<ClusterEntry>,
    /// Where the login provider is, when kaas-ui serves it under its own
    /// hostname.
    ///
    /// Absent — the default — and nothing is mounted at `/dex`, which is what
    /// a deployment with no authentication wants.
    pub dex: Option<DexConfig>,
    /// The identity provider, when there is one.
    ///
    /// Absent is the open deployment: no login routes, one anonymous caller.
    /// Present, and `/auth/login` exists — but what anyone may *see* is still
    /// `roles` below, so configuring this alone changes nothing except that
    /// kaas-ui learns your name.
    pub auth: Option<OidcConfig>,
    /// Who may see which clusters, and whether they may read payloads.
    ///
    /// Empty is the open deployment: no authentication, one anonymous caller,
    /// every cluster visible with both grants. That is what kaas-ui did before
    /// this block existed and it stays the default, so adding the auth code
    /// changed nothing for anyone who had not asked for it.
    ///
    /// A non-empty list is **enforced**, and until the OIDC exchange lands
    /// there is nobody for a role to cover: every caller is anonymous, so
    /// every cluster is invisible and the fleet is empty. That is the safe
    /// direction for the gap to fail, and [`Config::role_warning`] says so at
    /// startup rather than leaving it to be discovered.
    pub roles: Vec<Role>,
}

/// The Dex this deployment logs in through.
///
/// kaas-ui proxies `/dex/*` to it rather than giving it a hostname of its own.
/// Every browser hop of an OIDC login has to reach the provider, and this way
/// it does so over the name the browser is already on — one DNS record, one
/// public surface. ArgoCD serves its own Dex at `/api/dex` for the same
/// reason.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct DexConfig {
    /// The in-cluster address, as in `http://dex.dex.svc.cluster.local:5556`.
    ///
    /// Plain HTTP on purpose: the hop does not leave the cluster, and the
    /// public leg is terminated by whatever fronts kaas-ui.
    pub upstream: String,
}

/// HTTP server settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct ServerConfig {
    /// Address to bind.
    pub listen: SocketAddr,
    /// The path prefix a reverse proxy mounts kaas-ui under, if any.
    ///
    /// Empty — serving from `/` — is the normal case. Set it when something in
    /// front rewrites the path away before kaas-ui sees it: code-server's
    /// `/proxy/8099`, or an ingress hosting the app at `/kafka`.
    ///
    /// It has to be *told* rather than detected. A stripping proxy forwards no
    /// record of what it removed — code-server sends no `X-Forwarded-Prefix`
    /// and rewrites `Host` to its own — so the request that arrives is
    /// indistinguishable from one made at the root.
    ///
    /// kaas-ui's own routes are unaffected: they are always rooted at `/`,
    /// because the prefix is gone by the time a request is routed. All this
    /// changes is the URLs `index.html` hands the browser.
    pub base_path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 8080)),
            base_path: String::new(),
        }
    }
}

impl ServerConfig {
    /// The prefix as a leading-slash, no-trailing-slash string.
    ///
    /// `""` for the root. Normalised here so every reader gets the same shape
    /// whatever was typed — `/proxy/8099`, `proxy/8099/` and `/proxy/8099/`
    /// are the same deployment, and three call sites each doing their own
    /// trimming is how one of them ends up emitting `//assets/`.
    pub fn base_prefix(&self) -> String {
        let trimmed = self.base_path.trim().trim_matches('/');
        if trimmed.is_empty() {
            String::new()
        } else {
            format!("/{trimmed}")
        }
    }
}

/// One configured Kafka cluster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ClusterEntry {
    /// Stable identifier. Appears in every URL, so it may not contain
    /// anything that would have to be escaped in a path segment.
    pub id: String,
    /// Human-readable name. Defaults to the id.
    #[serde(default)]
    pub name: Option<String>,
    /// Bootstrap servers, `host:port`.
    pub bootstrap: Vec<String>,
    /// Free-form labels. `env` and `kind` group the fleet view; `env: prod`
    /// additionally forces the danger tone on the cluster chip.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// How often the background task refreshes metadata.
    ///
    /// One refresh per cluster forever, whether or not anyone is looking, so
    /// this is per cluster rather than global — a fleet of twelve is twelve
    /// timers.
    #[serde(default, with = "humantime_serde::option")]
    pub refresh_interval: Option<Duration>,
    /// Answer from a refresh rather than the cache once the snapshot is older
    /// than this.
    #[serde(default, with = "humantime_serde::option")]
    pub max_staleness: Option<Duration>,
    /// Connect timeout. This is what an unreachable cluster costs its own
    /// background task, and it is never on a request path.
    #[serde(default, with = "humantime_serde::option")]
    pub connect_timeout: Option<Duration>,
    /// Per-request timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub request_timeout: Option<Duration>,
    /// `client_id` sent to the broker. Defaults to `kaas-ui/<id>`.
    ///
    /// It shows up in broker request logs and quota attribution, which on a
    /// shared cluster is how someone else works out who is generating load.
    #[serde(default)]
    pub client_id: Option<String>,
    /// TLS. Certificates are file paths, never inline PEM: Strimzi delivers
    /// them as mounted Secrets, and a PEM inlined in YAML loses its newlines
    /// in the first careless edit.
    #[serde(default)]
    pub tls: Option<TlsSettings>,
    /// SASL credentials.
    #[serde(default)]
    pub sasl: Option<SaslSettings>,
}

/// TLS settings for one cluster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TlsSettings {
    /// PEM bundle to trust instead of the system roots — the Strimzi cluster
    /// CA, typically mounted from `<cluster>-cluster-ca-cert`.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
    /// Client certificate chain, for mTLS.
    #[serde(default)]
    pub cert_file: Option<PathBuf>,
    /// Client key, for mTLS.
    #[serde(default)]
    pub key_file: Option<PathBuf>,
    /// Override the name used for SNI and hostname verification.
    ///
    /// Not optional in practice: the address dialled through a Kubernetes
    /// Service is routinely not the name on the broker's certificate.
    #[serde(default)]
    pub server_name: Option<String>,
}

/// SASL settings for one cluster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SaslSettings {
    /// `plain`, `scram-sha-256` or `scram-sha-512`.
    pub mechanism: SaslMechanismName,
    /// Principal.
    pub username: String,
    /// Password, inline. Prefer `password_file`.
    #[serde(default)]
    pub password: Option<String>,
    /// Password read from a file — a mounted Secret key, usually.
    #[serde(default)]
    pub password_file: Option<PathBuf>,
    /// Permit `PLAIN` over an unencrypted socket. Off by default, because the
    /// failure mode of getting it wrong is a recoverable password on the wire
    /// and no error anywhere.
    #[serde(default)]
    pub allow_plaintext_password: bool,
}

/// The SASL mechanisms kaas-lib can negotiate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SaslMechanismName {
    /// `PLAIN`.
    Plain,
    /// `SCRAM-SHA-256`.
    ScramSha256,
    /// `SCRAM-SHA-512`.
    ScramSha512,
}

/// Why a configuration was rejected.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read or parsed.
    #[error("{0}")]
    Load(#[from] Box<figment::Error>),
    /// The file parsed but says something impossible.
    #[error("{0}")]
    Invalid(String),
}

impl Config {
    /// Load from a YAML file, overlaid with `KAAS_UI_*` environment variables.
    ///
    /// The overlay uses `__` as the nesting separator, so
    /// `KAAS_UI_SERVER__LISTEN=0.0.0.0:9000` sets `server.listen`, and
    /// `KAAS_UI_SERVER__BASE_PATH=/proxy/8099` sets the path prefix.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let config: Config = Figment::new()
            .merge(Yaml::file_exact(path))
            .merge(
                Env::prefixed("KAAS_UI_")
                    .split("__")
                    .filter(|key| is_ours(key.as_str())),
            )
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))?;
        config.validate()?;
        Ok(config)
    }

    /// Parse from a YAML string. The environment is not consulted.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let config: Config = Figment::new()
            .merge(Yaml::string(yaml))
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))?;
        config.validate()?;
        Ok(config)
    }

    /// What to say at startup about roles nobody can yet match.
    ///
    /// `None` once there is nothing to warn about. Roles are enforced the
    /// moment they are configured — that is the safe direction — but nothing
    /// can authenticate yet, so every caller is anonymous and every role
    /// declines to cover them. The result is a deployment that shows an empty
    /// fleet to everyone, which is correct, deliberate, and utterly baffling
    /// if it is not said out loud.
    #[must_use]
    pub fn role_warning(&self) -> Option<String> {
        if self.roles.is_empty() || self.auth.is_some() {
            return None;
        }
        Some(format!(
            "{} role(s) are configured but no identity provider is, so every request is \
             anonymous and no role covers it: the fleet will be empty for everyone until `auth` \
             is configured. Remove the roles to go back to an open deployment.",
            self.roles.len()
        ))
    }

    /// What to say at startup about a login provider reached the long way
    /// round.
    ///
    /// `None` unless kaas-ui both proxies Dex and discovers it over a public
    /// URL. That pair is how a deployment locks itself out: whatever fronts
    /// kaas-ui routes the issuer's hostname back to kaas-ui, so discovery at
    /// startup asks a process that is not listening yet to describe its own
    /// provider. It answers `502`, the process exits, and no amount of
    /// restarting helps — the thing it is waiting for is itself.
    ///
    /// Not an error, because the issuer may legitimately be somewhere else
    /// entirely. A warning, because when it is not, the failure is a crash
    /// loop that reads like a network problem.
    #[must_use]
    pub fn auth_warning(&self) -> Option<String> {
        let auth = self.auth.as_ref()?;
        if self.dex.is_none() || auth.internal_url.is_some() {
            return None;
        }
        Some(format!(
            "`dex` is proxied here but `auth.internal_url` is unset, so discovery, the token \
             exchange and the key set all go out to {} and come back. If that hostname routes to \
             kaas-ui, this process cannot start without another one already running. Set \
             `auth.internal_url` to the same Dex addressed in-cluster — `dex.upstream` followed \
             by the issuer's path, as in `http://dex.dex.svc.cluster.local:5556/dex`.",
            auth.issuer
        ))
    }

    /// Reject configurations that parse but cannot work.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.clusters.is_empty() {
            return Err(ConfigError::Invalid(
                "no clusters configured: kaas-ui with an empty registry has nothing to show".into(),
            ));
        }

        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
        for cluster in &self.clusters {
            if cluster.id.is_empty() {
                return Err(ConfigError::Invalid("a cluster has an empty id".into()));
            }
            if !cluster
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(ConfigError::Invalid(format!(
                    "cluster id {:?} may only contain letters, digits, '-' and '_': it appears \
                     verbatim in every URL",
                    cluster.id
                )));
            }
            if seen.insert(cluster.id.as_str(), ()).is_some() {
                return Err(ConfigError::Invalid(format!(
                    "duplicate cluster id {:?}",
                    cluster.id
                )));
            }
            if cluster.bootstrap.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "cluster {:?} has no bootstrap servers",
                    cluster.id
                )));
            }
            if let Some(sasl) = &cluster.sasl
                && sasl.password.is_none()
                && sasl.password_file.is_none()
            {
                return Err(ConfigError::Invalid(format!(
                    "cluster {:?} configures sasl without a password or password_file",
                    cluster.id
                )));
            }
            if let Some(tls) = &cluster.tls
                && tls.cert_file.is_some() != tls.key_file.is_some()
            {
                return Err(ConfigError::Invalid(format!(
                    "cluster {:?} configures one half of a client certificate: cert_file and \
                     key_file go together",
                    cluster.id
                )));
            }
        }

        for role in &self.roles {
            if role.name.is_empty() {
                return Err(ConfigError::Invalid(
                    "a role has an empty name: it is what `/api/me` and the audit log report"
                        .into(),
                ));
            }
            if role.subjects.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "role {:?} lists no subjects, so it can never apply to anyone",
                    role.name
                )));
            }
            if role.permissions.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "role {:?} permits nothing: say what it allows, or remove it",
                    role.name
                )));
            }
            for permission in &role.permissions {
                if permission.actions.is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "role {:?} has a permission with no actions",
                        role.name
                    )));
                }
                if permission.value.is_some() && !permission.resource.is_named() {
                    return Err(ConfigError::Invalid(format!(
                        "role {:?} scopes {:?} by value, but that resource has no names for a \
                         pattern to match — the pattern would be silently ignored",
                        role.name, permission.resource
                    )));
                }
                if permission.resource != Resource::Topic
                    && permission
                        .actions
                        .contains(&kaas_ui_auth::Action::MessagesRead)
                {
                    return Err(ConfigError::Invalid(format!(
                        "role {:?} grants `messages_read` on {:?}, which has no messages",
                        role.name, permission.resource
                    )));
                }
            }
        }

        Ok(())
    }
}

impl ClusterEntry {
    /// The name to render, which is the id unless one was given.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Whether a `KAAS_UI_*` variable is one of ours.
///
/// The environment is **not** ours alone. Kubernetes injects service links for
/// every Service in the namespace, so a Service named `kaas-ui` in namespace
/// `kaas-ui` produces `KAAS_UI_PORT`, `KAAS_UI_SERVICE_HOST`,
/// `KAAS_UI_PORT_80_TCP_ADDR` and several more — none of which are
/// configuration, all of which land under this prefix. `--config` also reads
/// `KAAS_UI_CONFIG`.
///
/// Combined with `deny_unknown_fields`, taking the whole prefix meant the
/// process refused to start in the namespace named after itself. That
/// combination is worth keeping — a `schema_registry:` block someone writes
/// before the phase that reads it exists *should* be a startup error — so the
/// overlay is narrowed to the two roots the file has instead.
/// Both separators are accepted because figment applies `split("__")` to the
/// key **before** the filter sees it, so `KAAS_UI_SERVER__LISTEN` arrives here
/// as `server.listen` rather than `server__listen`. Matching only the raw
/// spelling silently drops every legitimate override — which is exactly what
/// the first version of this function did, and what
/// `the_environment_overlay_still_overrides_the_file` now catches.
fn is_ours(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    for root in ["server", "clusters"] {
        if key == root
            || key.starts_with(&format!("{root}."))
            || key.starts_with(&format!("{root}__"))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#;

    #[test]
    fn minimal_config_parses_and_defaults() {
        let config = Config::from_yaml(MINIMAL).unwrap();
        assert_eq!(config.clusters.len(), 1);
        assert_eq!(config.server.listen.port(), 8080);
        assert_eq!(config.clusters[0].display_name(), "kaas");
    }

    #[test]
    fn durations_are_human_readable() {
        let config = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    refresh_interval: 45s
    connect_timeout: 2500ms
"#,
        )
        .unwrap();
        assert_eq!(
            config.clusters[0].refresh_interval,
            Some(Duration::from_secs(45))
        );
        assert_eq!(
            config.clusters[0].connect_timeout,
            Some(Duration::from_millis(2500))
        );
    }

    #[test]
    fn an_unknown_block_is_rejected_rather_than_ignored() {
        // The whole point: someone writes `schema_registry:` before the phase
        // that reads it exists, and finds out at startup rather than by
        // wondering why nothing happened.
        let err = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    schema_registry:
      url: http://localhost:8081
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("schema_registry"), "{err}");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let err = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
  - id: kaas
    bootstrap: ["b:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("duplicate"), "{err}");
    }

    #[test]
    fn an_id_that_would_need_escaping_is_rejected() {
        let err = Config::from_yaml(
            r#"
clusters:
  - id: "prod/eu"
    bootstrap: ["a:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("every URL"), "{err}");
    }

    #[test]
    fn empty_registry_is_rejected() {
        let err = Config::from_yaml("clusters: []").unwrap_err();
        assert!(format!("{err}").contains("no clusters"), "{err}");
    }

    #[test]
    fn the_environment_overlay_ignores_variables_that_are_not_ours() {
        // Every one of these is injected by Kubernetes into a pod in a
        // namespace containing a Service named `kaas-ui`. Before this filter
        // existed, `KAAS_UI_PORT` alone was enough to stop the process from
        // starting — in exactly the deployment it was written for.
        for injected in [
            "PORT",
            "PORT_80_TCP",
            "PORT_80_TCP_ADDR",
            "SERVICE_HOST",
            "SERVICE_PORT",
            "SERVICE_PORT_HTTP",
            "CONFIG",
        ] {
            assert!(!is_ours(injected), "{injected} should be ignored");
        }

        for ours in ["SERVER__LISTEN", "server__listen", "CLUSTERS", "SERVER"] {
            assert!(is_ours(ours), "{ours} should be read");
        }

        // A near miss is still read, so a typo is a startup error rather than
        // a setting that silently did nothing.
        assert!(is_ours("SERVER__LISTENN"));
    }

    /// The other half of the filter above: it must let ours through.
    ///
    /// Written after shipping a filter that rejected Kubernetes' variables
    /// *and* every real override, because `--check` does not print the listen
    /// address and so the first test of it proved nothing.
    #[test]
    // `figment::Error` is a large type and `Jail` insists on it by signature.
    #[allow(clippy::result_large_err)]
    fn the_environment_overlay_still_overrides_the_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.yaml",
                r#"
server:
  listen: "127.0.0.1:8080"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
"#,
            )?;

            // What Kubernetes injects, alongside what an operator sets.
            jail.set_env("KAAS_UI_PORT", "tcp://10.43.1.2:80");
            jail.set_env("KAAS_UI_SERVICE_HOST", "10.43.1.2");
            jail.set_env("KAAS_UI_SERVER__LISTEN", "0.0.0.0:9999");

            let config = Config::load(std::path::Path::new("config.yaml"))
                .map_err(|error| figment::Error::from(error.to_string()))?;

            assert_eq!(config.server.listen.port(), 9999);
            assert_eq!(config.clusters.len(), 1);
            Ok(())
        });
    }

    #[test]
    fn half_a_client_certificate_is_rejected() {
        let err = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    tls:
      cert_file: /etc/certs/user.crt
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("go together"), "{err}");
    }

    /// The deployed shape: Dex proxied here, issuer on kaas-ui's own hostname.
    const PROXIED_DEX: &str = r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
dex:
  upstream: http://dex.dex.svc.cluster.local:5556
auth:
  issuer: https://kaas.smeding.cloud/dex
  client_id: kaas-ui
  redirect_url: https://kaas.smeding.cloud/auth/callback
"#;

    #[test]
    fn proxying_dex_and_discovering_it_publicly_is_warned_about() {
        // The configuration that cannot cold-start: discovery goes to a
        // hostname the tunnel routes back to this process, which is not
        // listening yet. It ran for months because a rolling deploy always
        // left the previous pod up to answer.
        let config = Config::from_yaml(PROXIED_DEX).unwrap();
        let warning = config.auth_warning().expect("this shape deadlocks");
        assert!(warning.contains("auth.internal_url"), "{warning}");
        assert!(
            warning.contains("https://kaas.smeding.cloud/dex"),
            "{warning}"
        );
    }

    #[test]
    fn an_internal_url_parses_and_silences_the_warning() {
        let yaml =
            format!("{PROXIED_DEX}  internal_url: http://dex.dex.svc.cluster.local:5556/dex\n");
        let config = Config::from_yaml(&yaml).unwrap();
        assert_eq!(
            config.auth.as_ref().unwrap().internal_url.as_deref(),
            Some("http://dex.dex.svc.cluster.local:5556/dex")
        );
        assert_eq!(config.auth_warning(), None);
    }

    #[test]
    fn an_external_provider_is_not_warned_about() {
        // No `dex` block: nothing is proxied here, so the issuer resolves to
        // somebody else and there is no cycle to fall into.
        let config = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
auth:
  issuer: https://accounts.example.test
  client_id: kaas-ui
  redirect_url: https://kaas.smeding.cloud/auth/callback
"#,
        )
        .unwrap();
        assert_eq!(config.auth_warning(), None);
    }
}

#[cfg(test)]
mod base_path_tests {
    use super::*;

    #[test]
    fn the_root_is_the_default_and_normalises_to_nothing() {
        assert_eq!(ServerConfig::default().base_prefix(), "");
    }

    #[test]
    fn a_prefix_normalises_however_it_was_typed() {
        // Three spellings of one deployment. Normalising in one place is what
        // stops a caller from emitting `//assets/` by concatenating naively.
        for written in [
            "/proxy/8099",
            "proxy/8099",
            "/proxy/8099/",
            "  /proxy/8099/  ",
        ] {
            let config = ServerConfig {
                base_path: written.to_owned(),
                ..ServerConfig::default()
            };
            assert_eq!(config.base_prefix(), "/proxy/8099", "from {written:?}");
        }
    }

    #[test]
    fn a_prefix_of_only_slashes_is_the_root() {
        for written in ["", "/", "///", "   "] {
            let config = ServerConfig {
                base_path: written.to_owned(),
                ..ServerConfig::default()
            };
            assert_eq!(config.base_prefix(), "", "from {written:?}");
        }
    }

    #[test]
    // `figment::Error` is a large type and `Jail` insists on it by signature.
    #[allow(clippy::result_large_err)]
    fn the_environment_overlay_reaches_it() {
        // The route a debug session takes: no file edit, one variable.
        figment::Jail::expect_with(|jail| {
            // A real cluster, because `validate` rejects an empty registry —
            // kaas-ui with nothing to show is a configuration mistake.
            jail.create_file(
                "kaas-ui.yaml",
                "clusters:\n  - id: kaas\n    bootstrap: [\"broker:9092\"]\n",
            )?;
            jail.set_env("KAAS_UI_SERVER__BASE_PATH", "/proxy/8099");
            let config = Config::load(std::path::Path::new("kaas-ui.yaml")).unwrap();
            assert_eq!(config.server.base_prefix(), "/proxy/8099");
            Ok(())
        });
    }
}

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
use serde::{Deserialize, Serialize};

/// Everything kaas-ui reads at startup.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct Config {
    /// How the HTTP server binds.
    pub server: ServerConfig,
    /// The cluster registry.
    pub clusters: Vec<ClusterEntry>,
}

/// HTTP server settings.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct ServerConfig {
    /// Address to bind.
    pub listen: SocketAddr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 8080)),
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
    /// `KAAS_UI_SERVER__LISTEN=0.0.0.0:9000` sets `server.listen`.
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
fn is_ours(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    for root in ["server", "clusters"] {
        if key == root || key.starts_with(&format!("{root}__")) {
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
}

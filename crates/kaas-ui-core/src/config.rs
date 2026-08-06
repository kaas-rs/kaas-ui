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
use utoipa::ToSchema;

/// Everything kaas-ui reads at startup.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct Config {
    /// How the HTTP server binds.
    pub server: ServerConfig,
    /// The cluster registry.
    pub clusters: Vec<ClusterEntry>,
    /// The environments the fleet is sectioned by.
    ///
    /// Optional, and declaring one buys exactly two things: a display name and
    /// a position. Sections exist without it — a cluster's `env` label is
    /// enough to make one — but "dev, staging, prod" is an order nobody can
    /// derive from the strings, so undeclared environments sort after declared
    /// ones instead of pretending alphabetical was meaningful.
    pub environments: Vec<EnvironmentEntry>,
    /// Everything in an environment that is not a Kafka cluster.
    pub resources: Vec<ResourceEntry>,
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

/// One section of the fleet.
///
/// The id is what a cluster's `env` label and a resource's `environment` name.
/// That label is already a policy selector — `cluster_labels: {env: prod}`
/// selects a role's clusters — so the environment is not a new axis, it is the
/// one the fleet was always grouped by, given a name and an order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct EnvironmentEntry {
    /// Matches a cluster's `env` label, and a resource's `environment`.
    pub id: String,
    /// Human-readable name. Defaults to the id.
    #[serde(default)]
    pub name: Option<String>,
    /// One line under the heading, where the id does not say enough.
    #[serde(default)]
    pub description: Option<String>,
}

impl EnvironmentEntry {
    /// The name to render.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// Something in an environment that is not a Kafka cluster.
///
/// A schema registry, an MQTT broker, a Connect cluster: the things a team
/// reaches for beside its brokers and expects to find in the same place. They
/// are inventory, not monitoring — kaas-ui dials none of them — which is why
/// [`crate::dto::ResourceCard`] has no status field to render green.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ResourceEntry {
    /// Stable identifier, unique among resources.
    pub id: String,
    /// Human-readable name. Defaults to the id.
    #[serde(default)]
    pub name: Option<String>,
    /// What it is. Decides the icon and the wording, nothing else.
    pub kind: ResourceKind,
    /// Which section it belongs to.
    ///
    /// Required, and checked against the environments that actually exist: a
    /// typo here would otherwise open a section of one nobody meant to create,
    /// somewhere down the page, looking exactly like a real environment.
    pub environment: String,
    /// Where it is, as an address a human can act on.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// One line of context on the card.
    #[serde(default)]
    pub note: Option<String>,
    /// Free-form labels, as on a cluster.
    ///
    /// `env` is added from [`Self::environment`] before anything reads these,
    /// so a role's `cluster_labels: {env: prod}` selector covers a resource the
    /// same way it covers a broker.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

impl ResourceEntry {
    /// The name to render.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// The labels a visibility check sees: the configured ones, plus `env`.
    ///
    /// Built rather than stored so the two can never disagree — a resource in
    /// `prod` labelled `env: dev` would be a hole in exactly the selector
    /// people write first.
    #[must_use]
    pub fn effective_labels(&self) -> BTreeMap<String, String> {
        let mut labels = self.labels.clone();
        labels.insert("env".to_owned(), self.environment.clone());
        labels
    }
}

/// What a non-cluster resource is.
///
/// A closed set on purpose: each variant is an icon and a word in the UI, and
/// `Other` is the honest home for anything this list has not learned yet
/// rather than a reason to accept free text that renders as nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// A Confluent-compatible schema registry — Apicurio's `ccompat`, or
    /// Confluent's own.
    SchemaRegistry,
    /// An MQTT broker.
    MqttBroker,
    /// A Kafka Connect cluster.
    KafkaConnect,
    /// A REST proxy in front of a cluster.
    RestProxy,
    /// Anything else worth listing beside the brokers.
    Other,
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
        let mut config: Config = Figment::new()
            .merge(Yaml::file_exact(path))
            .merge(
                Env::prefixed("KAAS_UI_")
                    .split("__")
                    .filter(|key| is_ours(key.as_str())),
            )
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))?;
        config.apply_defaults();
        config.validate()?;
        Ok(config)
    }

    /// Parse from a YAML string. The environment is not consulted.
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let mut config: Config = Figment::new()
            .merge(Yaml::string(yaml))
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))?;
        config.apply_defaults();
        config.validate()?;
        Ok(config)
    }

    /// Fill in what one block implies about another.
    ///
    /// Only one thing so far, and it exists because forgetting it was a total
    /// outage: a deployment that proxies a Dex talks to *that* Dex. See
    /// [`OidcConfig::default_internal_url_from`] for why this is a default
    /// rather than a required field or an inference from the public URL.
    fn apply_defaults(&mut self) {
        if let (Some(dex), Some(auth)) = (self.dex.as_ref(), self.auth.as_mut()) {
            auth.default_internal_url_from(&dex.upstream);
        }
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

        let mut environments: BTreeMap<&str, ()> = BTreeMap::new();
        for environment in &self.environments {
            if environment.id.is_empty() {
                return Err(ConfigError::Invalid(
                    "an environment has an empty id: it is what a cluster's `env` label names"
                        .into(),
                ));
            }
            if environments.insert(environment.id.as_str(), ()).is_some() {
                return Err(ConfigError::Invalid(format!(
                    "duplicate environment id {:?}",
                    environment.id
                )));
            }
        }

        // An environment is real if it was declared or if a cluster is in it.
        // Anything else a resource names is a typo, and the whole point of
        // saying so here is that the alternative is silent: a lonely section,
        // rendered like every other one, at the bottom of the fleet.
        let mut inhabited: BTreeMap<&str, ()> = environments;
        for cluster in &self.clusters {
            if let Some(env) = cluster.labels.get("env") {
                inhabited.insert(env.as_str(), ());
            }
        }

        let mut seen_resources: BTreeMap<&str, ()> = BTreeMap::new();
        for resource in &self.resources {
            if resource.id.is_empty() {
                return Err(ConfigError::Invalid("a resource has an empty id".into()));
            }
            if seen_resources.insert(resource.id.as_str(), ()).is_some() {
                return Err(ConfigError::Invalid(format!(
                    "duplicate resource id {:?}",
                    resource.id
                )));
            }
            if resource.environment.is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "resource {:?} names no environment: there is no section to put it in",
                    resource.id
                )));
            }
            if !inhabited.contains_key(resource.environment.as_str()) {
                return Err(ConfigError::Invalid(format!(
                    "resource {:?} is in environment {:?}, which no cluster labels `env: {}` and \
                     no `environments:` entry declares — declare it there if the environment holds \
                     no Kafka cluster",
                    resource.id, resource.environment, resource.environment
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
    fn environments_and_resources_parse() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    name: Development
  - id: prod

clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    labels: { env: dev }

resources:
  - id: apicurio-dev
    kind: schema_registry
    environment: dev
    endpoint: http://apicurio:8080/apis/ccompat/v7
  - id: mosquitto
    kind: mqtt_broker
    environment: prod
"#,
        )
        .unwrap();

        assert_eq!(config.environments[0].display_name(), "Development");
        // Undeclared name falls back to the id, so a section always has a
        // heading.
        assert_eq!(config.environments[1].display_name(), "prod");
        assert_eq!(config.resources[0].kind, ResourceKind::SchemaRegistry);
        // `env` is derived, never stored: it cannot disagree with the section.
        assert_eq!(
            config.resources[1]
                .effective_labels()
                .get("env")
                .map(String::as_str),
            Some("prod")
        );
    }

    #[test]
    fn a_resource_in_an_environment_nobody_declared_is_rejected() {
        // The typo this exists for. Without the check it renders as a section
        // of one at the bottom of the fleet, looking exactly like a real
        // environment nobody has any clusters in.
        let err = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    labels: { env: dev }

resources:
  - id: apicurio
    kind: schema_registry
    environment: dve
"#,
        )
        .unwrap_err();
        assert!(
            format!("{err}").contains("no `environments:` entry declares"),
            "{err}"
        );
    }

    #[test]
    fn an_environment_of_resources_alone_is_legal_once_declared() {
        // Declaring it is the opt-in: an environment with no Kafka cluster in
        // it is a real thing to want, and saying so out loud is what separates
        // it from the typo above.
        let config = Config::from_yaml(
            r#"
environments:
  - id: edge

clusters:
  - id: kaas
    bootstrap: ["a:9092"]
    labels: { env: dev }

resources:
  - id: mosquitto
    kind: mqtt_broker
    environment: edge
"#,
        )
        .unwrap();
        assert_eq!(config.resources.len(), 1);
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
    fn proxying_a_dex_means_talking_to_that_dex() {
        // The configuration that could not cold-start until this default
        // existed: discovery went to a hostname the tunnel routes back to this
        // process, which is not listening yet. It survived eleven releases
        // because a rolling deploy always left the previous pod up to answer.
        //
        // Nothing has to be remembered now. Configuring `dex` is the statement
        // that there is a local Dex, and the one this deployment proxies is the
        // one it talks to.
        let config = Config::from_yaml(PROXIED_DEX).unwrap();
        assert_eq!(
            config.auth.as_ref().unwrap().internal_url.as_deref(),
            Some("http://dex.dex.svc.cluster.local:5556/dex"),
            "the issuer's path is appended, because kaas-ui lets Dex live under one"
        );
    }

    #[test]
    fn an_explicit_internal_url_wins() {
        let yaml = format!("{PROXIED_DEX}  internal_url: http://somewhere.else:5556/dex\n");
        let config = Config::from_yaml(&yaml).unwrap();
        assert_eq!(
            config.auth.as_ref().unwrap().internal_url.as_deref(),
            Some("http://somewhere.else:5556/dex"),
            "a default that overrode what was written down would be worse than no default"
        );
    }

    #[test]
    fn an_issuer_at_the_root_does_not_gain_a_trailing_slash() {
        // `Url::path()` is "/" for an issuer with no path, and appending that
        // verbatim yields `…:5556/`, whose discovery URL is `…:5556//.well-known`.
        let yaml = PROXIED_DEX.replace(
            "issuer: https://kaas.smeding.cloud/dex",
            "issuer: https://kaas.smeding.cloud",
        );
        let config = Config::from_yaml(&yaml).unwrap();
        assert_eq!(
            config.auth.as_ref().unwrap().internal_url.as_deref(),
            Some("http://dex.dex.svc.cluster.local:5556")
        );
    }

    #[test]
    fn an_external_provider_is_left_alone() {
        // No `dex` block: nothing is proxied here, so there is no local Dex to
        // assume anything about. This is the case that made deriving from the
        // public URL the wrong shape — a deployment authenticating against
        // somebody else\'s IdP must not be pointed at a Dex that is not theirs.
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
        assert_eq!(config.auth.as_ref().unwrap().internal_url, None);
    }
}

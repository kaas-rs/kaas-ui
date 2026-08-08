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
use kaas_ui_serde::{Codec, RegistryAuth, RegistrySettings};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Everything kaas-ui reads at startup.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct Config {
    /// How the HTTP server binds.
    pub server: ServerConfig,
    /// The fleet: every environment, and everything in one.
    ///
    /// The only place infrastructure is declared. An environment holds Kafka
    /// clusters, the schema registries they resolve against, and the inventory
    /// beside them — and it holds them *structurally*, so membership is where
    /// a block sits rather than a label that has to agree with a list
    /// somewhere else. Declaration order is display order, because "dev,
    /// staging, prod" is an order nobody can derive from the strings.
    pub environments: Vec<EnvironmentEntry>,
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
    /// Free-form labels. `kind` groups the fleet view.
    ///
    /// **`env` is not one of them.** It used to be how a cluster joined an
    /// environment; the nesting is that now, and setting it here is rejected
    /// rather than merged, because a label that disagrees with the block it
    /// sits in is a hole in the `cluster_labels: {env: prod}` selector people
    /// write first. Read [`Self::effective_labels`] instead of this field.
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
    /// Which declared schema registry this cluster's payloads resolve against.
    ///
    /// Absent is a normal path, not a degraded one: a kaas instance with no
    /// registry and a Strimzi cluster with Apicurio coexist in one
    /// environment, and every code path that touches a registry has to have
    /// an absent branch anyway.
    ///
    /// Naming a registry that was not declared is a **startup error** with the
    /// id in it, because the alternative is a deployment that starts
    /// successfully with decoding silently off.
    #[serde(default)]
    pub schema_registry: Option<String>,
    /// Per-topic codec overrides, in configuration.
    ///
    /// The registry is shared across an environment; the decision that
    /// `orders` on `strimzi` is JSON is not. This is the configured half of
    /// the override — the other half is the chip in the message list, which
    /// is a query parameter and wins over anything here.
    #[serde(default)]
    pub codecs: Vec<TopicCodec>,
}

impl ClusterEntry {
    /// The labels a visibility check sees: the configured ones, plus `env`.
    ///
    /// The counterpart of [`ResourceEntry::effective_labels`], and it exists
    /// for the same reason: every role selector in the wild keys on `env`, and
    /// the one place that can still be wrong about it is a reader that forgot
    /// to add it. There is exactly one such place now.
    #[must_use]
    pub fn effective_labels(&self, environment: &str) -> BTreeMap<String, String> {
        let mut labels = self.labels.clone();
        labels.insert("env".to_owned(), environment.to_owned());
        labels
    }
}

/// One declared schema registry.
///
/// Confluent-compatible only. Apicurio's native `/apis/registry/v3` is not
/// supported and that is a decision rather than a gap: one wire format and one
/// client is what buys three formats for one integration. A `url` pointing at
/// the native API is reported on first use as a configuration error naming the
/// endpoint that was expected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SchemaRegistryEntry {
    /// Stable identifier, unique **within its environment**. What a cluster's
    /// `schema_registry:` names, and what every decoded payload reports as
    /// the registry that answered.
    ///
    /// It appears in a URL — `/api/environments/{env}/schema-registries/{id}`
    /// — so it may not contain anything that would have to be escaped in a
    /// path segment. Scoped rather than global: two environments may each hold
    /// an `apicurio`, and neither can be reached without naming an environment
    /// the caller can already see.
    pub id: String,
    /// Human-readable name. Defaults to the id.
    #[serde(default)]
    pub name: Option<String>,
    /// The ccompat base url, as in
    /// `http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7`.
    pub url: String,
    /// Basic-auth principal. On Confluent Cloud this is the API key.
    #[serde(default)]
    pub username: Option<String>,
    /// Basic-auth secret, inline. Prefer `password_file`.
    #[serde(default)]
    pub password: Option<String>,
    /// Basic-auth secret read from a file — a mounted Secret key, usually.
    #[serde(default)]
    pub password_file: Option<PathBuf>,
    /// A bearer token, sent verbatim. Mutually exclusive with basic auth.
    #[serde(default)]
    pub bearer_token: Option<String>,
    /// A bearer token read from a file.
    #[serde(default)]
    pub bearer_token_file: Option<PathBuf>,
    /// Per-call timeout. A schema fetch sits on a request path.
    #[serde(default, with = "humantime_serde::option")]
    pub request_timeout: Option<Duration>,
    /// How long a subject listing is believed.
    ///
    /// Schemas are immutable and cached by id forever; *listings* are not, so
    /// a subject registered a moment ago has to appear without a restart.
    #[serde(default, with = "humantime_serde::option")]
    pub subjects_ttl: Option<Duration>,
}

impl SchemaRegistryEntry {
    /// The name to render.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Turn the configured entry into the client's settings.
    ///
    /// Reads whatever was pointed at a file. A Secret that is not mounted
    /// where the config says it is fails here, at startup, rather than on the
    /// first record of the first Avro topic somebody opens.
    pub fn to_settings(&self) -> Result<RegistrySettings, ConfigError> {
        let mut settings = RegistrySettings::new(self.id.clone(), self.url.clone())
            .with_name(self.display_name().to_owned());

        if let Some(timeout) = self.request_timeout {
            settings = settings.with_request_timeout(timeout);
        }
        if let Some(ttl) = self.subjects_ttl {
            settings = settings.with_subjects_ttl(ttl);
        }

        let read = |path: &PathBuf| -> Result<String, ConfigError> {
            std::fs::read_to_string(path)
                .map(|text| text.trim().to_owned())
                // The path is the whole diagnosis when a Secret is not mounted
                // where the config says it is.
                .map_err(|source| {
                    ConfigError::Invalid(format!(
                        "schema registry {:?}: {}: {source}",
                        self.id,
                        path.display()
                    ))
                })
        };

        if let Some(username) = &self.username {
            let password = match (&self.password, &self.password_file) {
                (Some(inline), _) => Some(inline.clone()),
                (None, Some(path)) => Some(read(path)?),
                (None, None) => None,
            };
            settings = settings.with_auth(RegistryAuth::Basic {
                username: username.clone(),
                password,
            });
        } else if let Some(token) = &self.bearer_token {
            settings = settings.with_auth(RegistryAuth::Bearer(token.clone()));
        } else if let Some(path) = &self.bearer_token_file {
            settings = settings.with_auth(RegistryAuth::Bearer(read(path)?));
        }

        Ok(settings)
    }
}

/// A configured codec override for one topic.
///
/// The override is only free in one direction. `hex` and `string` need no
/// schema and are always honoured; naming a registry-backed codec here cannot
/// invent a schema id for a payload that carries none, and is refused per
/// record with a reason rather than silently producing nothing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TopicCodec {
    /// The topic name, or a prefix ending in `*`.
    pub topic: String,
    /// How to read keys. Absent leaves keys alone.
    #[serde(default)]
    pub key: Option<Codec>,
    /// How to read values. Absent leaves values alone.
    #[serde(default)]
    pub value: Option<Codec>,
}

impl TopicCodec {
    /// Whether this entry covers `topic`.
    #[must_use]
    pub fn matches(&self, topic: &str) -> bool {
        match self.topic.strip_suffix('*') {
            Some(prefix) => topic.starts_with(prefix),
            None => self.topic == topic,
        }
    }
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
    /// Stable identifier. It leads every URL under `/api/environments`, so it
    /// may not contain anything that would have to be escaped in a path
    /// segment.
    pub id: String,
    /// Human-readable name. Defaults to the id.
    #[serde(default)]
    pub name: Option<String>,
    /// One line under the heading, where the id does not say enough.
    #[serde(default)]
    pub description: Option<String>,
    /// The Kafka clusters in this environment.
    #[serde(default)]
    pub kafka_clusters: Vec<ClusterEntry>,
    /// The schema registries in this environment.
    ///
    /// A registry serves an **environment**: every cluster here that names one
    /// resolves schema id 42 to the same schema, because it is the same
    /// registry answering. Nesting is membership; a cluster still names the
    /// one it uses with [`ClusterEntry::schema_registry`], because a cluster
    /// sitting beside a registry need not decode against it — and a cluster
    /// that should render hex must be able to say so.
    #[serde(default)]
    pub schema_registries: Vec<SchemaRegistryEntry>,
    /// Everything else here that kaas-ui does not dial: Connect, an MQTT
    /// broker, a REST proxy. Inventory, so that "what is in staging" has one
    /// answer and it is not only the brokers.
    #[serde(default)]
    pub resources: Vec<ResourceEntry>,
}

impl EnvironmentEntry {
    /// The name to render.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Whether anything at all is declared in here.
    ///
    /// An environment with nothing in it is legal — someone is about to fill
    /// it — but it renders as a heading with no content, so callers that
    /// arrange the fleet drop it rather than showing an empty section.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kafka_clusters.is_empty()
            && self.schema_registries.is_empty()
            && self.resources.is_empty()
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
    /// Where it is, as an address a human can act on.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// One line of context on the card.
    #[serde(default)]
    pub note: Option<String>,
    /// Free-form labels, as on a cluster.
    ///
    /// `env` is added from where this entry sits before anything reads these,
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
    /// Built from the nesting rather than stored, so the two can never
    /// disagree — a resource inside `prod` labelled `env: dev` would be a hole
    /// in exactly the selector people write first. Since the environment is
    /// now structural there is nothing to keep in step, which is the point.
    #[must_use]
    pub fn effective_labels(&self, environment: &str) -> BTreeMap<String, String> {
        let mut labels = self.labels.clone();
        labels.insert("env".to_owned(), environment.to_owned());
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

/// Whether an id can be a path segment verbatim.
///
/// Environment, cluster and registry ids all appear in URLs now, so they all
/// answer to this. Deliberately narrower than percent-encoding would require:
/// an id that needs escaping is one that will be copied into a curl command
/// wrong, and refusing it at startup costs a rename once.
fn is_path_safe(id: &str) -> bool {
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The top-level blocks that moved inside `environments:`.
///
/// `deny_unknown_fields` already refuses them, but it refuses them with
/// "unknown field `clusters`", which reads as *misspelled* rather than
/// *moved*. Anyone upgrading has a working config full of these, and the one
/// thing they need told is where the block went.
const MOVED_BLOCKS: [(&str, &str); 3] = [
    ("clusters", "kafka_clusters"),
    ("schema_registries", "schema_registries"),
    ("resources", "resources"),
];

/// Say what to do about a pre-nesting config, before serde says something
/// less useful about it.
fn migration_error(yaml: &str) -> Option<ConfigError> {
    let moved: Vec<&(&str, &str)> = MOVED_BLOCKS
        .iter()
        .filter(|(old, _)| {
            yaml.lines().any(|line| {
                line.trim_end() == format!("{old}:") || line.starts_with(&format!("{old}:"))
            })
        })
        .collect();
    if moved.is_empty() {
        return None;
    }
    let moves = moved
        .iter()
        .map(|(old, new)| format!("`{old}:` -> `environments[].{new}:`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(ConfigError::Invalid(format!(
        "this configuration is in the pre-nesting layout: {moves}. Everything now lives inside \
         the environment that holds it, and a cluster's `env` label is gone — it is in an \
         environment because it is declared there."
    )))
}

impl Config {
    /// Load from a YAML file, overlaid with `KAAS_UI_*` environment variables.
    ///
    /// The overlay uses `__` as the nesting separator, so
    /// `KAAS_UI_SERVER__LISTEN=0.0.0.0:9000` sets `server.listen`, and
    /// `KAAS_UI_SERVER__BASE_PATH=/proxy/8099` sets the path prefix.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if let Ok(yaml) = std::fs::read_to_string(path)
            && let Some(error) = migration_error(&yaml)
        {
            return Err(error);
        }
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
        if let Some(error) = migration_error(yaml) {
            return Err(error);
        }
        let mut config: Config = Figment::new()
            .merge(Yaml::string(yaml))
            .extract()
            .map_err(|e| ConfigError::Load(Box::new(e)))?;
        config.apply_defaults();
        config.validate()?;
        Ok(config)
    }

    /// Every cluster in the fleet, paired with the environment holding it.
    ///
    /// The nesting is the source of truth, so nothing caches a flattened copy
    /// beside it — readers that genuinely want the whole fleet walk it through
    /// here and get the environment id they need for a key or a label without
    /// having to remember to.
    pub fn clusters(&self) -> impl Iterator<Item = (&str, &ClusterEntry)> {
        self.environments.iter().flat_map(|environment| {
            environment
                .kafka_clusters
                .iter()
                .map(move |cluster| (environment.id.as_str(), cluster))
        })
    }

    /// Every schema registry in the fleet, paired with its environment.
    pub fn schema_registries(&self) -> impl Iterator<Item = (&str, &SchemaRegistryEntry)> {
        self.environments.iter().flat_map(|environment| {
            environment
                .schema_registries
                .iter()
                .map(move |registry| (environment.id.as_str(), registry))
        })
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
        if self.environments.is_empty() {
            return Err(ConfigError::Invalid(
                "no environments configured: every cluster, registry and resource lives inside \
                 one, so a fleet without them has nothing to show"
                    .into(),
            ));
        }

        let mut seen_environments: BTreeMap<&str, ()> = BTreeMap::new();
        let mut cluster_count = 0usize;

        for environment in &self.environments {
            let env = environment.id.as_str();
            if env.is_empty() {
                return Err(ConfigError::Invalid(
                    "an environment has an empty id: it leads every URL under \
                     `/api/environments`"
                        .into(),
                ));
            }
            if !is_path_safe(env) {
                return Err(ConfigError::Invalid(format!(
                    "environment id {env:?} may only contain letters, digits, '-' and '_': it \
                     appears verbatim in every URL"
                )));
            }
            if seen_environments.insert(env, ()).is_some() {
                return Err(ConfigError::Invalid(format!(
                    "duplicate environment id {env:?}"
                )));
            }

            // Registries first: the clusters below reference them, and an
            // unresolvable reference is the error worth reporting rather than
            // whatever the reference happened to be checked against.
            let mut registries: BTreeMap<&str, ()> = BTreeMap::new();
            for registry in &environment.schema_registries {
                if registry.id.is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "a schema registry in environment {env:?} has an empty id: it is what a \
                         cluster's `schema_registry` names"
                    )));
                }
                if !is_path_safe(&registry.id) {
                    return Err(ConfigError::Invalid(format!(
                        "schema registry id {:?} in environment {env:?} may only contain letters, \
                         digits, '-' and '_': it appears verbatim in every URL",
                        registry.id
                    )));
                }
                if registries.insert(registry.id.as_str(), ()).is_some() {
                    return Err(ConfigError::Invalid(format!(
                        "duplicate schema registry id {:?} in environment {env:?}",
                        registry.id
                    )));
                }
                if !registry.url.starts_with("http://") && !registry.url.starts_with("https://") {
                    return Err(ConfigError::Invalid(format!(
                        "schema registry {:?} in environment {env:?} has url {:?}, which is not an \
                         http(s) url",
                        registry.id, registry.url
                    )));
                }
                // Whether the url is *ccompat* cannot be settled here — it takes
                // asking the registry, and connecting is lazy. What can be settled
                // here is the shape of the credentials.
                if registry.username.is_none()
                    && (registry.password.is_some() || registry.password_file.is_some())
                {
                    return Err(ConfigError::Invalid(format!(
                        "schema registry {:?} in environment {env:?} configures a password with no \
                         username",
                        registry.id
                    )));
                }
                if registry.username.is_some()
                    && (registry.bearer_token.is_some() || registry.bearer_token_file.is_some())
                {
                    return Err(ConfigError::Invalid(format!(
                        "schema registry {:?} in environment {env:?} configures both basic auth \
                         and a bearer token: only one of them can be sent",
                        registry.id
                    )));
                }
            }

            let mut clusters: BTreeMap<&str, ()> = BTreeMap::new();
            for cluster in &environment.kafka_clusters {
                cluster_count += 1;
                if cluster.id.is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "a cluster in environment {env:?} has an empty id"
                    )));
                }
                if !is_path_safe(&cluster.id) {
                    return Err(ConfigError::Invalid(format!(
                        "cluster id {:?} may only contain letters, digits, '-' and '_': it \
                         appears verbatim in every URL",
                        cluster.id
                    )));
                }
                // Within the environment, not across the fleet. Two teams may
                // each call their cluster `kafka`, and now that an id is only
                // reachable under an environment there is nothing to collide.
                if clusters.insert(cluster.id.as_str(), ()).is_some() {
                    return Err(ConfigError::Invalid(format!(
                        "duplicate cluster id {:?} in environment {env:?}",
                        cluster.id
                    )));
                }
                // The nesting is the membership. A label saying otherwise is
                // not merged and not ignored: it is the one input that could
                // put a cluster in `prod` outside a `cluster_labels:
                // {env: prod}` selector, silently.
                if cluster.labels.contains_key("env") {
                    return Err(ConfigError::Invalid(format!(
                        "cluster {:?} sets an `env` label, which is no longer how a cluster joins \
                         an environment — it is in {env:?} because it is declared there. Remove \
                         the label.",
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
                // The reference has to name something *in this environment*.
                // Starting with decoding silently off is the failure this
                // exists to prevent, and it is invisible: every Avro topic
                // renders as hex and nothing says why.
                if let Some(registry) = &cluster.schema_registry
                    && !registries.contains_key(registry.as_str())
                {
                    return Err(ConfigError::Invalid(format!(
                        "cluster {:?} references schema registry {:?}, which environment {env:?} \
                         does not declare{}",
                        cluster.id,
                        registry,
                        if registries.is_empty() {
                            " — it declares none".to_owned()
                        } else {
                            format!(
                                " (it declares: {})",
                                registries
                                    .keys()
                                    .map(|id| format!("{id:?}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        }
                    )));
                }
                for codec in &cluster.codecs {
                    if codec.topic.is_empty() {
                        return Err(ConfigError::Invalid(format!(
                            "cluster {:?} has a `codecs` entry with an empty topic",
                            cluster.id
                        )));
                    }
                    if codec.key.is_none() && codec.value.is_none() {
                        return Err(ConfigError::Invalid(format!(
                            "cluster {:?} has a `codecs` entry for {:?} that sets neither `key` \
                             nor `value`",
                            cluster.id, codec.topic
                        )));
                    }
                }
            }

            let mut resources: BTreeMap<&str, ()> = BTreeMap::new();
            for resource in &environment.resources {
                if resource.id.is_empty() {
                    return Err(ConfigError::Invalid(format!(
                        "a resource in environment {env:?} has an empty id"
                    )));
                }
                if resources.insert(resource.id.as_str(), ()).is_some() {
                    return Err(ConfigError::Invalid(format!(
                        "duplicate resource id {:?} in environment {env:?}",
                        resource.id
                    )));
                }
            }
        }

        if cluster_count == 0 {
            return Err(ConfigError::Invalid(
                "no Kafka clusters configured: every environment declares only registries or \
                 inventory, so there is nothing to connect to"
                    .into(),
            ));
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
    for root in ["server", "environments"] {
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

    /// The smallest thing that loads: one environment, one cluster in it.
    ///
    /// Every fixture here starts this way now. That is the point of the shape
    /// — there is no top level to declare a cluster at, so a config cannot be
    /// written that leaves one homeless.
    const MINIMAL: &str = r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#;

    /// The first cluster, for tests that declared exactly one.
    fn only_cluster(config: &Config) -> &ClusterEntry {
        config.clusters().next().expect("a cluster").1
    }

    #[test]
    fn minimal_config_parses_and_defaults() {
        let config = Config::from_yaml(MINIMAL).unwrap();
        assert_eq!(config.clusters().count(), 1);
        assert_eq!(config.server.listen.port(), 8080);
        assert_eq!(only_cluster(&config).display_name(), "kaas");
    }

    #[test]
    fn a_cluster_is_in_the_environment_that_holds_it() {
        let config = Config::from_yaml(MINIMAL).unwrap();
        let (environment, cluster) = config.clusters().next().unwrap();
        assert_eq!(environment, "dev");
        // Derived, never stored: the label and the block it sits in cannot
        // disagree, because there is only one of them.
        assert_eq!(
            cluster.effective_labels(environment).get("env"),
            Some(&"dev".to_owned())
        );
    }

    #[test]
    fn an_env_label_is_rejected_because_nesting_decides_it() {
        // The one input that could have put a cluster in `prod` outside a
        // `cluster_labels: {env: prod}` selector. Merging it silently is how
        // that hole gets shipped; refusing it costs a deleted line.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        labels: { env: prod }
"#,
        )
        .unwrap_err();
        assert!(
            format!("{err}").contains("no longer how a cluster joins"),
            "{err}"
        );
    }

    #[test]
    fn the_same_id_in_two_environments_is_two_clusters() {
        // What the nesting buys. `kafka` in dev and `kafka` in prod collided
        // in one flat namespace; now each is reachable only under its own
        // environment and neither has to be renamed.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kafka
        bootstrap: ["dev:9092"]
  - id: prod
    kafka_clusters:
      - id: kafka
        bootstrap: ["prod:9092"]
"#,
        )
        .unwrap();
        let found: Vec<(&str, &str)> = config
            .clusters()
            .map(|(environment, cluster)| (environment, cluster.id.as_str()))
            .collect();
        assert_eq!(found, vec![("dev", "kafka"), ("prod", "kafka")]);
    }

    #[test]
    fn a_duplicate_id_within_one_environment_is_still_rejected() {
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
      - id: kaas
        bootstrap: ["b:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("duplicate cluster id"), "{err}");
    }

    #[test]
    fn a_pre_nesting_config_says_where_the_blocks_went() {
        // Anyone upgrading has a working config full of top-level `clusters:`.
        // `deny_unknown_fields` would call that a misspelling; it is a move,
        // and the one thing worth being told is the destination.
        let err = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["a:9092"]
"#,
        )
        .unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("pre-nesting"), "{message}");
        assert!(
            message.contains("environments[].kafka_clusters"),
            "{message}"
        );
    }

    #[test]
    fn durations_are_human_readable() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        refresh_interval: 45s
        connect_timeout: 2500ms
"#,
        )
        .unwrap();
        assert_eq!(
            only_cluster(&config).refresh_interval,
            Some(Duration::from_secs(45))
        );
        assert_eq!(
            only_cluster(&config).connect_timeout,
            Some(Duration::from_millis(2500))
        );
    }

    #[test]
    fn an_unknown_block_is_rejected_rather_than_ignored() {
        // The whole point: someone writes a block the shape does not have, and
        // finds out at startup rather than by wondering why nothing happened.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
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
    fn named_connectors_parse_and_are_optional() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
auth:
  issuer: https://kaas.smeding.cloud/dex
  client_id: kaas-ui
  redirect_url: https://kaas.smeding.cloud/auth/callback
  connectors:
    - id: github
      name: GitHub
    - id: local
      name: Email
"#,
        )
        .unwrap();
        let connectors = &config.auth.as_ref().expect("an auth block").connectors;
        assert_eq!(connectors.len(), 2);
        assert_eq!(connectors[0].id, "github");

        let bare = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
auth:
  issuer: https://kaas.smeding.cloud/dex
  client_id: kaas-ui
  redirect_url: https://kaas.smeding.cloud/auth/callback
"#,
        )
        .unwrap();
        assert!(bare.auth.expect("an auth block").connectors.is_empty());
    }

    #[test]
    fn environments_and_resources_parse() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    name: Development
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
    resources:
      - id: apicurio-dev
        kind: schema_registry
        endpoint: http://apicurio:8080/apis/ccompat/v7
  - id: prod
    kafka_clusters:
      - id: prod-eu
        bootstrap: ["b:9092"]
    resources:
      - id: mosquitto
        kind: mqtt_broker
"#,
        )
        .unwrap();

        assert_eq!(config.environments[0].display_name(), "Development");
        // Undeclared name falls back to the id, so a section always has a
        // heading.
        assert_eq!(config.environments[1].display_name(), "prod");
        assert_eq!(
            config.environments[0].resources[0].kind,
            ResourceKind::SchemaRegistry
        );
        // `env` is derived from where the entry sits, never stored.
        assert_eq!(
            config.environments[1].resources[0]
                .effective_labels("prod")
                .get("env")
                .map(String::as_str),
            Some("prod")
        );
    }

    #[test]
    fn a_resource_cannot_name_an_environment_that_does_not_exist() {
        // This used to be a validation rule with an error message: a resource
        // named an environment, and a typo opened a section of one at the
        // bottom of the fleet looking exactly like a real environment. The
        // shape retired the rule — there is nowhere to write the typo, because
        // a resource has no environment field to get wrong.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
    resources:
      - id: apicurio
        kind: schema_registry
        environment: dve
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("environment"), "{err}");
    }

    #[test]
    fn an_environment_of_resources_alone_is_legal() {
        // An environment with no Kafka cluster in it is a real thing to want,
        // and it needs no opt-in now: writing the block is the declaration.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
  - id: edge
    resources:
      - id: mosquitto
        kind: mqtt_broker
"#,
        )
        .unwrap();
        assert_eq!(config.environments[1].resources.len(), 1);
        assert!(config.environments[1].kafka_clusters.is_empty());
        assert!(!config.environments[1].is_empty());
    }

    /// The shape the nesting is about: a registry beside the clusters that use
    /// it, referenced by id, and a cluster in the same environment using none.
    #[test]
    fn a_registry_sits_in_an_environment_and_is_referenced_by_id() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        name: Apicurio (dev)
        url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7
        subjects_ttl: 15s
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
        schema_registry: apicurio
      - id: kaas
        bootstrap: ["b:9092"]
"#,
        )
        .unwrap();

        assert_eq!(config.schema_registries().count(), 1);
        let (environment, registry) = config.schema_registries().next().unwrap();
        assert_eq!(environment, "dev");
        assert_eq!(registry.display_name(), "Apicurio (dev)");

        let clusters: Vec<&ClusterEntry> = config.clusters().map(|(_, c)| c).collect();
        assert_eq!(clusters[0].schema_registry.as_deref(), Some("apicurio"));
        // Reference, not membership: a cluster beside a registry need not
        // decode against it, and absence is a normal path.
        assert_eq!(clusters[1].schema_registry, None);

        let settings = registry.to_settings().unwrap();
        assert_eq!(settings.subjects_ttl, Duration::from_secs(15));
        assert_eq!(settings.name, "Apicurio (dev)");
    }

    #[test]
    fn a_reference_may_not_cross_an_environment_boundary() {
        // `dev`'s registry is not in scope from `prod`, however global the id
        // looks. Allowing it would make one environment's decoding depend on
        // another's configuration.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
  - id: prod
    kafka_clusters:
      - id: prod-eu
        bootstrap: ["b:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("\"apicurio\""), "{message}");
        assert!(message.contains("it declares none"), "{message}");
    }

    #[test]
    fn an_unknown_schema_registry_reference_fails_startup_with_the_id_named() {
        // The alternative is a deployment that starts successfully with
        // decoding silently off: every Avro topic renders as hex and nothing
        // anywhere says why.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
        schema_registry: apicuriio
"#,
        )
        .unwrap_err();
        let message = format!("{err}");
        assert!(message.contains("\"apicuriio\""), "{message}");
        assert!(message.contains("\"apicurio\""), "{message}");
    }

    #[test]
    fn a_registry_id_nobody_declared_at_all_says_there_are_none() {
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("it declares none"), "{err}");
    }

    #[test]
    fn half_a_credential_is_rejected() {
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
        password: hunter2
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("no username"), "{err}");

        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
        username: someone
        bearer_token: abc
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("only one of them"), "{err}");
    }

    #[test]
    fn a_registry_url_that_is_not_a_url_is_rejected_at_load() {
        // Whether it is *ccompat* takes asking, and asking is lazy. Whether it
        // is an http url does not.
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: apicurio-registry.apicurio.svc.cluster.local:8080
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("not an http(s) url"), "{err}");
    }

    #[test]
    fn a_topic_codec_matches_exactly_or_by_prefix() {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
        codecs:
          - topic: raw-bytes-v1
            value: hex
          - topic: "orders-*"
            key: string
            value: json
"#,
        )
        .unwrap();

        let codecs = &only_cluster(&config).codecs;
        assert!(codecs[0].matches("raw-bytes-v1"));
        assert!(!codecs[0].matches("raw-bytes-v10"));
        assert!(codecs[1].matches("orders-eu"));
        assert!(!codecs[1].matches("shipments"));
        assert_eq!(codecs[0].value, Some(Codec::Hex));
        assert_eq!(codecs[1].key, Some(Codec::String));
    }

    #[test]
    fn a_codec_entry_that_sets_nothing_is_rejected() {
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        bootstrap: ["a:9092"]
        codecs:
          - topic: orders
"#,
        )
        .unwrap_err();
        assert!(
            format!("{err}").contains("neither `key` nor `value`"),
            "{err}"
        );
    }

    #[test]
    fn an_id_that_would_need_escaping_is_rejected() {
        for yaml in [
            r#"
environments:
  - id: "dev/eu"
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
"#,
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: "prod/eu"
        bootstrap: ["a:9092"]
"#,
            r#"
environments:
  - id: dev
    schema_registries:
      - id: "a/b"
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
"#,
        ] {
            let err = Config::from_yaml(yaml).unwrap_err();
            assert!(format!("{err}").contains("every URL"), "{err}");
        }
    }

    #[test]
    fn a_fleet_with_no_environments_is_rejected() {
        let err = Config::from_yaml("environments: []").unwrap_err();
        assert!(format!("{err}").contains("no environments"), "{err}");
    }

    #[test]
    fn an_environment_holding_no_cluster_at_all_is_rejected() {
        // Registries and inventory are things to look at, not things to
        // connect to. A fleet of nothing but them has no brokers in it.
        let err = Config::from_yaml(
            r#"
environments:
  - id: edge
    resources:
      - id: mosquitto
        kind: mqtt_broker
"#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("no Kafka clusters"), "{err}");
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

        for ours in ["SERVER__LISTEN", "server__listen", "ENVIRONMENTS", "SERVER"] {
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
environments:
  - id: dev
    kafka_clusters:
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
            assert_eq!(config.clusters().count(), 1);
            Ok(())
        });
    }

    #[test]
    fn half_a_client_certificate_is_rejected() {
        let err = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
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
environments:
  - id: dev
    kafka_clusters:
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
        // somebody else's IdP must not be pointed at a Dex that is not theirs.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
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

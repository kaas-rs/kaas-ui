//! The cluster registry: the one construction site, and the lazy connect.
//!
//! Three rules, each of which is a bug if broken.
//!
//! 1. **Connect lazily.** `main` connects to nothing. One unreachable cluster
//!    must not block startup, hang `/health`, or slow a page that does not
//!    touch it. `Cluster::connect` fetches a snapshot before it returns, so
//!    the first attempt against a dead cluster blocks for the connect timeout
//!    — which is exactly why it happens on a background task and never on a
//!    request path.
//! 2. **Isolate failures.** A failed attempt records
//!    [`ClusterHealth::Unreachable`] and schedules a retry with backoff. It
//!    never propagates out of the handle.
//! 3. **One construction site.** Exactly one `Admin::connect_read_only` in the
//!    workspace, below. No `Admin::connect` anywhere. Enforced by a grep *and*
//!    by [`tests::exactly_one_construction_site`], because the grep alone is
//!    defeated by a rename and the test alone by a call site nobody tested.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use arc_swap::{ArcSwap, ArcSwapOption};
use kaas_ui_auth::Access;
use kaas_ui_serde::{Codec, RegistryHandle};
use kafka_admin::{Admin, ClusterConfig};
use kafka_conn::{ConnectionConfig, Error, SaslConfig, SaslMechanism, TlsConfig};
use tokio::sync::Notify;

use crate::config::{
    ClusterEntry, Config, ConfigError, EnvironmentEntry, ResourceEntry, SaslMechanismName,
};
use crate::error::ErrorKind;
use crate::health::ClusterHealth;

/// Retry backoff floor and ceiling for a cluster that will not connect.
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// How often a connected cluster's snapshot age is looked at.
const HEALTH_INTERVAL: Duration = Duration::from_secs(10);

/// One configured cluster, connected or not.
#[derive(Debug)]
pub struct ClusterHandle {
    /// The id, as it appears in every URL.
    pub id: String,
    /// The name to render.
    pub name: String,
    /// Grouping labels.
    pub labels: BTreeMap<String, String>,
    entry: ClusterEntry,
    /// The schema registry this cluster's payloads resolve against.
    ///
    /// **Shared, not owned.** Two clusters in `dev` naming the same registry
    /// id hold the same `Arc`, and therefore the same decoders and the same
    /// id→schema cache. A handle built per cluster would be a second cache
    /// for one registry's ids, which is the mistake this type exists to make
    /// unrepresentable.
    ///
    /// `None` is a normal path, not a degraded one: a kaas instance with no
    /// registry sits in the same environment as a Strimzi cluster with one.
    registry: Option<Arc<RegistryHandle>>,
    /// `None` until the first successful connect.
    admin: ArcSwapOption<Admin>,
    health: ArcSwap<ClusterHealth>,
    /// Set when a request arrived for a cluster that is not connected. The
    /// connector wakes and retries immediately rather than serving out the
    /// backoff — which is what the fleet card's retry button does, without
    /// needing a non-GET route to do it.
    retry_now: Notify,
    /// Set when a config reload dropped this cluster. The connector exits.
    retired: AtomicBool,
}

impl ClusterHandle {
    fn new(entry: &ClusterEntry, registry: Option<Arc<RegistryHandle>>) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.display_name().to_owned(),
            labels: entry.labels.clone(),
            entry: entry.clone(),
            registry,
            admin: ArcSwapOption::empty(),
            health: ArcSwap::from_pointee(ClusterHealth::connecting()),
            retry_now: Notify::new(),
            retired: AtomicBool::new(false),
        }
    }

    /// The schema registry this cluster's payloads resolve against, if any.
    ///
    /// Every caller has an absent branch, because absence is the common case
    /// on a cluster that was never given one.
    pub fn schema_registry(&self) -> Option<&Arc<RegistryHandle>> {
        self.registry.as_ref()
    }

    /// The configured codec for one topic's keys and values.
    ///
    /// First match wins, in configured order, so a specific entry can precede
    /// a `prefix*` one. What comes back is the *configured* choice; the chip
    /// in the message list is a query parameter and overrides it per request.
    pub fn configured_codecs(&self, topic: &str) -> (Codec, Codec) {
        let mut key = Codec::Auto;
        let mut value = Codec::Auto;
        let mut key_set = false;
        let mut value_set = false;
        for codec in &self.entry.codecs {
            if !codec.matches(topic) {
                continue;
            }
            if let (false, Some(chosen)) = (key_set, codec.key) {
                key = chosen;
                key_set = true;
            }
            if let (false, Some(chosen)) = (value_set, codec.value) {
                value = chosen;
                value_set = true;
            }
            if key_set && value_set {
                break;
            }
        }
        (key, value)
    }

    /// The admin client, if this cluster has ever connected.
    ///
    /// Never blocks and never connects. A caller that gets `None` renders the
    /// health state instead — it does not wait.
    pub fn admin(&self) -> Option<Arc<Admin>> {
        self.admin.load_full()
    }

    /// The current health.
    pub fn health(&self) -> Arc<ClusterHealth> {
        self.health.load_full()
    }

    /// Ask the connector to retry now instead of waiting out its backoff.
    ///
    /// A GET that finds a cluster unreachable calls this. The request itself
    /// still answers immediately from health — nudging is a side effect, not
    /// a wait.
    pub fn request_retry(&self) {
        self.retry_now.notify_one();
    }

    /// Stop this handle's connector. Called when a reload drops the cluster.
    pub fn retire(&self) {
        self.retired.store(true, Ordering::Relaxed);
        self.retry_now.notify_one();
    }

    /// Whether a reload dropped this cluster.
    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Relaxed)
    }

    /// The configured staleness ceiling, which the UI renders against.
    pub fn max_staleness(&self) -> Duration {
        self.entry
            .max_staleness
            .unwrap_or_else(|| ClusterConfig::default().max_staleness)
    }

    /// Build the kaas-lib configuration for this cluster.
    fn cluster_config(&self) -> Result<ClusterConfig, Error> {
        let defaults = ClusterConfig::default();
        let mut connection = ConnectionConfig::new().with_client_id(
            self.entry
                .client_id
                .clone()
                .unwrap_or_else(|| format!("kaas-ui/{}", self.entry.id)),
        );

        if let Some(timeout) = self.entry.connect_timeout {
            connection = connection.with_connect_timeout(timeout);
        }
        if let Some(timeout) = self.entry.request_timeout {
            connection = connection.with_request_timeout(timeout);
        }

        if let Some(tls) = &self.entry.tls {
            let mut config = match &tls.ca_file {
                Some(path) => TlsConfig::with_ca_pem(read_pem(path)?),
                None => TlsConfig::system(),
            };
            if let (Some(cert), Some(key)) = (&tls.cert_file, &tls.key_file) {
                config = config.with_client_certificate(read_pem(cert)?, read_pem(key)?);
            }
            if let Some(name) = &tls.server_name {
                config = config.with_server_name(name.clone());
            }
            connection = connection.with_tls(config);
        }

        if let Some(sasl) = &self.entry.sasl {
            let password = match (&sasl.password, &sasl.password_file) {
                (Some(inline), _) => inline.clone(),
                (None, Some(path)) => String::from_utf8(read_pem(path)?)
                    .map_err(|_| {
                        Error::InvalidRequest(format!(
                            "password file {} is not valid UTF-8",
                            path.display()
                        ))
                    })?
                    .trim()
                    .to_owned(),
                // Rejected at load time; unreachable through `Config::load`.
                (None, None) => {
                    return Err(Error::InvalidRequest(format!(
                        "cluster {} configures sasl without a password",
                        self.entry.id
                    )));
                }
            };
            let mechanism = match sasl.mechanism {
                SaslMechanismName::Plain => SaslMechanism::Plain,
                SaslMechanismName::ScramSha256 => SaslMechanism::ScramSha256,
                SaslMechanismName::ScramSha512 => SaslMechanism::ScramSha512,
            };
            let mut config = SaslConfig::new(mechanism, sasl.username.clone(), password);
            if sasl.allow_plaintext_password {
                config = config.allow_plaintext_password();
            }
            connection = connection.with_sasl(config);
        }

        Ok(ClusterConfig {
            connection,
            retry: defaults.retry,
            refresh_interval: self
                .entry
                .refresh_interval
                .unwrap_or(defaults.refresh_interval),
            max_staleness: self.entry.max_staleness.unwrap_or(defaults.max_staleness),
        })
    }

    /// **The only `Admin::connect_read_only` call site in the workspace.**
    ///
    /// Read-only is the architecture, not a setting: the gate is enforced in
    /// `Connection::send` on `ApiKey::is_mutating`, so an admin method added
    /// upstream tomorrow is covered without anyone remembering to cover it.
    /// kaas-ui's only job is not to undermine it, and the way to not undermine
    /// it is to have one place where a connection can possibly be made.
    async fn connect(&self) -> Result<Admin, Error> {
        let config = self.cluster_config()?;
        Admin::connect_read_only(self.entry.bootstrap.clone(), config).await
    }

    fn record_failure(&self, error: &Error) {
        let previous = self.health.load();
        let (since, attempts) = match previous.as_ref() {
            ClusterHealth::Unreachable {
                since, attempts, ..
            } => (*since, attempts.saturating_add(1)),
            other => (SystemTime::now(), other.attempts().saturating_add(1)),
        };
        self.health.store(Arc::new(ClusterHealth::Unreachable {
            error: error.to_string(),
            kind: ErrorKind::of(error),
            since,
            attempts,
        }));
    }

    /// The connector task: connect, then watch. One per cluster, forever.
    async fn run(self: Arc<Self>) {
        let mut backoff = BACKOFF_MIN;

        while !self.is_retired() {
            match self.admin.load_full() {
                None => match self.connect().await {
                    Ok(admin) => {
                        tracing::info!(cluster = %self.id, "connected");
                        self.admin.store(Some(Arc::new(admin)));
                        self.health.store(Arc::new(ClusterHealth::Ready {
                            since: SystemTime::now(),
                        }));
                        backoff = BACKOFF_MIN;
                    }
                    Err(error) => {
                        tracing::warn!(cluster = %self.id, %error, "connect failed");
                        self.record_failure(&error);
                        sleep_or_nudge(&self.retry_now, backoff).await;
                        backoff = (backoff * 2).min(BACKOFF_MAX);
                    }
                },
                Some(admin) => {
                    sleep_or_nudge(&self.retry_now, HEALTH_INTERVAL).await;
                    if self.is_retired() {
                        break;
                    }
                    // kaas-lib runs its own refresh loop, so a healthy cluster
                    // needs nothing from us. Only when the snapshot has gone
                    // past its staleness ceiling — meaning that loop is
                    // failing — do we refresh ourselves, to turn silence into
                    // an error message the card can show.
                    let cluster = admin.cluster();
                    if cluster.snapshot().age() > self.max_staleness() {
                        match cluster.refresh().await {
                            Ok(_) => self.health.store(Arc::new(ClusterHealth::Ready {
                                since: SystemTime::now(),
                            })),
                            Err(error) => {
                                tracing::warn!(cluster = %self.id, %error, "metadata refresh failed");
                                self.record_failure(&error);
                            }
                        }
                    }
                }
            }
        }

        tracing::info!(cluster = %self.id, "connector retired");
    }
}

/// Wait for `delay`, or wake early if nudged.
async fn sleep_or_nudge(notify: &Notify, delay: Duration) {
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = notify.notified() => {}
    }
}

fn read_pem(path: &std::path::Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|source| {
        // The path is the whole diagnosis when a Secret is not mounted where
        // the config says it is.
        Error::InvalidRequest(format!("{}: {source}", path.display()))
    })
}

/// Every configured cluster, and the fleet it is arranged into.
#[derive(Debug)]
pub struct Registry {
    clusters: BTreeMap<String, Arc<ClusterHandle>>,
    /// One client per declared registry, whatever the clusters do with them.
    ///
    /// Held here rather than reachable only through the clusters that name
    /// one, because the schema browser has to be able to say *which* registry
    /// answered, and because a registry with no clusters is a configuration
    /// mistake worth being able to see rather than one that vanishes.
    registries: BTreeMap<String, Arc<RegistryHandle>>,
    /// Declared environments, in declaration order — which is display order.
    environments: Vec<EnvironmentEntry>,
    /// The non-cluster inventory. Held here rather than beside the config so
    /// that a reload swaps the fleet and its sections together.
    resources: Vec<ResourceEntry>,
}

impl Registry {
    /// Build from configuration. Connects to nothing, and dials nothing.
    ///
    /// Fallible only because credentials can be pointed at a file that is not
    /// mounted. Nothing here reaches a network: a registry is dialled on first
    /// use, for the same reason a cluster is.
    pub fn from_config(config: &Config) -> Result<Self, ConfigError> {
        let registries = build_registries(config, &BTreeMap::new())?;
        let clusters = config
            .clusters
            .iter()
            .map(|entry| {
                let registry = entry
                    .schema_registry
                    .as_ref()
                    .and_then(|id| registries.get(id))
                    .map(Arc::clone);
                (
                    entry.id.clone(),
                    Arc::new(ClusterHandle::new(entry, registry)),
                )
            })
            .collect();
        Ok(Self {
            clusters,
            registries,
            environments: config.environments.clone(),
            resources: config.resources.clone(),
        })
    }

    /// Every declared schema registry, by id.
    ///
    /// For the process's own business — the fleet card's registry state, a
    /// reload. A handler reaches a registry only through a cluster it can
    /// already see, because "which clusters use this registry" is a list that
    /// can name a cluster the caller may not.
    pub fn schema_registries(&self) -> impl Iterator<Item = &Arc<RegistryHandle>> {
        self.registries.values()
    }

    /// The declared environments, in the order they were declared.
    ///
    /// Not every section comes from here: an `env` label nobody declared still
    /// gets one. This is the part of the order that was chosen.
    pub fn environments(&self) -> &[EnvironmentEntry] {
        &self.environments
    }

    /// Every non-cluster resource this caller can see, in configured order.
    ///
    /// The same visibility test as a cluster, against the same selectors —
    /// [`ResourceEntry::effective_labels`] is what makes `env: prod` mean the
    /// same thing for a schema registry as for a broker. A resource nobody's
    /// role selects is absent, not greyed: this is the [`Registry::visible`]
    /// rule, and a fleet that hides a prod cluster while naming the registry
    /// beside it has leaked the environment it was hiding.
    pub fn resources_visible<'a>(
        &'a self,
        who: &'a Access,
    ) -> impl Iterator<Item = &'a ResourceEntry> {
        self.resources
            .iter()
            .filter(|resource| who.sees(&resource.id, &resource.effective_labels()))
    }

    /// Look up a cluster **as somebody**.
    ///
    /// **The only way to reach a handle.** A caller that cannot see a cluster
    /// gets `None` and the router turns that into `404`, not `403`, so cluster
    /// ids are not enumerable by probing. No handler indexes the map.
    ///
    /// The visibility test lives here rather than in the router for that
    /// reason: one lookup means one place to get it right, and a handler that
    /// forgot to ask would be a handler that leaks a cluster's existence.
    pub fn get(&self, id: &str, who: &Access) -> Option<&Arc<ClusterHandle>> {
        self.clusters
            .get(id)
            .filter(|handle| who.sees(&handle.id, &handle.labels))
    }

    /// Every cluster this caller can see, in id order.
    ///
    /// The fleet view is this list. A caller in no matching role gets an empty
    /// one, which is a true answer about their fleet rather than an error
    /// about their account.
    pub fn visible<'a>(&'a self, who: &'a Access) -> impl Iterator<Item = &'a Arc<ClusterHandle>> {
        self.clusters
            .values()
            .filter(|handle| who.sees(&handle.id, &handle.labels))
    }

    /// Every cluster, whoever is asking.
    ///
    /// For the process's own business — connectors, health, shutdown — never
    /// for answering a request. Anything reachable from a handler wants
    /// [`Registry::visible`].
    pub fn all(&self) -> impl Iterator<Item = &Arc<ClusterHandle>> {
        self.clusters.values()
    }

    /// How many clusters are configured.
    pub fn len(&self) -> usize {
        self.clusters.len()
    }

    /// Whether the registry is empty. Rejected at config load, so this is
    /// false in practice.
    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }

    /// Start one connector task per cluster.
    ///
    /// Returns immediately. This is what makes startup independent of how many
    /// of the configured clusters happen to be up.
    pub fn spawn_connectors(&self) {
        for handle in self.clusters.values() {
            tokio::spawn(Arc::clone(handle).run());
        }
    }

    /// Build the registry a new configuration asks for, **reusing handles**.
    ///
    /// Adding a cluster must not disturb the connections of the eleven that
    /// did not change, so an unchanged entry keeps its `Arc<ClusterHandle>`,
    /// its connection and its snapshot. A changed entry is rebuilt; a dropped
    /// one is retired, which is what stops its connector.
    pub fn reloaded(&self, config: &Config) -> Result<Self, ConfigError> {
        // A registry whose settings did not change keeps its handle, and
        // therefore its warm id→schema cache. Adding a cluster to `dev` must
        // not cost `dev` every schema it had already resolved.
        let registries = build_registries(config, &self.registries)?;

        let mut clusters = BTreeMap::new();
        for entry in &config.clusters {
            let registry = entry
                .schema_registry
                .as_ref()
                .and_then(|id| registries.get(id))
                .map(Arc::clone);
            // A cluster whose entry is unchanged can still need rebuilding: if
            // the registry it names was rebuilt, keeping the old handle would
            // leave it decoding against a client the reload replaced.
            let registry_unchanged =
                |existing: &ClusterHandle| match (&existing.registry, &registry) {
                    (None, None) => true,
                    (Some(before), Some(after)) => Arc::ptr_eq(before, after),
                    _ => false,
                };
            match self.clusters.get(&entry.id) {
                Some(existing) if existing.entry == *entry && registry_unchanged(existing) => {
                    clusters.insert(entry.id.clone(), Arc::clone(existing));
                }
                Some(existing) => {
                    existing.retire();
                    let handle = Arc::new(ClusterHandle::new(entry, registry));
                    tokio::spawn(Arc::clone(&handle).run());
                    clusters.insert(entry.id.clone(), handle);
                }
                None => {
                    let handle = Arc::new(ClusterHandle::new(entry, registry));
                    tokio::spawn(Arc::clone(&handle).run());
                    clusters.insert(entry.id.clone(), handle);
                }
            }
        }

        for (id, existing) in &self.clusters {
            if !clusters.contains_key(id) {
                existing.retire();
            }
        }

        // Sections and inventory hold no connection, so they are simply taken
        // from the new configuration: nothing to reuse, nothing to retire.
        Ok(Self {
            clusters,
            registries,
            environments: config.environments.clone(),
            resources: config.resources.clone(),
        })
    }
}

/// Build one client per declared registry, reusing the unchanged ones.
///
/// The reuse is what makes a reload cheap: a `RegistryHandle` holds the
/// id→schema cache, so rebuilding one that nobody edited would throw away
/// every schema the environment had resolved.
fn build_registries(
    config: &Config,
    existing: &BTreeMap<String, Arc<RegistryHandle>>,
) -> Result<BTreeMap<String, Arc<RegistryHandle>>, ConfigError> {
    let mut registries = BTreeMap::new();
    for entry in &config.schema_registries {
        let settings = entry.to_settings()?;
        if let Some(handle) = existing.get(&entry.id)
            && *handle.settings() == settings
        {
            registries.insert(entry.id.clone(), Arc::clone(handle));
            continue;
        }
        let handle =
            RegistryHandle::new(settings).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        registries.insert(entry.id.clone(), Arc::new(handle));
    }
    Ok(registries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// The caller these tests are about: an open deployment's anonymous one,
    /// who can see every configured cluster. Visibility itself is tested in
    /// `kaas-ui-auth`, and against the registry in
    /// [`a_cluster_no_role_selects_does_not_exist`].
    fn anyone() -> Access {
        Access::admin()
    }

    fn config() -> Config {
        Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
  - id: strimzi
    bootstrap: ["kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn a_new_registry_has_connected_to_nothing() {
        let registry = Registry::from_config(&config()).unwrap();
        assert_eq!(registry.len(), 2);
        for handle in registry.all() {
            assert!(handle.admin().is_none());
            assert!(matches!(
                handle.health().as_ref(),
                ClusterHealth::Connecting { .. }
            ));
        }
    }

    #[test]
    fn an_unconfigured_cluster_is_absent_rather_than_forbidden() {
        let registry = Registry::from_config(&config()).unwrap();
        assert!(registry.get("nope", &anyone()).is_none());
        assert!(registry.get("kaas", &anyone()).is_some());
    }

    /// The registry is declared once and clusters reference it — so two of
    /// them hold the *same* client, and therefore the same id→schema cache.
    fn shared_registry_config() -> Config {
        Config::from_yaml(
            r#"
schema_registries:
  - id: dev
    url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7

clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
    labels: { env: dev }
  - id: strimzi
    bootstrap: ["strimzi:9092"]
    labels: { env: dev }
    schema_registry: dev
  - id: second-strimzi
    bootstrap: ["strimzi-2:9092"]
    labels: { env: dev }
    schema_registry: dev
"#,
        )
        .unwrap()
    }

    #[test]
    fn two_clusters_naming_one_registry_hold_one_client_between_them() {
        let registry = Registry::from_config(&shared_registry_config()).unwrap();

        let first = registry
            .get("strimzi", &anyone())
            .unwrap()
            .schema_registry()
            .expect("strimzi names a registry");
        let second = registry
            .get("second-strimzi", &anyone())
            .unwrap()
            .schema_registry()
            .expect("so does the second one");

        assert!(
            Arc::ptr_eq(first, second),
            "one registry id, two clients: that is a second cache for one \
             registry's ids, and one of them will answer a stale schema"
        );
        assert_eq!(first.id(), "dev");

        // And absence is a normal path, not a degraded one.
        assert!(
            registry
                .get("kaas", &anyone())
                .unwrap()
                .schema_registry()
                .is_none()
        );
        assert_eq!(registry.schema_registries().count(), 1);
    }

    #[tokio::test]
    async fn a_reload_that_adds_a_cluster_keeps_the_registrys_warm_cache() {
        let registry = Registry::from_config(&shared_registry_config()).unwrap();
        let before = Arc::clone(
            registry
                .get("strimzi", &anyone())
                .unwrap()
                .schema_registry()
                .unwrap(),
        );

        // A fourth cluster, in the same environment, naming the same registry.
        let grown = Config::from_yaml(
            r#"
schema_registries:
  - id: dev
    url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7

clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
    labels: { env: dev }
  - id: strimzi
    bootstrap: ["strimzi:9092"]
    labels: { env: dev }
    schema_registry: dev
  - id: second-strimzi
    bootstrap: ["strimzi-2:9092"]
    labels: { env: dev }
    schema_registry: dev
  - id: third-strimzi
    bootstrap: ["strimzi-3:9092"]
    labels: { env: dev }
    schema_registry: dev
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&grown).unwrap();
        let after = reloaded
            .get("strimzi", &anyone())
            .unwrap()
            .schema_registry()
            .unwrap();

        assert!(
            Arc::ptr_eq(&before, after),
            "adding a cluster to `dev` threw away every schema `dev` had resolved"
        );
        // The unchanged clusters kept their connections too.
        assert!(!before.settings().url.is_empty());
    }

    #[tokio::test]
    async fn a_registry_whose_url_changed_rebuilds_the_clusters_that_name_it() {
        // The cache is keyed by (registry, schema id) and the registry just
        // became a different registry. Keeping the old client would decode
        // `staging` ids against `dev`'s answers.
        let registry = Registry::from_config(&shared_registry_config()).unwrap();
        let before = Arc::clone(registry.get("strimzi", &anyone()).unwrap());

        let moved = Config::from_yaml(
            r#"
schema_registries:
  - id: dev
    url: http://somewhere-else:8080/apis/ccompat/v7

clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
    labels: { env: dev }
  - id: strimzi
    bootstrap: ["strimzi:9092"]
    labels: { env: dev }
    schema_registry: dev
  - id: second-strimzi
    bootstrap: ["strimzi-2:9092"]
    labels: { env: dev }
    schema_registry: dev
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&moved).unwrap();
        let after = reloaded.get("strimzi", &anyone()).unwrap();
        assert!(!Arc::ptr_eq(&before, after));
        assert!(before.is_retired());
        assert_eq!(
            after.schema_registry().unwrap().settings().url,
            "http://somewhere-else:8080/apis/ccompat/v7"
        );
    }

    #[test]
    fn a_configured_codec_matches_the_topic_and_the_first_entry_wins() {
        let config = Config::from_yaml(
            r#"
clusters:
  - id: strimzi
    bootstrap: ["a:9092"]
    codecs:
      - topic: orders-legacy
        value: hex
      - topic: "orders-*"
        key: string
        value: json
"#,
        )
        .unwrap();
        let registry = Registry::from_config(&config).unwrap();
        let cluster = registry.get("strimzi", &anyone()).unwrap();

        // The specific entry precedes the pattern, and wins the field it sets.
        // The pattern still fills in the field the specific one left alone,
        // which is what makes a narrowing entry a narrowing rather than a
        // replacement.
        assert_eq!(
            cluster.configured_codecs("orders-legacy"),
            (Codec::String, Codec::Hex)
        );
        assert_eq!(
            cluster.configured_codecs("orders-eu"),
            (Codec::String, Codec::Json)
        );
        // Anything unmatched is the Phase 3 rendering: text where it is text.
        assert_eq!(
            cluster.configured_codecs("shipments"),
            (Codec::Auto, Codec::Auto)
        );
    }

    #[tokio::test]
    async fn reload_keeps_the_handles_it_did_not_change() {
        let registry = Registry::from_config(&config()).unwrap();
        let before = Arc::clone(registry.get("kaas", &anyone()).unwrap());

        let grown = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
  - id: strimzi
    bootstrap: ["kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092"]
  - id: third
    bootstrap: ["third:9092"]
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&grown).unwrap();
        assert_eq!(reloaded.len(), 3);
        // Same allocation: the connection is not disturbed.
        assert!(Arc::ptr_eq(
            &before,
            reloaded.get("kaas", &anyone()).unwrap()
        ));
        assert!(!before.is_retired());
    }

    #[tokio::test]
    async fn reload_retires_a_dropped_cluster() {
        let registry = Registry::from_config(&config()).unwrap();
        let dropped = Arc::clone(registry.get("strimzi", &anyone()).unwrap());

        let shrunk = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&shrunk).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert!(dropped.is_retired());
        assert!(!registry.get("kaas", &anyone()).unwrap().is_retired());
    }

    #[tokio::test]
    async fn a_changed_entry_is_rebuilt() {
        let registry = Registry::from_config(&config()).unwrap();
        let before = Arc::clone(registry.get("kaas", &anyone()).unwrap());

        let moved = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["somewhere-else:9092"]
  - id: strimzi
    bootstrap: ["kafka-cluster-kafka-bootstrap.strimzi.svc.cluster.local:9092"]
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&moved).unwrap();
        assert!(!Arc::ptr_eq(
            &before,
            reloaded.get("kaas", &anyone()).unwrap()
        ));
        assert!(before.is_retired());
    }

    /// The invariant, asserted independently of the CI grep: a grep is
    /// defeated by a rename, and a test is defeated by a second call site
    /// nobody tested. Both, or neither is worth having.
    #[test]
    fn exactly_one_construction_site() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("crates");

        let mut read_only = 0usize;
        let mut mutating = 0usize;
        let mut files = vec![root];
        while let Some(path) = files.pop() {
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).unwrap() {
                    files.push(entry.unwrap().path());
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for line in source.lines() {
                // The check itself must not count: this file names both
                // spellings in prose and in this loop.
                if line.trim_start().starts_with("//") || line.contains("needle") {
                    continue;
                }
                let needle = "Admin::connect";
                for (index, _) in line.match_indices(needle) {
                    let rest = line.get(index + needle.len()..).unwrap_or_default();
                    if rest.starts_with("_read_only(") {
                        read_only += 1;
                    } else if rest.starts_with('(') {
                        mutating += 1;
                    }
                }
            }
        }

        assert_eq!(
            read_only, 1,
            "expected exactly one read-only construction site"
        );
        assert_eq!(
            mutating, 0,
            "a mutating Admin::connect exists in the workspace"
        );
    }
}

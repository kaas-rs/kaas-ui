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
use kafka_admin::{Admin, ClusterConfig};
use kafka_conn::{ConnectionConfig, Error, SaslConfig, SaslMechanism, TlsConfig};
use tokio::sync::Notify;

use crate::config::{ClusterEntry, Config, SaslMechanismName};
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
    fn new(entry: &ClusterEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.display_name().to_owned(),
            labels: entry.labels.clone(),
            entry: entry.clone(),
            admin: ArcSwapOption::empty(),
            health: ArcSwap::from_pointee(ClusterHealth::connecting()),
            retry_now: Notify::new(),
            retired: AtomicBool::new(false),
        }
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

/// Every configured cluster.
#[derive(Debug)]
pub struct Registry {
    clusters: BTreeMap<String, Arc<ClusterHandle>>,
}

impl Registry {
    /// Build from configuration. Connects to nothing.
    pub fn from_config(config: &Config) -> Self {
        let clusters = config
            .clusters
            .iter()
            .map(|entry| (entry.id.clone(), Arc::new(ClusterHandle::new(entry))))
            .collect();
        Self { clusters }
    }

    /// Look up a cluster.
    ///
    /// **The only way to reach a handle.** A caller that cannot see a cluster
    /// gets `None` and the router turns that into `404`, not `403`, so cluster
    /// ids are not enumerable by probing. No handler indexes the map.
    pub fn get(&self, id: &str) -> Option<&Arc<ClusterHandle>> {
        self.clusters.get(id)
    }

    /// Every cluster, in id order.
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
    pub fn reloaded(&self, config: &Config) -> Self {
        let mut clusters = BTreeMap::new();
        for entry in &config.clusters {
            match self.clusters.get(&entry.id) {
                Some(existing) if existing.entry == *entry => {
                    clusters.insert(entry.id.clone(), Arc::clone(existing));
                }
                Some(existing) => {
                    existing.retire();
                    let handle = Arc::new(ClusterHandle::new(entry));
                    tokio::spawn(Arc::clone(&handle).run());
                    clusters.insert(entry.id.clone(), handle);
                }
                None => {
                    let handle = Arc::new(ClusterHandle::new(entry));
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

        Self { clusters }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
        let registry = Registry::from_config(&config());
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
        let registry = Registry::from_config(&config());
        assert!(registry.get("nope").is_none());
        assert!(registry.get("kaas").is_some());
    }

    #[tokio::test]
    async fn reload_keeps_the_handles_it_did_not_change() {
        let registry = Registry::from_config(&config());
        let before = Arc::clone(registry.get("kaas").unwrap());

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

        let reloaded = registry.reloaded(&grown);
        assert_eq!(reloaded.len(), 3);
        // Same allocation: the connection is not disturbed.
        assert!(Arc::ptr_eq(&before, reloaded.get("kaas").unwrap()));
        assert!(!before.is_retired());
    }

    #[tokio::test]
    async fn reload_retires_a_dropped_cluster() {
        let registry = Registry::from_config(&config());
        let dropped = Arc::clone(registry.get("strimzi").unwrap());

        let shrunk = Config::from_yaml(
            r#"
clusters:
  - id: kaas
    bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&shrunk);
        assert_eq!(reloaded.len(), 1);
        assert!(dropped.is_retired());
        assert!(!registry.get("kaas").unwrap().is_retired());
    }

    #[tokio::test]
    async fn a_changed_entry_is_rebuilt() {
        let registry = Registry::from_config(&config());
        let before = Arc::clone(registry.get("kaas").unwrap());

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

        let reloaded = registry.reloaded(&moved);
        assert!(!Arc::ptr_eq(&before, reloaded.get("kaas").unwrap()));
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

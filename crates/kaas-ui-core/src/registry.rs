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
use kafka_conn::{
    ConnectionConfig, Error, OidcConfig, OidcTokenProvider, SaslConfig, SaslMechanism, TlsConfig,
};
use tokio::sync::Notify;

use crate::config::{
    ClusterEntry, Config, ConfigError, EnvironmentEntry, PasswordCredentials, ResourceEntry,
    SaslSettings, secret_from_env, secret_var,
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
    /// The environment holding it, which leads every URL that reaches it.
    ///
    /// Part of the identity, not decoration: a cluster is addressed as
    /// `(environment, id)` and two environments may each hold a `kafka`.
    pub environment: String,
    /// The id, as it appears in every URL after the environment.
    pub id: String,
    /// The name to render.
    pub name: String,
    /// Grouping labels, including the `env` the nesting implies.
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
    /// The credentials, prepared once.
    ///
    /// Built here rather than per connect, and that is load-bearing for
    /// `OAUTHBEARER`: the token provider caches a token and refreshes it
    /// single-flight, so one per handle means every connection to this
    /// cluster shares one token and one fetch. Rebuilding it on each attempt
    /// would fetch a token per reconnect and per broker.
    ///
    /// Secrets are read from disk here too, which is why this is fallible and
    /// why a Secret that is not mounted where the config says it is fails at
    /// startup rather than on the first page that touches the cluster.
    sasl: Option<SaslConfig>,
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
    fn new(
        environment: &str,
        entry: &ClusterEntry,
        registry: Option<Arc<RegistryHandle>>,
    ) -> Result<Self, ConfigError> {
        let sasl = entry
            .sasl
            .as_ref()
            .map(|settings| build_sasl(environment, &entry.id, settings))
            .transpose()?;
        Ok(Self {
            environment: environment.to_owned(),
            id: entry.id.clone(),
            name: entry.display_name().to_owned(),
            labels: entry.effective_labels(environment),
            entry: entry.clone(),
            registry,
            sasl,
            admin: ArcSwapOption::empty(),
            health: ArcSwap::from_pointee(ClusterHealth::connecting()),
            retry_now: Notify::new(),
            retired: AtomicBool::new(false),
        })
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

        // Cloned, not rebuilt: the `SaslConfig` was prepared once when the
        // handle was, and for `OAUTHBEARER` it holds the token provider whose
        // whole value is being shared. `SaslConfig` clones the `Arc` inside.
        if let Some(sasl) = &self.sasl {
            connection = connection.with_sasl(sasl.clone());
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

/// Turn one cluster's configured credentials into kaas-lib's.
///
/// Called once per cluster, when the registry is built, so everything that
/// can be wrong about a credential — a Secret that is not mounted, a token
/// endpoint that is not a url, an `http://` issuer — is wrong at startup with
/// the cluster id in the message, rather than on the third retry of a
/// background connector nobody is reading the logs of.
fn build_sasl(
    environment: &str,
    cluster: &str,
    settings: &SaslSettings,
) -> Result<SaslConfig, ConfigError> {
    let invalid = |message: String| ConfigError::Invalid(format!("cluster {cluster:?}: {message}"));

    match settings {
        SaslSettings::Plain(credentials) => {
            password_sasl(environment, cluster, SaslMechanism::Plain, credentials)
        }
        SaslSettings::ScramSha256(credentials) => password_sasl(
            environment,
            cluster,
            SaslMechanism::ScramSha256,
            credentials,
        ),
        SaslSettings::ScramSha512(credentials) => password_sasl(
            environment,
            cluster,
            SaslMechanism::ScramSha512,
            credentials,
        ),
        SaslSettings::OauthBearer(oauth) => {
            // The file wins where it says anything, and says nothing in the
            // deployment — which is the point: the config carries a credential
            // nowhere and the environment supplies it.
            let secret = match &oauth.client_secret {
                Some(inline) => inline.clone(),
                None => secret_from_env(
                    &format!("cluster {cluster:?}"),
                    &secret_var("client_secret", environment, cluster),
                )?,
            };

            let mut config = OidcConfig::new(
                oauth.token_endpoint.clone(),
                oauth.client_id.clone(),
                secret,
            )
            .with_maybe_scope(oauth.scope.clone())
            .with_maybe_audience(oauth.audience.clone());
            if let Some(margin) = oauth.refresh_margin {
                config = config.with_refresh_margin(margin);
            }
            if let Some(timeout) = oauth.timeout {
                config = config.with_timeout(timeout);
            }
            if oauth.credentials_in_body {
                config = config.with_credentials_in_body();
            }
            if oauth.allow_plaintext_endpoint {
                config = config.with_allow_plaintext_endpoint();
            }

            // Nothing is fetched here — the first token is fetched when the
            // first connection authenticates — so this validates the endpoint
            // and nothing else. Which is the point: a typo in the url should
            // not wait for a broker to be reachable to be noticed.
            let provider =
                OidcTokenProvider::new(config).map_err(|error| invalid(error.to_string()))?;

            let mut sasl = SaslConfig::oauth_bearer(provider);
            if oauth.allow_plaintext_token {
                sasl = sasl.allow_plaintext_password();
            }
            Ok(sasl)
        }
    }
}

/// The three mechanisms that authenticate with a password.
fn password_sasl(
    environment: &str,
    cluster: &str,
    mechanism: SaslMechanism,
    credentials: &PasswordCredentials,
) -> Result<SaslConfig, ConfigError> {
    // Same rule as the OAuth secret one function up: written here it wins,
    // omitted it comes from the environment under a derived name.
    let password = match &credentials.password {
        Some(inline) => inline.clone(),
        None => secret_from_env(
            &format!("cluster {cluster:?} ({mechanism})"),
            &secret_var("password", environment, cluster),
        )?,
    };
    let mut sasl = SaslConfig::new(mechanism, credentials.username.clone(), password);
    if credentials.allow_plaintext_password {
        sasl = sasl.allow_plaintext_password();
    }
    Ok(sasl)
}

/// Read a PEM file — a CA bundle, a client certificate, a key.
///
/// The one credential-adjacent thing that stays a *file*, and deliberately:
/// PEM is multi-line, and an environment variable holding a certificate chain
/// is a quoting problem waiting to happen. Single-line secrets go through
/// `secret_var`; this does not.
fn read_pem(path: &std::path::Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|source| {
        // The path is the whole diagnosis when a Secret is not mounted where
        // the config says it is.
        Error::InvalidRequest(format!("{}: {source}", path.display()))
    })
}

/// What addresses a cluster or a registry: the environment, then the id.
///
/// An id alone addresses nothing. Two environments may each hold a `kafka` or
/// an `apicurio`, and a lookup that took only the second half would answer
/// with whichever one sorted first — across an environment boundary a caller
/// may not be allowed to cross.
pub type Key = (String, String);

/// Every configured cluster, and the fleet it is arranged into.
#[derive(Debug)]
pub struct Registry {
    clusters: BTreeMap<Key, Arc<ClusterHandle>>,
    /// One client per declared registry, whatever the clusters do with them.
    ///
    /// Held here rather than reachable only through the clusters that name
    /// one, because the schema browser addresses a registry directly now, and
    /// because a registry no cluster references is a configuration mistake
    /// worth being able to see rather than one that vanishes.
    registries: BTreeMap<Key, Arc<RegistryHandle>>,
    /// The environments, in declaration order — which is display order. They
    /// carry the inventory too, so a reload swaps the fleet and its sections
    /// together because they are one value.
    environments: Vec<EnvironmentEntry>,
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
            .clusters()
            .map(|(environment, entry)| {
                let registry = registry_for(&registries, environment, entry);
                Ok((
                    (environment.to_owned(), entry.id.clone()),
                    Arc::new(ClusterHandle::new(environment, entry, registry)?),
                ))
            })
            .collect::<Result<_, ConfigError>>()?;
        Ok(Self {
            clusters,
            registries,
            environments: config.environments.clone(),
        })
    }

    /// Every declared schema registry, with the environment holding it.
    ///
    /// For the process's own business — a reload, a health sweep. A handler
    /// wants [`Registry::schema_registry`], which asks who is looking.
    pub fn schema_registries(&self) -> impl Iterator<Item = (&Key, &Arc<RegistryHandle>)> {
        self.registries.iter()
    }

    /// The environments, in the order they were declared.
    pub fn environments(&self) -> &[EnvironmentEntry] {
        &self.environments
    }

    /// One environment, if this caller can see anything in it.
    ///
    /// An environment is visible when at least one of its clusters is. That is
    /// the rule the fleet already had — an empty section is dropped, because
    /// rendering "Production" with nothing under it tells a caller who may not
    /// see prod that prod exists — and making it the lookup means the URL
    /// namespace inherits it instead of restating it.
    pub fn environment(&self, id: &str, who: &Access) -> Option<&EnvironmentEntry> {
        let entry = self
            .environments
            .iter()
            .find(|environment| environment.id == id)?;
        self.visible_in(id, who).next()?;
        Some(entry)
    }

    /// Every non-cluster resource this caller can see, with its environment.
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
    ) -> impl Iterator<Item = (&'a str, &'a ResourceEntry)> {
        self.environments.iter().flat_map(move |environment| {
            environment
                .resources
                .iter()
                .map(move |resource| (environment.id.as_str(), resource))
                .filter(move |(env, resource)| {
                    who.sees(&resource.id, &resource.effective_labels(env))
                })
        })
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
    pub fn get(&self, environment: &str, id: &str, who: &Access) -> Option<&Arc<ClusterHandle>> {
        self.clusters
            .get(&(environment.to_owned(), id.to_owned()))
            .filter(|handle| who.sees(&handle.id, &handle.labels))
    }

    /// Look up a schema registry **as somebody**.
    ///
    /// The same shape as [`Registry::get`], and it has to be: a registry is
    /// addressable now, so it needs its own 404-not-403 rule rather than
    /// borrowing a cluster's by being reached through one.
    ///
    /// Two cases, and conflating them was a bug:
    ///
    /// * **Referenced by at least one cluster** — visible only when the caller
    ///   can read topics on one of *those*. A registry only some clusters use
    ///   is a fact about those clusters, and someone who cannot see them must
    ///   not learn it exists from a URL that answers.
    /// * **Referenced by nothing** — visible to anyone who may read topics
    ///   anywhere in the environment. It names no cluster, so there is nothing
    ///   to leak, and hiding it hides a configuration mistake: a registry
    ///   nobody decodes against is worth being able to see rather than one
    ///   that silently vanishes.
    pub fn schema_registry(
        &self,
        environment: &str,
        id: &str,
        who: &Access,
    ) -> Option<&Arc<RegistryHandle>> {
        let handle = self
            .registries
            .get(&(environment.to_owned(), id.to_owned()))?;

        let references = |cluster: &Arc<ClusterHandle>| {
            cluster
                .schema_registry()
                .is_some_and(|r| Arc::ptr_eq(r, handle))
        };
        let referenced = self
            .clusters
            .values()
            .any(|cluster| cluster.environment == environment && references(cluster));

        let mut readers = self.readers_in(environment, who);
        if referenced {
            readers.any(references).then_some(handle)
        } else {
            readers.next().map(|_| handle)
        }
    }

    /// The clusters here that decode against this registry, for a caller who
    /// may read topic names on them.
    ///
    /// The referencing half of [`Registry::schema_registry`]'s rule, exposed:
    /// that decides whether a registry may be *seen*, and this answers who is
    /// reading it. Both go through [`Registry::readers_in`], so a caller who
    /// may not read topics on a cluster cannot learn what it holds by asking
    /// the registry beside it instead.
    ///
    /// Empty is a real answer — a registry nobody references — and a caller
    /// that turns it into a count has to decide what nothing to check against
    /// means before it can report one.
    pub fn readers_of<'a>(
        &'a self,
        environment: &'a str,
        registry: &'a Arc<RegistryHandle>,
        who: &'a Access,
    ) -> impl Iterator<Item = &'a Arc<ClusterHandle>> {
        self.readers_in(environment, who).filter(move |cluster| {
            cluster
                .schema_registry()
                .is_some_and(|held| Arc::ptr_eq(held, registry))
        })
    }

    /// The clusters this caller can see in one environment.
    fn visible_in<'a>(
        &'a self,
        environment: &'a str,
        who: &'a Access,
    ) -> impl Iterator<Item = &'a Arc<ClusterHandle>> {
        self.visible(who)
            .filter(move |handle| handle.environment == environment)
    }

    /// The clusters here this caller may *view topics on*.
    ///
    /// Stronger than [`Registry::visible_in`], and the subject list needs the
    /// stronger one: `Resource::Topic` + `view` is what guards a topic name,
    /// and a subject name is metadata of the same kind. Seeing that a cluster
    /// exists is not the same permission as reading what is described on it.
    fn readers_in<'a>(
        &'a self,
        environment: &'a str,
        who: &'a Access,
    ) -> impl Iterator<Item = &'a Arc<ClusterHandle>> {
        self.visible_in(environment, who).filter(move |handle| {
            who.may(
                &handle.id,
                &handle.labels,
                kaas_ui_auth::Resource::Topic,
                kaas_ui_auth::Action::View,
                None,
            )
        })
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
        for (environment, entry) in config.clusters() {
            let registry = registry_for(&registries, environment, entry);
            // A cluster whose entry is unchanged can still need rebuilding: if
            // the registry it names was rebuilt, keeping the old handle would
            // leave it decoding against a client the reload replaced.
            let registry_unchanged =
                |existing: &ClusterHandle| match (&existing.registry, &registry) {
                    (None, None) => true,
                    (Some(before), Some(after)) => Arc::ptr_eq(before, after),
                    _ => false,
                };
            // Moving a cluster between environments changes its identity, so
            // it falls out of this lookup and is rebuilt — which is right: its
            // labels, its registry and its URL all just changed.
            let key = (environment.to_owned(), entry.id.clone());
            match self.clusters.get(&key) {
                Some(existing) if existing.entry == *entry && registry_unchanged(existing) => {
                    clusters.insert(key, Arc::clone(existing));
                }
                Some(existing) => {
                    existing.retire();
                    let handle = Arc::new(ClusterHandle::new(environment, entry, registry)?);
                    tokio::spawn(Arc::clone(&handle).run());
                    clusters.insert(key, handle);
                }
                None => {
                    let handle = Arc::new(ClusterHandle::new(environment, entry, registry)?);
                    tokio::spawn(Arc::clone(&handle).run());
                    clusters.insert(key, handle);
                }
            }
        }

        for (key, existing) in &self.clusters {
            if !clusters.contains_key(key) {
                existing.retire();
            }
        }

        // Environments hold no connection, so they are simply taken from the
        // new configuration: nothing to reuse, nothing to retire.
        Ok(Self {
            clusters,
            registries,
            environments: config.environments.clone(),
        })
    }
}

/// The registry handle a cluster's `schema_registry:` names, within its own
/// environment.
///
/// Scoped deliberately. A cluster in `dev` naming `apicurio` must not reach
/// `prod`'s `apicurio` because the map happened to hold one — the config
/// validator already refuses a reference the environment does not declare, and
/// this is the same rule expressed where the handle is actually chosen.
fn registry_for(
    registries: &BTreeMap<Key, Arc<RegistryHandle>>,
    environment: &str,
    entry: &ClusterEntry,
) -> Option<Arc<RegistryHandle>> {
    let id = entry.schema_registry.as_ref()?;
    registries
        .get(&(environment.to_owned(), id.clone()))
        .map(Arc::clone)
}

/// Build one client per declared registry, reusing the unchanged ones.
///
/// The reuse is what makes a reload cheap: a `RegistryHandle` holds the
/// id→schema cache, so rebuilding one that nobody edited would throw away
/// every schema the environment had resolved.
fn build_registries(
    config: &Config,
    existing: &BTreeMap<Key, Arc<RegistryHandle>>,
) -> Result<BTreeMap<Key, Arc<RegistryHandle>>, ConfigError> {
    let mut registries = BTreeMap::new();
    for (environment, entry) in config.schema_registries() {
        let key = (environment.to_owned(), entry.id.clone());
        let settings = entry.to_settings(environment)?;
        if let Some(handle) = existing.get(&key)
            && *handle.settings() == settings
        {
            registries.insert(key, Arc::clone(handle));
            continue;
        }
        let handle =
            RegistryHandle::new(settings).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        registries.insert(key, Arc::new(handle));
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
environments:
  - id: dev
    kafka_clusters:
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
        assert!(registry.get("dev", "nope", &anyone()).is_none());
        assert!(registry.get("dev", "kaas", &anyone()).is_some());
    }

    /// The registry is declared once and clusters reference it — so two of
    /// them hold the *same* client, and therefore the same id→schema cache.
    fn shared_registry_config() -> Config {
        Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
      - id: strimzi
        bootstrap: ["strimzi:9092"]
        schema_registry: apicurio
      - id: second-strimzi
        bootstrap: ["strimzi-2:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap()
    }

    #[test]
    fn two_clusters_naming_one_registry_hold_one_client_between_them() {
        let registry = Registry::from_config(&shared_registry_config()).unwrap();

        let first = registry
            .get("dev", "strimzi", &anyone())
            .unwrap()
            .schema_registry()
            .expect("strimzi names a registry");
        let second = registry
            .get("dev", "second-strimzi", &anyone())
            .unwrap()
            .schema_registry()
            .expect("so does the second one");

        assert!(
            Arc::ptr_eq(first, second),
            "one registry id, two clients: that is a second cache for one \
             registry's ids, and one of them will answer a stale schema"
        );
        assert_eq!(first.id(), "apicurio");

        // And absence is a normal path, not a degraded one.
        assert!(
            registry
                .get("dev", "kaas", &anyone())
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
                .get("dev", "strimzi", &anyone())
                .unwrap()
                .schema_registry()
                .unwrap(),
        );

        // A fourth cluster, in the same environment, naming the same registry.
        let grown = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio-registry.apicurio.svc.cluster.local:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
      - id: strimzi
        bootstrap: ["strimzi:9092"]
        schema_registry: apicurio
      - id: second-strimzi
        bootstrap: ["strimzi-2:9092"]
        schema_registry: apicurio
      - id: third-strimzi
        bootstrap: ["strimzi-3:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&grown).unwrap();
        let after = reloaded
            .get("dev", "strimzi", &anyone())
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
        let before = Arc::clone(registry.get("dev", "strimzi", &anyone()).unwrap());

        let moved = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://somewhere-else:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
      - id: strimzi
        bootstrap: ["strimzi:9092"]
        schema_registry: apicurio
      - id: second-strimzi
        bootstrap: ["strimzi-2:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&moved).unwrap();
        let after = reloaded.get("dev", "strimzi", &anyone()).unwrap();
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
environments:
  - id: dev
    kafka_clusters:
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
        let cluster = registry.get("dev", "strimzi", &anyone()).unwrap();

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
        let before = Arc::clone(registry.get("dev", "kaas", &anyone()).unwrap());

        let grown = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
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
            reloaded.get("dev", "kaas", &anyone()).unwrap()
        ));
        assert!(!before.is_retired());
    }

    #[tokio::test]
    async fn reload_retires_a_dropped_cluster() {
        let registry = Registry::from_config(&config()).unwrap();
        let dropped = Arc::clone(registry.get("dev", "strimzi", &anyone()).unwrap());

        let shrunk = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .unwrap();

        let reloaded = registry.reloaded(&shrunk).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert!(dropped.is_retired());
        assert!(!registry.get("dev", "kaas", &anyone()).unwrap().is_retired());
    }

    #[tokio::test]
    async fn a_changed_entry_is_rebuilt() {
        let registry = Registry::from_config(&config()).unwrap();
        let before = Arc::clone(registry.get("dev", "kaas", &anyone()).unwrap());

        let moved = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
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
            reloaded.get("dev", "kaas", &anyone()).unwrap()
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

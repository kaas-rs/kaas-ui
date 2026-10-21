//! One Connect cluster's REST API, read-only.
//!
//! **Every call here is a `GET`, and that is a construction property, not a
//! habit.** [`ConnectClient`] has one typed method per read endpoint and one
//! private [`ConnectClient::get_json`] that all of them go through. There is
//! no method that takes an HTTP method and no passthrough. Writing lives in
//! `writer.rs`, whose only constructor takes a `Grant` from the gate — and
//! `cargo xtask ci` greps that no non-GET is spelled anywhere else in this
//! crate, while `tests/reads.rs` asserts against a stub worker that the read
//! path sends nothing but `GET`.
//!
//! The credentials kaas-ui is given for a Connect cluster can usually delete
//! connectors. Connect has no read-only mode to ask for, so the guarantee has
//! to be this one: there is nothing here to send a write with.
//!
//! Like the registry client, connecting is lazy and failure is a state. A
//! Connect cluster that cannot be reached degrades the pages that need it and
//! delays nothing else — not startup, not `/health`, not a Kafka page.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use arc_swap::ArcSwap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;
use utoipa::ToSchema;

use crate::fault::ConnectFault;
use crate::model::{
    ConnectorDetail, ConnectorSummary, Plugin, RedactedConfig, ServerInfo, StoredOffset, TaskConfig,
};
use crate::redact::{PluginKeys, redact};
use crate::settings::{ConnectAuth, ConnectSettings, DEFAULT_TOPICS_TTL};
use crate::wire;

/// Retry floor and ceiling for a Connect cluster that will not answer.
const PROBE_BACKOFF_MIN: Duration = Duration::from_secs(1);
const PROBE_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// Where one Connect cluster is in its lifecycle.
#[derive(Debug, Clone)]
pub enum ConnectHealth {
    /// Nothing has needed it yet.
    Unprobed,
    /// It answered `GET /`.
    Ready {
        /// What it said.
        info: ServerInfo,
        /// When it last did.
        since: SystemTime,
    },
    /// It could not be reached.
    Unreachable {
        /// What went wrong.
        error: String,
        /// Since when.
        since: SystemTime,
        /// How many attempts have failed.
        attempts: u32,
    },
    /// It answered and is not usable as configured: refused the credentials,
    /// or is not a Connect REST endpoint.
    Misconfigured {
        /// What is wrong.
        error: String,
        /// Since when.
        since: SystemTime,
    },
}

impl ConnectHealth {
    /// The wire form.
    #[must_use]
    pub fn status(&self) -> ConnectStatus {
        match self {
            Self::Unprobed => ConnectStatus::Unprobed,
            Self::Ready { .. } => ConnectStatus::Ready,
            Self::Unreachable { .. } => ConnectStatus::Unreachable,
            Self::Misconfigured { .. } => ConnectStatus::Misconfigured,
        }
    }

    /// What went wrong, where something did.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Unprobed | Self::Ready { .. } => None,
            Self::Unreachable { error, .. } | Self::Misconfigured { error, .. } => Some(error),
        }
    }

    /// What `GET /` last said, where it has.
    #[must_use]
    pub fn info(&self) -> Option<&ServerInfo> {
        match self {
            Self::Ready { info, .. } => Some(info),
            _ => None,
        }
    }
}

/// The Connect health state, flattened for the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConnectStatus {
    /// Nothing has needed it yet.
    Unprobed,
    /// It answers.
    Ready,
    /// It could not be reached. Retried on a backoff.
    Unreachable,
    /// It answered, and refused the credentials or is not Connect.
    Misconfigured,
}

/// Why a client could not be built at all.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    /// The HTTP client would not build, or a PEM file would not load.
    #[error("Connect cluster {id}: {message}")]
    Client {
        /// The configured id.
        id: String,
        /// What went wrong.
        message: String,
    },
}

/// The expanded connector list, and when it was fetched.
#[derive(Debug, Clone)]
struct Cached<T> {
    value: T,
    at: Instant,
}

/// What the last probe concluded, and when.
#[derive(Debug, Default)]
struct Probe {
    outcome: Option<Result<ServerInfo, ConnectFault>>,
    at: Option<Instant>,
    attempts: u32,
}

/// One Connect cluster.
#[derive(Debug)]
pub struct ConnectClient {
    settings: ConnectSettings,
    http: reqwest::Client,
    health: ArcSwap<ConnectHealth>,
    probe: Mutex<Probe>,
    connectors: RwLock<Option<Cached<Arc<Vec<ConnectorSummary>>>>>,
    plugins: RwLock<HashMap<String, Cached<Arc<PluginKeys>>>>,
    topics: RwLock<HashMap<String, Cached<Arc<Vec<String>>>>>,
    requests: AtomicU64,
}

impl ConnectClient {
    /// Build the client for one configured Connect cluster. Dials nothing.
    ///
    /// Reads the PEM files it was pointed at, so a Secret not mounted where
    /// the config says fails here, at startup, rather than on the first page.
    pub fn new(settings: ConnectSettings) -> Result<Self, ConnectError> {
        kaas_ui_serde::install_crypto_provider();
        let fail = |message: String| ConnectError::Client {
            id: settings.id.clone(),
            message,
        };

        let mut builder = reqwest::Client::builder()
            .timeout(settings.request_timeout)
            .connect_timeout(settings.connect_timeout)
            // A worker never redirects a REST call; following one would be
            // following it somewhere the credentials were not meant to go.
            .redirect(reqwest::redirect::Policy::none());
        if let Some(tls) = &settings.tls {
            if let Some(ca) = &tls.ca_file {
                let pem = read(ca).map_err(fail)?;
                for certificate in reqwest::Certificate::from_pem_bundle(&pem)
                    .map_err(|e| fail(format!("{}: {e}", ca.display())))?
                {
                    builder = builder.add_root_certificate(certificate);
                }
            }
            if let Some((cert, key)) = &tls.client {
                let mut pem = read(cert).map_err(fail)?;
                pem.extend(read(key).map_err(fail)?);
                let identity = reqwest::Identity::from_pem(&pem)
                    .map_err(|e| fail(format!("{} + {}: {e}", cert.display(), key.display())))?;
                builder = builder.identity(identity);
            }
        }
        let http = builder.build().map_err(|e| fail(e.to_string()))?;

        Ok(Self {
            settings,
            http,
            health: ArcSwap::from_pointee(ConnectHealth::Unprobed),
            probe: Mutex::new(Probe::default()),
            connectors: RwLock::new(None),
            plugins: RwLock::new(HashMap::new()),
            topics: RwLock::new(HashMap::new()),
            requests: AtomicU64::new(0),
        })
    }

    /// The configured id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.settings.id
    }

    /// The name to render.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.settings.name
    }

    /// The configured url.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.settings.url
    }

    /// The settings, so a reload can tell whether anything changed.
    #[must_use]
    pub fn settings(&self) -> &ConnectSettings {
        &self.settings
    }

    /// The current state.
    #[must_use]
    pub fn health(&self) -> Arc<ConnectHealth> {
        self.health.load_full()
    }

    /// How many requests this process has sent this Connect cluster.
    #[must_use]
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// `GET /`, behind a backoff: what the workers are, or why not.
    ///
    /// Concurrent callers share one attempt, and a failure is remembered for
    /// its backoff so a page full of components asking does not become a
    /// burst against a worker that is down.
    pub async fn ready(&self) -> Result<ServerInfo, ConnectFault> {
        if let Some(info) = self.health.load().info() {
            return Ok(info.clone());
        }
        let mut probe = self.probe.lock().await;
        if let (Some(outcome), Some(at)) = (probe.outcome.clone(), probe.at) {
            let due = match &outcome {
                Ok(info) => return Ok(info.clone()),
                Err(ConnectFault::Unreachable(_) | ConnectFault::Rebalancing(_)) => {
                    backoff(probe.attempts)
                }
                Err(_) => PROBE_BACKOFF_MAX,
            };
            if at.elapsed() < due {
                return outcome;
            }
        }

        let outcome = self
            .get_json::<wire::ServerInfo>("/")
            .await
            .map(ServerInfo::from);
        probe.at = Some(Instant::now());
        match &outcome {
            Ok(info) => {
                probe.attempts = 0;
                self.health.store(Arc::new(ConnectHealth::Ready {
                    info: info.clone(),
                    since: SystemTime::now(),
                }));
            }
            Err(fault) => {
                probe.attempts = probe.attempts.saturating_add(1);
                self.record_fault(fault, probe.attempts);
            }
        }
        probe.outcome = Some(outcome.clone());
        outcome
    }

    /// Every connector, with its state and class, from one call.
    ///
    /// `GET /connectors?expand=status&expand=info` — absent from the OpenAPI
    /// document, read by the worker from the query string, and present on
    /// every worker kaas-ui will meet. Cached for
    /// [`ConnectSettings::connectors_ttl`].
    pub async fn connectors(&self) -> Result<Arc<Vec<ConnectorSummary>>, ConnectFault> {
        if let Some(cached) = self.connectors.read().ok().and_then(|c| c.clone())
            && cached.at.elapsed() < self.settings.connectors_ttl
        {
            return Ok(cached.value);
        }
        self.ready().await?;
        let expanded: BTreeMap<String, wire::Expanded> = self
            .get_json("/connectors?expand=status&expand=info")
            .await?;
        let summaries: Vec<ConnectorSummary> = expanded
            .into_iter()
            .map(|(name, entry)| {
                let info = entry.info;
                ConnectorSummary::of(
                    &name,
                    entry.status,
                    info.as_ref().map(|info| &info.config),
                    info.as_ref().and_then(|info| info.kind.as_deref()),
                )
            })
            .collect();
        let summaries = Arc::new(summaries);
        if let Ok(mut cache) = self.connectors.write() {
            *cache = Some(Cached {
                value: Arc::clone(&summaries),
                at: Instant::now(),
            });
        }
        Ok(summaries)
    }

    /// One connector: state, tasks, and its config and task configs redacted.
    ///
    /// Fresh rather than cached — the detail page is where someone watches a
    /// task come back.
    pub async fn connector(&self, name: &str) -> Result<ConnectorDetail, ConnectFault> {
        self.ready().await?;
        let base = format!("/connectors/{}", segment(name));
        let info: wire::ConnectorInfo = self.get_json(&base).await?;
        let status: wire::StateInfo = self.get_json(&format!("{base}/status")).await?;
        // Task configs are a second-class read: a worker that will describe
        // the connector and not its tasks still gets a page.
        let tasks: Vec<wire::TaskInfo> = self
            .get_json(&format!("{base}/tasks"))
            .await
            .unwrap_or_default();

        let keys = match info.config.get("connector.class") {
            Some(class) => self.plugin_keys(class).await,
            None => None,
        };
        let (entries, redaction) = redact(&info.config, keys.as_deref());
        let task_configs = tasks
            .into_iter()
            .map(|task| {
                let (entries, redaction) = redact(&task.config, keys.as_deref());
                TaskConfig {
                    id: task.id.task,
                    config: RedactedConfig { entries, redaction },
                }
            })
            .collect();

        Ok(ConnectorDetail {
            summary: ConnectorSummary::of(
                name,
                Some(status),
                Some(&info.config),
                info.kind.as_deref(),
            ),
            config: RedactedConfig { entries, redaction },
            task_configs,
        })
    }

    /// The topics a connector has actually used (`GET /connectors/{c}/topics`).
    ///
    /// [`ConnectFault::Absent`] where the worker does not track topics.
    pub async fn topics(&self, name: &str) -> Result<Vec<String>, ConnectFault> {
        self.ready().await?;
        let mut answer: BTreeMap<String, wire::ActiveTopics> = self
            .get_json(&format!("/connectors/{}/topics", segment(name)))
            .await?;
        let mut topics = answer
            .remove(name)
            .map(|active| active.topics)
            .unwrap_or_default();
        topics.sort();
        if let Ok(mut cache) = self.topics.write() {
            cache.insert(
                name.to_owned(),
                Cached {
                    value: Arc::new(topics.clone()),
                    at: Instant::now(),
                },
            );
        }
        Ok(topics)
    }

    /// The same list, believed for half a minute.
    ///
    /// For the reverse index — "which connectors touch this topic" — which is
    /// one call per connector and would otherwise make every topic page cost
    /// the worker a call per connector it runs.
    pub async fn topics_cached(&self, name: &str) -> Result<Arc<Vec<String>>, ConnectFault> {
        if let Some(cached) = self
            .topics
            .read()
            .ok()
            .and_then(|cache| cache.get(name).cloned())
            && cached.at.elapsed() < DEFAULT_TOPICS_TTL
        {
            return Ok(cached.value);
        }
        self.topics(name).await.map(Arc::new)
    }

    /// A connector's stored offsets (`GET /connectors/{c}/offsets`).
    ///
    /// [`ConnectFault::Absent`] on a worker without the endpoint.
    pub async fn offsets(&self, name: &str) -> Result<Vec<StoredOffset>, ConnectFault> {
        self.ready().await?;
        let offsets: wire::Offsets = self
            .get_json(&format!("/connectors/{}/offsets", segment(name)))
            .await?;
        Ok(offsets
            .offsets
            .into_iter()
            .map(StoredOffset::from)
            .collect())
    }

    /// The installed connector plugins — connectors only, not converters.
    pub async fn plugins(&self) -> Result<Vec<Plugin>, ConnectFault> {
        self.ready().await?;
        let plugins: Vec<wire::PluginInfo> = self
            .get_json("/connector-plugins?connectorsOnly=true")
            .await?;
        let mut plugins: Vec<Plugin> = plugins.into_iter().map(Plugin::from).collect();
        plugins.sort_by(|a, b| a.class.cmp(&b.class));
        Ok(plugins)
    }

    /// What a plugin's definition declares, cached; `None` when it cannot be
    /// fetched, which makes redaction fall back to names alone.
    async fn plugin_keys(&self, class: &str) -> Option<Arc<PluginKeys>> {
        if let Some(cached) = self
            .plugins
            .read()
            .ok()
            .and_then(|cache| cache.get(class).cloned())
            && cached.at.elapsed() < self.settings.plugin_ttl
        {
            return Some(cached.value);
        }
        let definition: Vec<wire::ConfigKeyInfo> = self
            .get_json(&format!("/connector-plugins/{}/config", segment(class)))
            .await
            .ok()?;
        let mut keys = PluginKeys::default();
        for key in definition {
            if key.kind.as_deref() == Some("PASSWORD") {
                keys.secrets.insert(key.name.clone());
            }
            keys.described
                .insert(key.name, (key.group, key.documentation));
        }
        let keys = Arc::new(keys);
        if let Ok(mut cache) = self.plugins.write() {
            cache.insert(
                class.to_owned(),
                Cached {
                    value: Arc::clone(&keys),
                    at: Instant::now(),
                },
            );
        }
        Some(keys)
    }

    /// The HTTP client, for the writer — which builds its requests in its
    /// own file, the one place `cargo xtask ci` lets a non-GET be spelled.
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// `path` joined onto the configured base.
    pub(crate) fn url_for(&self, path: &str) -> String {
        format!("{}{path}", self.settings.base())
    }

    /// Attach the configured credentials.
    pub(crate) fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.settings.auth {
            Some(ConnectAuth::Basic { username, password }) => {
                request.basic_auth(username, Some(password))
            }
            Some(ConnectAuth::Bearer(token)) => request.bearer_auth(token),
            None => request,
        }
    }

    /// Forget the cached connector list, after a write changed it.
    pub(crate) fn invalidate(&self) {
        if let Ok(mut cache) = self.connectors.write() {
            *cache = None;
        }
    }

    /// Classify a failed response, recording an outage or a refusal as the
    /// health it is — shared by reads and the writer.
    pub(crate) async fn fault_of(&self, response: reqwest::Response, path: &str) -> ConnectFault {
        let status = response.status();
        let message = response
            .json::<wire::ErrorBody>()
            .await
            .ok()
            .and_then(|body| body.message);
        let fault =
            ConnectFault::of_status(status, message, &self.settings.id, &self.settings.url, path);
        if matches!(
            fault,
            ConnectFault::Unreachable(_) | ConnectFault::Refused(_)
        ) {
            self.record_fault(&fault, 0);
        }
        fault
    }

    /// Record that the workers could not be reached at all.
    pub(crate) fn unreachable(&self, error: &reqwest::Error) -> ConnectFault {
        let fault = ConnectFault::Unreachable(format!(
            "Connect cluster {:?} at {} could not be reached: {error}",
            self.settings.id, self.settings.url
        ));
        self.record_fault(&fault, 0);
        fault
    }

    /// **The only read request this crate makes.** A `GET`, authenticated,
    /// parsed.
    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ConnectFault> {
        let request = self.authenticate(
            self.http
                .get(self.url_for(path))
                .header(reqwest::header::ACCEPT, "application/json"),
        );

        self.requests.fetch_add(1, Ordering::Relaxed);
        let id = &self.settings.id;
        let response = request
            .send()
            .await
            .map_err(|error| self.unreachable(&error))?;

        if !response.status().is_success() {
            return Err(self.fault_of(response, path).await);
        }

        response.json::<T>().await.map_err(|error| {
            // A 200 that is not the shape Connect sends is a proxy's login
            // page or a url pointing at something else. Configuration, not an
            // outage.
            ConnectFault::Misconfigured(format!(
                "Connect cluster {id:?} at {} answered {path} with something that is not the \
                 Connect REST API: {error}",
                self.settings.url
            ))
        })
    }

    /// Count one request — reads and writes alike.
    pub(crate) fn count(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    fn record_fault(&self, fault: &ConnectFault, attempts: u32) {
        let previous = self.health.load();
        let since = match previous.as_ref() {
            ConnectHealth::Unreachable { since, .. }
            | ConnectHealth::Misconfigured { since, .. } => *since,
            _ => SystemTime::now(),
        };
        let health = match fault {
            ConnectFault::Refused(error) | ConnectFault::Misconfigured(error) => {
                ConnectHealth::Misconfigured {
                    error: error.clone(),
                    since,
                }
            }
            other => ConnectHealth::Unreachable {
                error: other.to_string(),
                since,
                attempts: attempts.max(1),
            },
        };
        self.health.store(Arc::new(health));
        // A worker that stopped answering must be re-probed by the next
        // caller rather than trusted on a stale `Ready`.
        if let Ok(mut probe) = self.probe.try_lock()
            && matches!(probe.outcome, Some(Ok(_)))
        {
            probe.outcome = None;
            probe.at = None;
        }
    }
}

/// Read a PEM file, naming the path when it is missing — the path is the
/// whole diagnosis when a Secret is not mounted where the config says.
fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|source| format!("{}: {source}", path.display()))
}

/// Percent-encode one path segment.
///
/// Connector names are free text — spaces, slashes and all — and a name
/// spliced into a path unescaped would address a different resource.
pub(crate) fn segment(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn backoff(attempts: u32) -> Duration {
    let shift = attempts.saturating_sub(1).min(6);
    PROBE_BACKOFF_MIN
        .saturating_mul(1u32 << shift)
        .min(PROBE_BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_cannot_escape_its_segment() {
        assert_eq!(segment("gtfs-rt.v1_x~"), "gtfs-rt.v1_x~");
        assert_eq!(segment("a/b c"), "a%2Fb%20c");
        assert_eq!(segment("../connectors"), "..%2Fconnectors");
    }

    #[test]
    fn a_new_client_has_asked_nothing() {
        let client =
            ConnectClient::new(ConnectSettings::new("connect", "http://connect:8083")).unwrap();
        assert_eq!(client.requests(), 0);
        assert!(matches!(*client.health(), ConnectHealth::Unprobed));
    }

    #[test]
    fn a_missing_ca_file_fails_at_build_naming_the_path() {
        let error = ConnectClient::new(
            ConnectSettings::new("connect", "https://connect:8083").with_tls(
                crate::settings::ConnectTls {
                    ca_file: Some("/nonexistent/ca.crt".into()),
                    client: None,
                },
            ),
        )
        .unwrap_err();
        assert!(error.to_string().contains("/nonexistent/ca.crt"), "{error}");
    }
}

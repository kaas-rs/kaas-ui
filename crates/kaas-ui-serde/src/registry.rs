//! The schema registry client: one per registry, shared by every cluster that
//! names it.
//!
//! Three properties this module exists to hold:
//!
//! 1. **Sharing is construction, not a cache someone keyed correctly.** A
//!    schema id is unique *within* a registry, and the id→schema caches live
//!    inside the decoders. One [`RegistryHandle`] per configured registry
//!    therefore makes both mistakes unrepresentable: there is no way to build
//!    a second cache for `dev`, and no way to hand `dev`'s decoder `prod`'s
//!    settings.
//! 2. **Connecting is lazy, and backing off is per registry.** Nothing is
//!    dialled at startup, so an unreachable registry delays neither the
//!    process nor `/health`. Ten clusters sharing `dev` share one backoff
//!    schedule, because they share the handle that holds it.
//! 3. **ccompat or a configuration error.** The Confluent API is the supported
//!    one; Apicurio's native `/apis/registry/v3` is not. A `url` pointing at
//!    it is caught on first use and reported as configuration, because the
//!    failure to design against is a deployment where every record on every
//!    Avro topic renders as hex and the cause is one missing path segment.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use arc_swap::ArcSwap;
use schema_registry_converter::async_impl::avro::AvroDecoder;
use schema_registry_converter::async_impl::json::{JsonDecoder, validate};
use schema_registry_converter::async_impl::proto_decoder::ProtoDecoder;
use schema_registry_converter::async_impl::schema_registry::{SrSettings, get_schema_by_id};
use schema_registry_converter::error::SRCError;
use schema_registry_converter::schema_registry_common::SchemaType;
use serde::Serialize;
use tokio::sync::Mutex;
use utoipa::ToSchema;

use crate::codec::{Codec, NoteKind, Payload, PayloadNote, SchemaFormat, SchemaRef};
use crate::proto;
use crate::subjects::{CachedVersions, SubjectSchema};

/// Retry floor and ceiling for a registry that will not answer.
const PROBE_BACKOFF_MIN: Duration = Duration::from_secs(1);
const PROBE_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// The default per-call timeout. A schema fetch sits on a request path, so it
/// may not be allowed to sit there long.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a subject listing is believed.
///
/// Schemas are immutable and cached by id forever; *listings* are not — a
/// subject registered a moment ago has to become visible without a restart.
const DEFAULT_SUBJECTS_TTL: Duration = Duration::from_secs(30);

/// What the ccompat API answers with, and what we ask for.
const CCOMPAT_MEDIA_TYPE: &str = "application/vnd.schemaregistry.v1+json";

/// How to reach one registry.
///
/// Required data in [`RegistrySettings::new`], everything optional through a
/// consuming `with_*` builder — `STYLE.md` rule 8, no exceptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySettings {
    /// The configured id. Appears in every payload this registry decoded, so
    /// a reader can tell which registry answered.
    pub id: String,
    /// The name to render. Defaults to the id.
    pub name: String,
    /// The ccompat base url, as in `http://host:8080/apis/ccompat/v7`.
    pub url: String,
    /// Credentials, where the registry wants them.
    pub auth: Option<RegistryAuth>,
    /// Per-call timeout.
    pub request_timeout: Duration,
    /// How long a subject listing is believed.
    pub subjects_ttl: Duration,
}

impl RegistrySettings {
    /// A registry at `url`, known by `id`.
    #[must_use]
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            url: url.into(),
            auth: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            subjects_ttl: DEFAULT_SUBJECTS_TTL,
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
    pub fn with_auth(mut self, auth: RegistryAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Per-call timeout.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// How long a subject listing is believed.
    #[must_use]
    pub fn with_subjects_ttl(mut self, ttl: Duration) -> Self {
        self.subjects_ttl = ttl;
        self
    }

    /// The base url with any trailing slash removed, which is what every
    /// call joins a path onto.
    fn base(&self) -> &str {
        self.url.trim_end_matches('/')
    }
}

/// How a registry wants to be authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryAuth {
    /// HTTP basic. On Confluent Cloud the username is the API key.
    Basic {
        /// The principal.
        username: String,
        /// The secret, where there is one.
        password: Option<String>,
    },
    /// A bearer token, sent verbatim.
    Bearer(String),
}

/// Why a registry is not usable right now.
///
/// Two causes, never conflated: one is somebody else's outage and heals on
/// its own, the other is a line in the configuration file and does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryFault {
    /// It could not be reached, or answered with a server error.
    Unreachable(String),
    /// It answered, and is not speaking the Confluent API — or refused our
    /// credentials.
    Misconfigured(String),
}

impl RegistryFault {
    /// Which note a payload degraded by this fault carries.
    #[must_use]
    pub fn note_kind(&self) -> NoteKind {
        match self {
            Self::Unreachable(_) => NoteKind::RegistryUnavailable,
            Self::Misconfigured(_) => NoteKind::RegistryMisconfigured,
        }
    }

    /// The sentence to render.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Unreachable(message) | Self::Misconfigured(message) => message,
        }
    }
}

impl std::fmt::Display for RegistryFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Where one registry is in its lifecycle.
///
/// Shaped like [`kaas_ui_core::ClusterHealth`](../../kaas_ui_core/health) and
/// for the same reason: a failure lands here and stops, because a registry
/// that is down degrades the pages that need it and blanks none of them.
#[derive(Debug, Clone)]
pub enum RegistryHealth {
    /// Nothing has needed it yet. Connecting is lazy, so this is what a
    /// registry looks like until the first record on a framed topic.
    Unprobed,
    /// It answered the Confluent API.
    Ready {
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
    /// It answered, and is not the API this build speaks. A configuration
    /// fault: it will still be wrong after the next retry.
    Misconfigured {
        /// What is wrong, and what was expected instead.
        error: String,
        /// Since when.
        since: SystemTime,
    },
}

impl RegistryHealth {
    /// The wire form.
    #[must_use]
    pub fn status(&self) -> RegistryStatus {
        match self {
            Self::Unprobed => RegistryStatus::Unprobed,
            Self::Ready { .. } => RegistryStatus::Ready,
            Self::Unreachable { .. } => RegistryStatus::Unreachable,
            Self::Misconfigured { .. } => RegistryStatus::Misconfigured,
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
}

/// The registry health state, flattened for the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RegistryStatus {
    /// Nothing has needed it yet.
    Unprobed,
    /// It answers the Confluent API.
    Ready,
    /// It could not be reached. Retried on one schedule for the registry.
    Unreachable,
    /// It answered, and is not a Confluent endpoint.
    Misconfigured,
}

/// Why a registry could not be built at all.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The HTTP client would not build, or the url is not one.
    #[error("schema registry {id}: {message}")]
    Client {
        /// The configured id.
        id: String,
        /// What went wrong.
        message: String,
    },
}

/// What the registry says a schema id is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SchemaInfo {
    format: SchemaFormat,
    subject: Option<String>,
    version: Option<u32>,
}

/// A subject listing, and when it was fetched.
#[derive(Debug, Clone)]
struct CachedSubjects {
    subjects: Arc<Vec<String>>,
    at: Instant,
}

/// What the last probe concluded, and when.
#[derive(Debug)]
struct Probe {
    outcome: Option<Result<(), RegistryFault>>,
    at: Option<Instant>,
    attempts: u32,
}

/// One schema registry, shared by every cluster that references its id.
#[derive(Debug)]
pub struct RegistryHandle {
    settings: RegistrySettings,
    http: reqwest::Client,
    sr: SrSettings,
    avro: AvroDecoder<'static>,
    json: JsonDecoder<'static>,
    proto: ProtoDecoder<'static>,
    /// What the registry said each schema id is. Populated once per id and
    /// never invalidated, because schemas are immutable.
    types: RwLock<HashMap<u32, Arc<SchemaInfo>>>,
    subjects: RwLock<Option<CachedSubjects>>,
    /// Version listings, by subject. Expire with the subject list.
    versions: RwLock<HashMap<String, CachedVersions>>,
    /// Schema text, by `(subject, version)`. Never expires — a registered
    /// version is immutable, which is the same property the id cache rests on.
    schemas: RwLock<HashMap<(String, u32), Arc<SubjectSchema>>>,
    health: ArcSwap<RegistryHealth>,
    probe: Mutex<Probe>,
    requests: AtomicU64,
}

/// Make sure rustls has a crypto provider before the first client is built.
///
/// `reqwest` is compiled with `rustls-no-provider`, which is what keeps
/// `aws-lc-rs` — and therefore cmake — out of the musl builder. "No provider"
/// obliges *somebody* to install one, and the crate that needs TLS is the
/// right somebody: a `main` that forgot would produce a registry that works
/// over HTTP and fails to build a client over HTTPS.
///
/// `install_default` fails when a provider is already installed, which is a
/// success for our purposes: something else got there first, and one process
/// only ever needs one.
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl RegistryHandle {
    /// Build the client for one configured registry. Dials nothing.
    pub fn new(settings: RegistrySettings) -> Result<Self, RegistryError> {
        install_crypto_provider();

        let fail = |message: String| RegistryError::Client {
            id: settings.id.clone(),
            message,
        };

        let mut sr = SrSettings::new_builder(settings.base().to_owned());
        sr.set_timeout(settings.request_timeout);
        match &settings.auth {
            Some(RegistryAuth::Basic { username, password }) => {
                sr.set_basic_authorization(username, password.as_deref());
            }
            Some(RegistryAuth::Bearer(token)) => {
                sr.set_token_authorization(token);
            }
            None => {}
        }
        let sr = sr.build().map_err(|e| fail(e.to_string()))?;

        let http = reqwest::Client::builder()
            .timeout(settings.request_timeout)
            .build()
            .map_err(|e| fail(e.to_string()))?;

        Ok(Self {
            avro: AvroDecoder::new(sr.clone()),
            json: JsonDecoder::new(sr.clone()),
            proto: ProtoDecoder::new(sr.clone()),
            sr,
            http,
            settings,
            types: RwLock::new(HashMap::new()),
            subjects: RwLock::new(None),
            versions: RwLock::new(HashMap::new()),
            schemas: RwLock::new(HashMap::new()),
            health: ArcSwap::from_pointee(RegistryHealth::Unprobed),
            probe: Mutex::new(Probe {
                outcome: None,
                at: None,
                attempts: 0,
            }),
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
    pub fn settings(&self) -> &RegistrySettings {
        &self.settings
    }

    /// The current state.
    #[must_use]
    pub fn health(&self) -> Arc<RegistryHealth> {
        self.health.load_full()
    }

    /// How many times this process has asked this registry a question.
    ///
    /// Counts the calls kaas-ui makes: the ccompat probe, the browser's
    /// listings, and one schema resolution per **id** — not per record, which
    /// is the whole point of sharing the handle. It is what the acceptance
    /// asserts on when it claims a second cluster decoded a record carrying an
    /// already-resolved id with zero registry requests.
    #[must_use]
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    /// Whether the registry has been shown to speak ccompat.
    ///
    /// Probes if nothing has yet, or if the last failure's backoff has
    /// elapsed. Concurrent callers share the one attempt — ten clusters
    /// naming this registry are ten callers of this function, not ten
    /// schedules.
    pub async fn ready(&self) -> Result<(), RegistryFault> {
        if matches!(**self.health.load(), RegistryHealth::Ready { .. }) {
            return Ok(());
        }

        let mut probe = self.probe.lock().await;
        if let (Some(outcome), Some(at)) = (probe.outcome.clone(), probe.at) {
            let due = match &outcome {
                Ok(()) => return Ok(()),
                Err(RegistryFault::Unreachable(_)) => backoff(probe.attempts),
                // A url pointing at the wrong API does not heal on its own, so
                // it is retried at the ceiling rather than never: a 404 from a
                // registry that is still starting up must not become permanent.
                Err(RegistryFault::Misconfigured(_)) => PROBE_BACKOFF_MAX,
            };
            if at.elapsed() < due {
                return outcome;
            }
        }

        let outcome = self.run_probe().await;
        probe.at = Some(Instant::now());
        match &outcome {
            Ok(()) => {
                probe.attempts = 0;
                self.health.store(Arc::new(RegistryHealth::Ready {
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

    /// Decode a Confluent-framed payload.
    ///
    /// `bytes` is the whole record — framing included — because the framing is
    /// what the reader is shown as `raw`, and because a decoded payload has to
    /// be convertible back to hex without a refetch.
    pub async fn decode(&self, id: u32, bytes: &[u8], ceiling: usize) -> Decoded {
        if let Err(fault) = self.ready().await {
            return Decoded::degraded(bytes, ceiling, self.fault_note(&fault));
        }

        let info = match self.schema_info(id).await {
            Ok(info) => info,
            Err(error) => return self.decode_failure(bytes, ceiling, id, &error),
        };

        let schema = SchemaRef {
            id,
            format: info.format,
            registry: self.settings.id.clone(),
            subject: info.subject.clone(),
            version: info.version,
            name: None,
        };

        match info.format {
            SchemaFormat::Avro => self.decode_avro(bytes, ceiling, schema).await,
            SchemaFormat::Json => self.decode_json(bytes, ceiling, schema).await,
            SchemaFormat::Protobuf => self.decode_protobuf(bytes, ceiling, schema).await,
        }
    }

    async fn decode_avro(&self, bytes: &[u8], ceiling: usize, mut schema: SchemaRef) -> Decoded {
        match self.avro.decode_with_schema(Some(bytes)).await {
            Ok(Some(result)) => {
                schema.name = result.name.map(|name| name.fullname(None));
                if schema.subject.is_none() {
                    schema.subject.clone_from(&result.schema.subject);
                }
                if schema.version.is_none() {
                    schema.version = result.schema.version;
                }
                match serde_json::Value::try_from(result.value) {
                    Ok(value) => Decoded::of(value, bytes, Codec::Avro, schema, ceiling),
                    Err(error) => self.payload_error(
                        bytes,
                        ceiling,
                        schema,
                        format!("decoded, but the value has no JSON rendering: {error}"),
                    ),
                }
            }
            // `None` is a null payload, which the caller already handled: a
            // tombstone never reaches a decoder.
            Ok(None) => Decoded::of(serde_json::Value::Null, bytes, Codec::Avro, schema, ceiling),
            Err(error) => self.decoder_error(bytes, ceiling, schema, &error),
        }
    }

    async fn decode_json(&self, bytes: &[u8], ceiling: usize, mut schema: SchemaRef) -> Decoded {
        match self.json.decode(Some(bytes)).await {
            Ok(Some(result)) => {
                if schema.subject.is_none() {
                    schema.subject.clone_from(&result.schema.subject);
                }
                if schema.version.is_none() {
                    schema.version = result.schema.version;
                }
                // Parsing and conforming are different questions, and a record
                // that answers yes to the first and no to the second is
                // exactly the one worth pointing at.
                let violation = validate(result.schema.clone(), &result.value).err();
                let decoded = Decoded::of(result.value, bytes, Codec::JsonSchema, schema, ceiling);
                match violation {
                    None => decoded,
                    Some(error) => decoded.with_note(PayloadNote::new(
                        NoteKind::NonConforming,
                        format!(
                            "this record parses, and does not satisfy the schema it names: {}",
                            error.error
                        ),
                    )),
                }
            }
            Ok(None) => Decoded::of(
                serde_json::Value::Null,
                bytes,
                Codec::JsonSchema,
                schema,
                ceiling,
            ),
            Err(error) => self.decoder_error(bytes, ceiling, schema, &error),
        }
    }

    async fn decode_protobuf(
        &self,
        bytes: &[u8],
        ceiling: usize,
        mut schema: SchemaRef,
    ) -> Decoded {
        match self.proto.decode_with_context(Some(bytes)).await {
            Ok(Some(result)) => {
                schema.name = Some(result.full_name.to_string());
                let value = proto::to_json(&result.value, &result.context.context);
                Decoded::of(value, bytes, Codec::Protobuf, schema, ceiling)
            }
            Ok(None) => Decoded::of(
                serde_json::Value::Null,
                bytes,
                Codec::Protobuf,
                schema,
                ceiling,
            ),
            Err(error) => self.decoder_error(bytes, ceiling, schema, &error),
        }
    }

    /// What the registry says schema `id` is, resolved once per id.
    async fn schema_info(&self, id: u32) -> Result<Arc<SchemaInfo>, SRCError> {
        if let Some(info) = self.types.read().ok().and_then(|c| c.get(&id).cloned()) {
            return Ok(info);
        }

        self.requests.fetch_add(1, Ordering::Relaxed);
        let registered = get_schema_by_id(id, &self.sr).await.inspect_err(|error| {
            if error.retriable {
                self.went_away(error);
            }
        })?;

        let format = match registered.schema_type {
            SchemaType::Avro => SchemaFormat::Avro,
            SchemaType::Protobuf => SchemaFormat::Protobuf,
            SchemaType::Json => SchemaFormat::Json,
            SchemaType::Other(ref other) => {
                return Err(SRCError::non_retryable_without_cause(&format!(
                    "the registry says schema id {id} is {other:?}, which is not one of the three \
                     formats the Confluent wire format carries"
                )));
            }
        };

        // Confluent returns the subject and version with the schema; Apicurio
        // does not, and a payload chip reading "schema 1" with no subject is
        // most of the information missing. `GET /schemas/ids/{id}/versions` is
        // the ccompat way to ask, and it costs one request per *id* — the
        // answer is cached beside the format and never fetched again, because
        // a schema id does not change what it is registered as.
        let (subject, version) = match (registered.subject, registered.version) {
            (Some(subject), version) => (Some(subject), version),
            (None, version) => match self.first_subject(id).await {
                Some((subject, subject_version)) => (Some(subject), version.or(subject_version)),
                None => (None, version),
            },
        };

        let info = Arc::new(SchemaInfo {
            format,
            subject,
            version,
        });
        if let Ok(mut cache) = self.types.write() {
            cache.insert(id, Arc::clone(&info));
        }
        Ok(info)
    }

    /// The first subject a schema id is registered against, where the registry
    /// will say.
    ///
    /// "First" rather than "the": one schema can be registered against several
    /// subjects, so this is a label for the reader and not an identity. Best
    /// effort throughout — a registry that does not implement the endpoint
    /// leaves the chip reading `schema 1`, which is what it read before.
    async fn first_subject(&self, id: u32) -> Option<(String, Option<u32>)> {
        #[derive(serde::Deserialize)]
        struct SubjectVersion {
            subject: String,
            version: Option<u32>,
        }

        let response = self
            .get(&format!("/schemas/ids/{id}/versions"))
            .await
            .ok()?;
        let versions: Vec<SubjectVersion> = response.json().await.ok()?;
        versions
            .into_iter()
            .next()
            .map(|found| (found.subject, found.version))
    }

    /// The cached version listing for a subject, however stale.
    pub(crate) fn cached_versions(&self, subject: &str) -> Option<CachedVersions> {
        self.versions.read().ok()?.get(subject).cloned()
    }

    pub(crate) fn store_versions(&self, subject: &str, versions: &Arc<Vec<u32>>) {
        if let Ok(mut cache) = self.versions.write() {
            cache.insert(
                subject.to_owned(),
                CachedVersions {
                    versions: Arc::clone(versions),
                    at: Instant::now(),
                },
            );
        }
    }

    pub(crate) fn cached_schema(&self, subject: &str, version: u32) -> Option<Arc<SubjectSchema>> {
        self.schemas
            .read()
            .ok()?
            .get(&(subject.to_owned(), version))
            .map(Arc::clone)
    }

    pub(crate) fn store_schema(&self, subject: &str, version: u32, schema: &Arc<SubjectSchema>) {
        if let Ok(mut cache) = self.schemas.write() {
            cache.insert((subject.to_owned(), version), Arc::clone(schema));
        }
    }

    /// A GET against the ccompat API, counted.
    pub(crate) async fn get(&self, path: &str) -> Result<reqwest::Response, RegistryFault> {
        let url = format!("{}{path}", self.settings.base());
        let mut request = self
            .http
            .get(&url)
            .header(reqwest::header::ACCEPT, CCOMPAT_MEDIA_TYPE);
        match &self.settings.auth {
            Some(RegistryAuth::Basic { username, password }) => {
                request = request.basic_auth(username, password.as_ref());
            }
            Some(RegistryAuth::Bearer(token)) => {
                request = request.bearer_auth(token);
            }
            None => {}
        }

        self.requests.fetch_add(1, Ordering::Relaxed);
        let response = request.send().await.map_err(|error| {
            let fault = RegistryFault::Unreachable(format!(
                "schema registry {:?} at {} could not be reached: {error}",
                self.settings.id, self.settings.url
            ));
            self.record_fault(&fault, 0);
            fault
        })?;

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        Err(self.status_fault(status, path))
    }

    /// Turn a non-2xx into the right kind of fault.
    fn status_fault(&self, status: reqwest::StatusCode, path: &str) -> RegistryFault {
        let id = &self.settings.id;
        let url = &self.settings.url;
        if status.is_server_error() {
            return RegistryFault::Unreachable(format!(
                "schema registry {id:?} at {url} answered {status}"
            ));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return RegistryFault::Misconfigured(format!(
                "schema registry {id:?} at {url} refused our credentials ({status}): check the \
                 `username`/`password` or `bearer_token` on the `schema_registries` entry"
            ));
        }
        RegistryFault::Misconfigured(format!(
            "schema registry {id:?} answered {status} for {path}, so {url} is not a Confluent \
             (ccompat) endpoint. kaas-ui speaks the Confluent API only — for Apicurio that is \
             `/apis/ccompat/v7`, not `/apis/registry/v3`"
        ))
    }

    /// The ccompat check: `GET /subjects` has to answer a JSON array.
    async fn run_probe(&self) -> Result<(), RegistryFault> {
        self.fetch_subjects().await.map(|_| ())
    }

    /// The subject list, fetched and cached for [`RegistrySettings::subjects_ttl`].
    pub async fn subjects(&self) -> Result<Arc<Vec<String>>, RegistryFault> {
        if let Some(cached) = self.subjects.read().ok().and_then(|c| c.clone())
            && cached.at.elapsed() < self.settings.subjects_ttl
        {
            return Ok(cached.subjects);
        }
        self.fetch_subjects().await
    }

    async fn fetch_subjects(&self) -> Result<Arc<Vec<String>>, RegistryFault> {
        let response = self.get("/subjects").await?;
        let subjects: Vec<String> = response.json().await.map_err(|error| {
            // A 200 whose body is not a subject list is the native API
            // answering something else, or a proxy's login page. Either way it
            // is configuration, not an outage.
            let fault = RegistryFault::Misconfigured(format!(
                "schema registry {:?} at {} answered `GET /subjects` with something that is not a \
                 list of subjects ({error}). kaas-ui speaks the Confluent API only — for Apicurio \
                 that is `/apis/ccompat/v7`, not `/apis/registry/v3`",
                self.settings.id, self.settings.url
            ));
            self.record_fault(&fault, 0);
            fault
        })?;

        let subjects = Arc::new(subjects);
        if let Ok(mut cache) = self.subjects.write() {
            *cache = Some(CachedSubjects {
                subjects: Arc::clone(&subjects),
                at: Instant::now(),
            });
        }
        self.health.store(Arc::new(RegistryHealth::Ready {
            since: SystemTime::now(),
        }));
        Ok(subjects)
    }

    /// Record that the registry stopped answering mid-flight.
    ///
    /// The decoders cache their errors, so a network blip would otherwise
    /// stick to every id it touched. Clearing is deliberately limited to
    /// retriable failures: a schema that genuinely does not exist should stay
    /// cached as missing.
    fn went_away(&self, error: &SRCError) {
        let fault = RegistryFault::Unreachable(format!(
            "schema registry {:?} at {} stopped answering: {}",
            self.settings.id, self.settings.url, error.error
        ));
        self.record_fault(&fault, 0);
        self.avro.remove_errors_from_cache();
        self.json.remove_errors_from_cache();
        self.proto.remove_errors_from_cache();
        // Force the next caller to probe rather than trust a stale `Ready`.
        if let Ok(mut probe) = self.probe.try_lock() {
            probe.outcome = None;
            probe.at = None;
        }
    }

    fn record_fault(&self, fault: &RegistryFault, attempts: u32) {
        let previous = self.health.load();
        let since = match previous.as_ref() {
            RegistryHealth::Unreachable { since, .. }
            | RegistryHealth::Misconfigured { since, .. } => *since,
            _ => SystemTime::now(),
        };
        let health = match fault {
            RegistryFault::Unreachable(error) => RegistryHealth::Unreachable {
                error: error.clone(),
                since,
                attempts: attempts.max(1),
            },
            RegistryFault::Misconfigured(error) => RegistryHealth::Misconfigured {
                error: error.clone(),
                since,
            },
        };
        self.health.store(Arc::new(health));
    }

    fn fault_note(&self, fault: &RegistryFault) -> PayloadNote {
        PayloadNote::new(fault.note_kind(), fault.message())
    }

    /// A framed payload whose schema could not be resolved.
    fn decode_failure(&self, bytes: &[u8], ceiling: usize, id: u32, error: &SRCError) -> Decoded {
        Decoded::degraded(
            bytes,
            ceiling,
            PayloadNote::new(
                NoteKind::DecodeError,
                format!(
                    "schema id {id} could not be resolved against registry {:?}: {}",
                    self.settings.id, error.error
                ),
            ),
        )
    }

    /// A framed payload whose schema resolved and whose bytes did not decode.
    ///
    /// A **payload** error: the record is fine, its value is not. Never the
    /// same row as a batch that would not decode at the protocol level.
    fn decoder_error(
        &self,
        bytes: &[u8],
        ceiling: usize,
        schema: SchemaRef,
        error: &SRCError,
    ) -> Decoded {
        if error.retriable {
            self.went_away(error);
        }
        self.payload_error(bytes, ceiling, schema, error.error.clone())
    }

    fn payload_error(
        &self,
        bytes: &[u8],
        ceiling: usize,
        schema: SchemaRef,
        message: String,
    ) -> Decoded {
        let format = schema.format;
        Decoded {
            payload: Payload::hex(bytes, ceiling)
                .with_schema(schema)
                .with_note(PayloadNote::new(
                    NoteKind::DecodeError,
                    format!("not valid {}: {message}", format.codec().label()),
                )),
            value: None,
        }
    }
}

/// A payload, decoded: what goes on the wire, and what a predicate sees.
///
/// Two fields rather than one because they answer different questions. The
/// wire wants text, cut to a ceiling. A JS predicate wants the value, whole
/// and structured, and it runs before the response exists.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// The rendering that reaches the browser.
    pub payload: Payload,
    /// The decoded value, where there was one. `None` for anything that did
    /// not decode, and for the codecs that produce text rather than structure.
    pub value: Option<serde_json::Value>,
}

impl Decoded {
    /// A decoded value and the bytes it came from.
    #[must_use]
    pub fn of(
        value: serde_json::Value,
        original: &[u8],
        codec: Codec,
        schema: SchemaRef,
        ceiling: usize,
    ) -> Self {
        Self {
            payload: Payload::decoded(&value, original, codec, schema, ceiling),
            value: Some(value),
        }
    }

    /// A payload that could not be decoded, rendered anyway with the reason.
    ///
    /// The records are still there: a registry outage must not empty the
    /// message view, it must make it hex with a sentence saying why.
    ///
    /// Hex rather than [`Payload::auto`], and that is not a detail. These
    /// bytes are framed, so they begin with a zero and four more that are
    /// almost always inside the ASCII range — `auto` would call the whole
    /// record text and render a schema id as two control characters and a
    /// digit.
    #[must_use]
    pub fn degraded(bytes: &[u8], ceiling: usize, note: PayloadNote) -> Self {
        Self {
            payload: Payload::hex(bytes, ceiling).with_note(note),
            value: None,
        }
    }

    /// A payload that needed no registry.
    #[must_use]
    pub fn plain(payload: Payload) -> Self {
        Self {
            payload,
            value: None,
        }
    }

    /// Attach a note.
    #[must_use]
    pub fn with_note(mut self, note: PayloadNote) -> Self {
        self.payload = self.payload.with_note(note);
        self
    }
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
    fn the_backoff_climbs_and_stops() {
        assert_eq!(backoff(0), PROBE_BACKOFF_MIN);
        assert_eq!(backoff(1), PROBE_BACKOFF_MIN);
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(4), Duration::from_secs(8));
        assert_eq!(backoff(20), PROBE_BACKOFF_MAX);
    }

    #[test]
    fn a_url_keeps_its_shape_whatever_was_typed() {
        let settings = RegistrySettings::new("dev", "http://apicurio:8080/apis/ccompat/v7/");
        assert_eq!(settings.base(), "http://apicurio:8080/apis/ccompat/v7");
        // The configured spelling is what gets rendered, so the operator can
        // recognise the line they wrote.
        assert_eq!(settings.url, "http://apicurio:8080/apis/ccompat/v7/");
    }

    #[test]
    fn the_name_defaults_to_the_id() {
        let settings = RegistrySettings::new("dev", "http://apicurio:8080");
        assert_eq!(settings.name, "dev");
        assert_eq!(
            RegistrySettings::new("dev", "http://apicurio:8080")
                .with_name("Apicurio (dev)")
                .name,
            "Apicurio (dev)"
        );
    }

    #[test]
    fn a_new_handle_has_asked_nothing() {
        let handle =
            RegistryHandle::new(RegistrySettings::new("dev", "http://apicurio:8080")).unwrap();
        assert_eq!(handle.requests(), 0);
        assert!(matches!(*handle.health(), RegistryHealth::Unprobed));
    }
}

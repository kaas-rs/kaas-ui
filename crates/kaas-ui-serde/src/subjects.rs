//! The schema browser's half of the registry: subjects, versions, text.
//!
//! `schema_registry_converter` resolves schema ids, which is what decoding
//! needs and all it needs. Listing what a registry *holds* is a different set
//! of ccompat calls and this module is them, on the same client, behind the
//! same counter and the same backoff.
//!
//! Two caches, with deliberately different lifetimes:
//!
//! * **Schema text is cached by `(subject, version)` forever.** A registered
//!   version is immutable; that is the property the whole id cache rests on.
//! * **Listings are cached briefly.** A subject registered a moment ago has to
//!   appear without a restart, so the version list expires with the subject
//!   list — see [`RegistrySettings::subjects_ttl`](crate::RegistrySettings).

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::codec::SchemaFormat;
use crate::registry::{RegistryFault, RegistryHandle};

/// One registered version of a subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectSchema {
    /// The subject it is registered against.
    pub subject: String,
    /// The version within that subject.
    pub version: u32,
    /// The global schema id — the number the wire format carries.
    pub id: u32,
    /// Which of the three formats.
    pub format: SchemaFormat,
    /// The schema itself, as the registry stores it. Text rather than parsed,
    /// because this is what Monaco highlights and what a diff is taken over.
    pub schema: String,
    /// Subjects this one refers to.
    pub references: Vec<SchemaReference>,
}

/// A schema this one refers to, stored separately by the registry.
///
/// A resolver that fetches only the id in the payload decodes the simple
/// topics and fails on the interesting ones — so these are shown, and they are
/// followed when decoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaReference {
    /// The name the referring schema uses — a fully-qualified Avro name, or a
    /// `.proto` import path.
    pub name: String,
    /// The subject the referenced schema is registered against.
    pub subject: String,
    /// Its version.
    pub version: u32,
}

/// What the registry answered for one subject and version.
#[derive(Debug, Deserialize)]
struct RawSubjectSchema {
    subject: Option<String>,
    version: Option<u32>,
    id: Option<u32>,
    #[serde(rename = "schemaType")]
    schema_type: Option<String>,
    schema: Option<String>,
    #[serde(default)]
    references: Vec<RawReference>,
}

#[derive(Debug, Deserialize)]
struct RawReference {
    name: Option<String>,
    subject: Option<String>,
    version: Option<u32>,
}

/// A version listing, and when it was fetched.
#[derive(Debug, Clone)]
pub(crate) struct CachedVersions {
    pub(crate) versions: Arc<Vec<u32>>,
    pub(crate) at: Instant,
}

impl RegistryHandle {
    /// The versions registered against a subject, oldest first.
    pub async fn versions(&self, subject: &str) -> Result<Arc<Vec<u32>>, RegistryFault> {
        if let Some(cached) = self.cached_versions(subject)
            && cached.at.elapsed() < self.settings().subjects_ttl
        {
            return Ok(cached.versions);
        }

        let response = self
            .get(&format!("/subjects/{}/versions", encode(subject)))
            .await?;
        let mut versions: Vec<u32> = response.json().await.map_err(|error| {
            RegistryFault::Misconfigured(format!(
                "schema registry {:?} answered the versions of subject {subject:?} with something \
                 that is not a list of version numbers: {error}",
                self.id()
            ))
        })?;
        versions.sort_unstable();

        let versions = Arc::new(versions);
        self.store_versions(subject, &versions);
        Ok(versions)
    }

    /// One registered version, text and references included.
    ///
    /// Cached forever once fetched: a registered version cannot change, and if
    /// it could the id cache underneath decoding would already be wrong.
    pub async fn schema(
        &self,
        subject: &str,
        version: u32,
    ) -> Result<Arc<SubjectSchema>, RegistryFault> {
        if let Some(cached) = self.cached_schema(subject, version) {
            return Ok(cached);
        }

        let response = self
            .get(&format!("/subjects/{}/versions/{version}", encode(subject)))
            .await?;
        let raw: RawSubjectSchema = response.json().await.map_err(|error| {
            RegistryFault::Misconfigured(format!(
                "schema registry {:?} answered subject {subject:?} version {version} with \
                 something that is not a registered schema: {error}",
                self.id()
            ))
        })?;

        let schema = Arc::new(SubjectSchema {
            subject: raw.subject.unwrap_or_else(|| subject.to_owned()),
            version: raw.version.unwrap_or(version),
            id: raw.id.unwrap_or_default(),
            // Confluent omits `schemaType` for Avro, which is the default
            // rather than an absence.
            format: match raw.schema_type.as_deref() {
                Some("PROTOBUF") => SchemaFormat::Protobuf,
                Some("JSON") => SchemaFormat::Json,
                _ => SchemaFormat::Avro,
            },
            schema: raw.schema.unwrap_or_default(),
            references: raw
                .references
                .into_iter()
                .filter_map(|reference| {
                    Some(SchemaReference {
                        name: reference.name?,
                        subject: reference.subject?,
                        version: reference.version?,
                    })
                })
                .collect(),
        });

        self.store_schema(subject, version, &schema);
        Ok(schema)
    }

    /// The compatibility mode the registry reports for a subject.
    ///
    /// Best effort, and `None` where the registry does not say: a subject with
    /// no override of its own is a 404 on Confluent and on Apicurio alike, and
    /// that is an answer rather than a failure. Not cached — it is one call per
    /// subject page, and it is the one thing here that can change without a
    /// new version appearing.
    pub async fn compatibility(&self, subject: &str) -> Option<String> {
        self.config_at(&format!("/config/{}", encode(subject)))
            .await
    }

    /// The registry-wide compatibility mode.
    ///
    /// What a subject with no override of its own is actually governed by, and
    /// therefore the only way a compatibility column can say anything on the
    /// common registry where nobody has set a per-subject rule. A column that
    /// is blank on every row because the answer lives one endpoint over is a
    /// column that has taught the reader nothing.
    pub async fn global_compatibility(&self) -> Option<String> {
        self.config_at("/config").await
    }

    async fn config_at(&self, path: &str) -> Option<String> {
        #[derive(Deserialize)]
        struct Config {
            #[serde(rename = "compatibilityLevel")]
            compatibility_level: Option<String>,
        }

        let response = self.get(path).await.ok()?;
        let config: Config = response.json().await.ok()?;
        config.compatibility_level
    }
}

/// Percent-encode one path segment.
///
/// Subjects are producer-chosen strings and reach this as path segments. A
/// subject containing `/` — or a space, which Kafka topic names forbid and
/// subject names do not — would otherwise change which endpoint was called.
fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            other => {
                use std::fmt::Write as _;
                // Writing into a String cannot fail.
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subject_is_encoded_as_one_path_segment() {
        assert_eq!(encode("kaas-canary-v1-value"), "kaas-canary-v1-value");
        // The two that would otherwise change which endpoint is called.
        assert_eq!(encode("a/b"), "a%2Fb");
        assert_eq!(encode("with space"), "with%20space");
        assert_eq!(encode("héllo"), "h%C3%A9llo");
    }
}

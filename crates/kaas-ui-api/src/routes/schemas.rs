//! The schema browser.
//!
//! Rooted at `/api/environments/{env}/schema-registries/{registry}`, which it
//! was not: it used to hang off `/api/clusters/{id}/schemas`, because a
//! registry id as a top-level namespace would have been enumerable and "which
//! clusters use this registry" is a list that can name a cluster the caller
//! may not see.
//!
//! Nesting settles that without the indirection. A registry id is scoped to
//! an environment, an environment is reachable only when the caller can see a
//! cluster in it, and `AppState::schema_registry` additionally requires that
//! they can see a cluster *referencing this registry* — so the URL says what
//! the thing is instead of routing through a cluster that merely knows it,
//! and it still cannot be probed.
//!
//! Three things that are **not** errors here, because each of them is an
//! ordinary state of a healthy deployment:
//!
//! * an environment with no registry at all — that is most of them;
//! * a registry that cannot be reached — the environment is degraded, and the
//!   card says so rather than the page failing;
//! * a registry answering the wrong API — a configuration fault, reported as
//!   one, on the card.
//!
//! No cluster has to be connected. A registry serves an environment and knows
//! nothing about brokers, so subjects stay browsable while every cluster
//! beside it is down.

use axum::Json;
use axum::extract::{Path, Query, State};
use futures::StreamExt;
use kaas_ui_core::dto::RegistryCard;
use kaas_ui_serde::{SchemaFormat, SubjectSchema};
use serde::Serialize;
use utoipa::ToSchema;

use crate::{ApiResult, AppState, Caller};

/// The subjects one registry holds, and which registry that is.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectList {
    /// The registry answering, or `None` where this cluster references none.
    pub registry: Option<RegistryCard>,
    /// The page of subjects, filtered and ordered as asked.
    pub subjects: Vec<SubjectRow>,
    /// How many subjects matched before paging.
    pub total: usize,
    /// The registry-wide compatibility mode, when `details` was asked for.
    ///
    /// What every row without an override of its own is governed by, so the
    /// table can say `BACKWARD` on a registry where nobody has set a
    /// per-subject rule instead of a column of blanks.
    pub compatibility: Option<String>,
}

/// One row of the subject table.
///
/// Everything but the name is `Option`, and for one reason: the name is in the
/// listing the registry already gave us, and the rest is a call per subject.
/// The client asks for the page first and the columns second.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectRow {
    /// The subject name.
    pub subject: String,
    /// The global schema id of the newest version — the number the wire
    /// format carries.
    pub id: Option<u32>,
    /// Which of the three formats the newest version is.
    pub format: Option<SchemaFormat>,
    /// The newest version number.
    pub version: Option<u32>,
    /// The compatibility mode governing this subject.
    pub compatibility: Option<String>,
    /// Whether that mode is the registry's rather than this subject's own.
    ///
    /// Two subjects reading `BACKWARD` are not the same fact if one of them
    /// set it and the other is inheriting it — the second changes when the
    /// registry default does.
    pub compatibility_inherited: bool,
}

/// How much of the subject table to fill in.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubjectQuery {
    /// Case-insensitive substring match on the subject name.
    pub search: Option<String>,
    /// `asc` or `desc`, by name. The only orderable column, because every
    /// other one would have to be fetched for every subject in the registry
    /// before the first row could be placed.
    pub order: Option<String>,
    /// Page size.
    pub limit: Option<usize>,
    /// Page offset.
    pub offset: Option<usize>,
    /// Fetch id, format, version and compatibility for the returned page.
    ///
    /// Opt-in, and page-scoped, because it is two registry calls per row —
    /// unlike the topic list's fan-out this cost scales with the number of
    /// *rows*, so a registry with five hundred subjects would otherwise spend
    /// a thousand calls to render fifty.
    #[serde(default)]
    pub details: bool,
}

/// The versions of one subject, newest last.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectDetail {
    /// The registry answering.
    pub registry: Option<RegistryCard>,
    /// The subject that was asked for.
    pub subject: String,
    /// The compatibility mode, where the registry reports one.
    pub compatibility: Option<String>,
    /// Every registered version, oldest first, with its text.
    pub versions: Vec<SubjectSchema>,
    /// Versions the registry listed and would not hand over, with why.
    ///
    /// Partial failure is a result: a subject with thirty versions of which
    /// one is unreadable renders twenty-nine and names the thirtieth.
    pub errors: Vec<kaas_ui_core::ResourceError>,
}

/// `GET /api/environments/{env}/schema-registries/{registry}/subjects`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/schema-registries/{registry}/subjects",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("registry" = String, Path, description = "Schema registry id"),
        ("search" = Option<String>, Query, description = "Substring match on the subject"),
        ("order" = Option<String>, Query, description = "asc | desc, by name"),
        ("limit" = Option<usize>, Query, description = "Page size"),
        ("offset" = Option<usize>, Query, description = "Page offset"),
        ("details" = Option<bool>, Query, description = "Fetch id, format, version and compatibility"),
    ),
    responses((status = 200, description = "The subjects of the registry this cluster uses", body = SubjectList)),
    tag = "schemas",
)]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Query(query): Query<SubjectQuery>,
) -> ApiResult<Json<SubjectList>> {
    // The lookup carries the whole visibility decision — it answers only for a
    // caller who can see a cluster in this environment that references this
    // registry — so there is no second guard to forget here. Absence is a 404,
    // never a 403, exactly as for a cluster.
    let registry = state.schema_registry(&env, &id, &caller)?;
    let registry = registry.as_ref();

    // A fault is rendered, never raised: the card carries the state and the
    // list is empty, which is the same shape a healthy empty registry has and
    // is distinguished by the card rather than by the status code.
    let subjects = registry.subjects().await.unwrap_or_default();

    let mut names: Vec<&String> = match query.search.as_deref().map(str::trim) {
        Some(needle) if !needle.is_empty() => {
            let needle = needle.to_lowercase();
            subjects
                .iter()
                .filter(|subject| subject.to_lowercase().contains(&needle))
                .collect()
        }
        _ => subjects.iter().collect(),
    };
    names.sort();
    if query.order.as_deref() == Some("desc") {
        names.reverse();
    }

    let total = names.len();
    let offset = query.offset.unwrap_or(0).min(total);
    let limit = query.limit.unwrap_or(total);
    let page: Vec<String> = names
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();

    if !query.details {
        return Ok(Json(SubjectList {
            registry: Some(RegistryCard::of(registry)),
            subjects: page.into_iter().map(SubjectRow::of).collect(),
            total,
            compatibility: None,
        }));
    }

    // Concurrently, because these are independent HTTP calls to one registry
    // and doing fifty of them in a row is fifty round trips end to end. Capped
    // rather than unbounded: a registry is a single service, and a page of
    // subjects arriving as one burst is how a browse turns into an outage for
    // whatever else is decoding against it.
    let global = registry.global_compatibility().await;
    let rows: Vec<SubjectRow> = futures::stream::iter(page)
        .map(|subject| {
            let global = global.clone();
            async move { SubjectRow::describe(registry, subject, global).await }
        })
        .buffered(REGISTRY_CONCURRENCY)
        .collect()
        .await;

    Ok(Json(SubjectList {
        registry: Some(RegistryCard::of(registry)),
        subjects: rows,
        total,
        compatibility: global,
    }))
}

/// How many subjects to describe at once.
///
/// A registry is one service that other requests are decoding against. Eight
/// is enough that a fifty-row page is six round trips rather than fifty, and
/// few enough that browsing does not become that service's load problem.
const REGISTRY_CONCURRENCY: usize = 8;

impl SubjectRow {
    /// The name alone, which is all the listing gives.
    fn of(subject: String) -> Self {
        Self {
            subject,
            id: None,
            format: None,
            version: None,
            compatibility: None,
            compatibility_inherited: false,
        }
    }

    /// The name plus what the newest version and the config say.
    ///
    /// A subject that cannot be described still returns its row. It exists —
    /// the registry just listed it — and dropping it would make a failing
    /// registry look like a shrinking one.
    async fn describe(
        registry: &kaas_ui_serde::RegistryHandle,
        subject: String,
        global: Option<String>,
    ) -> Self {
        let mut row = Self::of(subject);

        // `versions` is cached for the listing's lifetime and `schema` forever,
        // so a second page visit costs the config call and nothing else.
        if let Ok(versions) = registry.versions(&row.subject).await
            && let Some(newest) = versions.last().copied()
        {
            row.version = Some(newest);
            if let Ok(schema) = registry.schema(&row.subject, newest).await {
                row.id = Some(schema.id);
                row.format = Some(schema.format);
                // The registry is free to disagree with the version we asked
                // for; believe what it labelled the answer with.
                row.version = Some(schema.version);
            }
        }

        match registry.compatibility(&row.subject).await {
            Some(own) => row.compatibility = Some(own),
            None => {
                row.compatibility = global;
                row.compatibility_inherited = row.compatibility.is_some();
            }
        }

        row
    }
}

/// `GET /api/environments/{env}/schema-registries/{registry}/subjects/{subject}/versions`
#[utoipa::path(
    get,
    path = "/api/environments/{env}/schema-registries/{registry}/subjects/{subject}/versions",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("registry" = String, Path, description = "Schema registry id"),
        ("subject" = String, Path, description = "Subject name"),
    ),
    responses((status = 200, description = "Every registered version of a subject", body = SubjectDetail)),
    tag = "schemas",
)]
pub async fn versions(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, subject)): Path<(String, String, String)>,
) -> ApiResult<Json<SubjectDetail>> {
    let registry = state.schema_registry(&env, &id, &caller)?;
    let registry = registry.as_ref();

    let card = RegistryCard::of(registry);
    let listed = match registry.versions(&subject).await {
        Ok(versions) => versions,
        // Both "this subject does not exist" and "the registry is down" land
        // here, and the card is what tells them apart: a `ready` registry with
        // an empty subject means the subject is gone.
        Err(_) => {
            return Ok(Json(SubjectDetail {
                registry: Some(RegistryCard::of(registry)),
                subject,
                compatibility: None,
                versions: Vec::new(),
                errors: Vec::new(),
            }));
        }
    };

    let mut versions = Vec::with_capacity(listed.len());
    let mut errors = Vec::new();
    for version in listed.iter() {
        match registry.schema(&subject, *version).await {
            Ok(schema) => versions.push(schema.as_ref().clone()),
            Err(fault) => errors.push(kaas_ui_core::ResourceError {
                resource: format!("{subject}/{version}"),
                kind: kaas_ui_core::ErrorKind::Transport,
                code: None,
                code_number: None,
                message: fault.message().to_owned(),
                unsupported_api: None,
                retriable: true,
            }),
        }
    }

    Ok(Json(SubjectDetail {
        registry: Some(card),
        compatibility: registry.compatibility(&subject).await,
        subject,
        versions,
        errors,
    }))
}

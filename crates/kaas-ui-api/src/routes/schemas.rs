//! The schema browser.
//!
//! Rooted at `/api/clusters/{id}/schemas`, and there is deliberately no
//! `/api/schema-registries/{id}`: registry ids would then be a second
//! enumerable namespace beside cluster ids, and "which clusters use this
//! registry" is a list that can name a cluster the caller may not see. A
//! caller reaches a registry only through a cluster they can already see, and
//! the lookup that decides that is the same one every other route uses.
//!
//! Three things that are **not** errors here, because each of them is an
//! ordinary state of a healthy deployment:
//!
//! * a cluster that references no registry — that is `kaas`, and it is the
//!   common case;
//! * a registry that cannot be reached — the environment is degraded, and the
//!   card says so rather than the page failing;
//! * a registry answering the wrong API — a configuration fault, reported as
//!   one, on the card.
//!
//! The cluster does **not** have to be connected. A registry serves an
//! environment and knows nothing about brokers, so schemas stay browsable
//! while the cluster whose nav you arrived through is down.

use axum::Json;
use axum::extract::{Path, State};
use kaas_ui_core::dto::RegistryCard;
use kaas_ui_serde::SubjectSchema;
use serde::Serialize;
use utoipa::ToSchema;

use kaas_ui_auth::{Action, Resource};

use crate::{ApiError, ApiResult, AppState, Caller};

/// The subjects one registry holds, and which registry that is.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectList {
    /// The registry answering, or `None` where this cluster references none.
    pub registry: Option<RegistryCard>,
    /// Every subject, in the order the registry listed them.
    pub subjects: Vec<String>,
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

/// `GET /api/clusters/{id}/schemas`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/schemas",
    params(("id" = String, Path, description = "Cluster id")),
    responses((status = 200, description = "The subjects of the registry this cluster uses", body = SubjectList)),
    tag = "schemas",
)]
pub async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> ApiResult<Json<SubjectList>> {
    // `cluster`, not `connected`: a registry has no brokers in it, so a
    // cluster that is down does not make its environment's schemas
    // unreadable.
    let handle = state.cluster(&id, &caller)?;
    // Guarded exactly like the topic list. Subject names are metadata of the
    // same kind as topic names, and a schema is a description rather than a
    // payload — `messages_read` is spent on records, not on their shape.
    caller.require(&id, &handle.labels, Resource::Topic, Action::View, None)?;

    let Some(registry) = handle.schema_registry() else {
        return Ok(Json(SubjectList {
            registry: None,
            subjects: Vec::new(),
        }));
    };

    // A fault is rendered, never raised: the card carries the state and the
    // list is empty, which is the same shape a healthy empty registry has and
    // is distinguished by the card rather than by the status code.
    let subjects = registry.subjects().await.unwrap_or_default();

    Ok(Json(SubjectList {
        registry: Some(RegistryCard::of(registry)),
        subjects: subjects.as_ref().clone(),
    }))
}

/// `GET /api/clusters/{id}/schemas/{subject}/versions`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/schemas/{subject}/versions",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("subject" = String, Path, description = "Subject name"),
    ),
    responses((status = 200, description = "Every registered version of a subject", body = SubjectDetail)),
    tag = "schemas",
)]
pub async fn versions(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, subject)): Path<(String, String)>,
) -> ApiResult<Json<SubjectDetail>> {
    let handle = state.cluster(&id, &caller)?;
    caller.require(&id, &handle.labels, Resource::Topic, Action::View, None)?;

    let Some(registry) = handle.schema_registry() else {
        // Named rather than a bare 404, because the other 404 on this path is
        // "no such cluster" and a reader has to be able to tell them apart.
        return Err(ApiError::not_found(format!(
            "cluster {id:?} references no schema registry, so it has no subject {subject:?}"
        )));
    };

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

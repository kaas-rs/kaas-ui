//! Configuration viewing. There is no writing: `AlterConfigs` and
//! `IncrementalAlterConfigs` are mutating and absent from kaas-ui entirely.

use axum::Json;
use axum::extract::{Path, Query, State};
use kaas_ui_core::dto::{ConfigEntryDto, ConfigResourceEntry, config_resource_name};
use kaas_ui_core::envelope::Envelope;
use kafka_admin::ConfigResource;
use serde::Deserialize;

use crate::routes::split_list;
use crate::{ApiError, ApiResult, AppState, Caller, call};

/// Which resources to describe.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigQuery {
    /// `broker:1`, `topic:orders`, `group:consumers` — comma-separated.
    ///
    /// Defaults to every broker in the snapshot.
    pub resource: Option<String>,
}

/// `GET /api/clusters/{id}/configs?resource=broker:1`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/configs",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("resource" = Option<String>, Query, description = "Comma-separated `type:name` list"),
    ),
    responses((status = 200, description = "Configurations", body = Envelope<ConfigResourceEntry>)),
    tag = "configs",
)]
pub async fn cluster_configs(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
    Query(query): Query<ConfigQuery>,
) -> ApiResult<Json<Envelope<ConfigResourceEntry>>> {
    let (_, admin) = state.connected(&id, &caller)?;

    let resources: Vec<ConfigResource> = match query.resource.as_deref() {
        Some(raw) => split_list(raw)
            .iter()
            .map(|spec| parse_resource(spec))
            .collect::<ApiResult<Vec<_>>>()?,
        None => admin
            .cluster()
            .snapshot()
            .brokers()
            .iter()
            // `ConfigResource::broker` takes the node id as the *name*.
            // Getting that wrong yields an empty result rather than an error,
            // which is the worst kind of wrong.
            .map(|broker| ConfigResource::broker(broker.node_id))
            .collect(),
    };

    if resources.is_empty() {
        return Err(ApiError::bad_request(
            "no config resources to describe: pass ?resource=broker:1 or wait for a snapshot",
        ));
    }

    describe(&admin, resources).await
}

/// `GET /api/clusters/{id}/topics/{topic}/configs`
#[utoipa::path(
    get,
    path = "/api/clusters/{id}/topics/{topic}/configs",
    params(
        ("id" = String, Path, description = "Cluster id"),
        ("topic" = String, Path, description = "Topic name"),
    ),
    responses((status = 200, description = "Topic configuration", body = Envelope<ConfigResourceEntry>)),
    tag = "configs",
)]
pub async fn topic_configs(
    State(state): State<AppState>,
    caller: Caller,
    Path((id, topic)): Path<(String, String)>,
) -> ApiResult<Json<Envelope<ConfigResourceEntry>>> {
    let (_, admin) = state.connected(&id, &caller)?;
    describe(&admin, vec![ConfigResource::topic(topic)]).await
}

/// Describe with documentation. It roughly triples the response size, and it
/// is what turns a wall of keys into something a reader can act on.
async fn describe(
    admin: &kafka_admin::Admin,
    resources: Vec<ConfigResource>,
) -> ApiResult<Json<Envelope<ConfigResourceEntry>>> {
    let described = call(
        "describe_configs",
        admin.describe_configs_documented(resources),
    )
    .await?;

    let envelope = Envelope::from_per_item(described, config_resource_name, |resource, entries| {
        let mut entries: Vec<ConfigEntryDto> = entries.iter().map(ConfigEntryDto::from).collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        ConfigResourceEntry {
            resource: config_resource_name(resource),
            resource_type: format!("{:?}", resource.resource_type),
            name: resource.name.clone(),
            entries,
        }
    });

    Ok(Json(envelope))
}

fn parse_resource(spec: &str) -> ApiResult<ConfigResource> {
    let (kind, name) = spec
        .split_once(':')
        .ok_or_else(|| ApiError::bad_request(format!("resource {spec:?} is not `type:name`")))?;

    match kind {
        "broker" => {
            let node_id: i32 = name.parse().map_err(|_| {
                ApiError::bad_request(format!("broker resource {spec:?} needs a numeric node id"))
            })?;
            Ok(ConfigResource::broker(node_id))
        }
        "topic" => Ok(ConfigResource::topic(name)),
        "group" => Ok(ConfigResource::group(name)),
        other => Err(ApiError::bad_request(format!(
            "unknown config resource type {other:?}: expected broker, topic or group"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_specs_parse() {
        assert_eq!(parse_resource("topic:orders").unwrap().name, "orders");
        assert_eq!(parse_resource("broker:2").unwrap().name, "2");
        assert!(parse_resource("broker:one").is_err());
        assert!(parse_resource("orders").is_err());
        assert!(parse_resource("nonsense:x").is_err());
    }
}

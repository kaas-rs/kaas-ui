//! The OpenAPI document.
//!
//! Assembled from day one so there is something to generate a typed client
//! from, and so the schemas are checked against the DTOs by the compiler
//! rather than by a reviewer.
//!
//! Nothing upstream appears here. Every schema below is a kaas-ui type, which
//! is what stops a kaas-lib bump from rewriting the client's types.

use utoipa::OpenApi;

/// The spec.
#[derive(Debug, OpenApi)]
#[openapi(
    info(
        title = "kaas-ui",
        description = "A read-only, multi-cluster Kafka UI. Every data route is a GET; \
                       there is no mutating endpoint in this document because there is \
                       none in the router.",
        license(name = "Apache-2.0"),
    ),
    paths(
        crate::routes::health::health,
        crate::routes::me::me,
        crate::routes::clusters::fleet,
        crate::routes::clusters::list,
        crate::routes::clusters::detail,
        crate::routes::clusters::brokers,
        crate::routes::clusters::log_dirs,
        crate::routes::capabilities::capabilities,
        crate::routes::configs::cluster_configs,
        crate::routes::configs::topic_configs,
        crate::routes::topics::list,
        crate::routes::topics::detail,
        crate::routes::topics::offsets,
        crate::routes::groups::list,
        crate::routes::groups::detail,
        crate::routes::groups::offsets,
        crate::routes::messages::tail,
        crate::routes::messages::page,
        crate::routes::messages::one,
        crate::routes::messages::stream::stream,
        crate::routes::analysis::analysis,
        crate::routes::schemas::list,
        crate::routes::schemas::versions,
        crate::routes::admin::acls,
        crate::routes::admin::quotas,
        crate::routes::admin::scram_users,
        crate::routes::admin::reassignments,
        crate::routes::admin::transactions,
        crate::routes::admin::transaction,
        crate::routes::admin::producers,
        crate::routes::spec::spec,
    ),
    components(schemas(
        crate::error::ApiErrorBody,
        crate::routes::health::Health,
        crate::routes::messages::MessagePage,
        crate::routes::messages::SeekMode,
        kaas_ui_core::dto::EnvironmentRegistry,
        crate::routes::schemas::SubjectDetail,
        crate::routes::schemas::SubjectList,
        crate::routes::schemas::SubjectRow,
        crate::routes::topics::PartitionOffsets,
        kaas_ui_auth::Action,
        kaas_ui_auth::Resource,
        kaas_ui_core::capabilities::ApiKeyEntry,
        kaas_ui_core::capabilities::Capabilities,
        kaas_ui_core::capabilities::CapabilitySource,
        kaas_ui_core::capabilities::Feature,
        kaas_ui_core::capabilities::FeatureEntry,
        kaas_ui_core::capabilities::FeatureState,
        kaas_ui_core::analysis::AnalysisProgress,
        kaas_ui_core::analysis::AnalysisStats,
        kaas_ui_core::analysis::HourCount,
        kaas_ui_core::analysis::SizeStats,
        kaas_ui_core::analysis::TopicAnalysis,
        kaas_ui_core::dto::Broker,
        kaas_ui_core::dto::ClusterCard,
        kaas_ui_core::dto::ClusterDescriptionDto,
        kaas_ui_core::dto::ClusterDetail,
        kaas_ui_core::config::ResourceKind,
        kaas_ui_core::dto::ConfigEntryDto,
        kaas_ui_core::dto::ConfigResourceEntry,
        kaas_ui_core::dto::EnvironmentSection,
        kaas_ui_core::dto::ResourceCard,
        kaas_ui_core::dto::GroupDetail,
        kaas_ui_core::dto::GroupMember,
        kaas_ui_core::dto::GroupOffset,
        kaas_ui_core::dto::GroupSummary,
        kaas_ui_core::dto::Identity,
        kaas_ui_core::dto::Dropped,
        kaas_ui_core::dto::Header,
        kaas_ui_core::dto::Lag,
        kaas_ui_core::dto::LoginConnector,
        kaas_ui_core::dto::LogDirDto,
        kaas_ui_core::dto::LogDirReplicaDto,
        kaas_ui_core::dto::MalformedDetail,
        kaas_ui_core::dto::MalformedRow,
        kaas_ui_core::dto::Message,
        kaas_ui_core::dto::MessageDetail,
        kaas_ui_core::dto::Partition,
        kaas_ui_core::dto::Payload,
        kaas_ui_core::dto::RegistryCard,
        kaas_ui_core::dto::ResolvedPartition,
        kaas_ui_core::dto::ResolvedSeek,
        kaas_ui_core::dto::StreamPhase,
        kaas_ui_core::dto::StreamProgress,
        kaas_ui_core::dto::StreamRecord,
        kaas_ui_core::dto::StreamRow,
        kaas_ui_core::dto::TopicDetail,
        kaas_ui_core::admin::Acl,
        kaas_ui_core::admin::ClientQuota,
        kaas_ui_core::admin::Producer,
        kaas_ui_core::admin::QuotaComponent,
        kaas_ui_core::admin::QuotaValue,
        kaas_ui_core::admin::Reassignment,
        kaas_ui_core::admin::ScramCredential,
        kaas_ui_core::admin::ScramUser,
        kaas_ui_core::admin::Transaction,
        kaas_ui_core::admin::TransactionPartitions,
        kaas_ui_core::dto::TopicPartitions,
        kaas_ui_core::dto::TopicSchemas,
        kaas_ui_core::dto::TopicSummary,
        kaas_ui_core::envelope::ResourceError,
        kaas_ui_core::error::ErrorKind,
        kaas_ui_core::error::UnsupportedApiDetail,
        kaas_ui_core::health::ClusterStatus,
        kaas_ui_serde::Codec,
        kaas_ui_serde::NamingStrategy,
        kaas_ui_serde::NoteKind,
        kaas_ui_serde::PayloadNote,
        kaas_ui_serde::RawBytes,
        kaas_ui_serde::RegistryStatus,
        kaas_ui_serde::SchemaFormat,
        kaas_ui_serde::SchemaRef,
        kaas_ui_serde::SchemaReference,
        kaas_ui_serde::SubjectNaming,
        kaas_ui_serde::SubjectSchema,
    )),
    tags(
        (name = "health", description = "Liveness. Never consults a cluster."),
        (name = "clusters", description = "The fleet, and one cluster's detail."),
        (name = "capabilities", description = "What a cluster can be asked."),
        (name = "topics", description = "Topics, partitions and offsets."),
        (name = "configs", description = "Configuration viewing. There is no writing."),
        (name = "groups", description = "Consumer groups, committed offsets and lag."),
        (name = "messages", description = "The message browser."),
        (name = "schemas", description = "The schema registry an environment shares, reached through a cluster."),
        (name = "admin", description = "The read-only admin surface: ACLs, quotas, SCRAM users, reassignments, transactions. Every one is a describe; the altering neighbours are not in the workspace."),
        (name = "meta", description = "The API describing itself."),
    ),
)]
pub struct ApiDoc;

/// The spec as pretty JSON.
pub fn spec_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_builds_and_has_no_mutating_path() {
        let spec = spec_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&spec).unwrap();
        let paths = parsed["paths"].as_object().unwrap();
        assert!(!paths.is_empty());

        // The claim PLAN.md makes, checked against the generated document
        // rather than against intent.
        for (path, item) in paths {
            for method in item.as_object().unwrap().keys() {
                assert_eq!(
                    method, "get",
                    "{path} exposes {method}: kaas-ui has no mutating routes"
                );
            }
        }
    }
}

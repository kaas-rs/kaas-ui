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
        description = "A multi-cluster Kafka UI. Every read is a GET; every write passes \
                       two gates — the target's configured ceiling and a role — and is \
                       audited before it is sent.",
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
        crate::routes::clusters::tls,
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
        crate::routes::connect::detail,
        crate::routes::connect::connectors,
        crate::routes::connect::connector,
        crate::routes::connect::plugins,
        crate::routes::connect::topic_connectors,
        crate::routes::connect_write::write,
        crate::routes::connect_write::write_task,
        crate::routes::connect_write::create,
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
        crate::routes::connect::ConnectClusterDetail,
        crate::routes::connect::ConnectorCounts,
        crate::routes::connect::ConnectorList,
        crate::routes::connect::ConnectorPage,
        crate::routes::connect::KafkaClusterCheck,
        crate::routes::connect::KafkaClusterCheckState,
        crate::routes::connect::PluginList,
        crate::routes::connect::TopicConnector,
        crate::routes::connect::TopicConnectors,
        crate::routes::connect_write::CreateRequest,
        crate::routes::connect_write::Refusal,
        crate::routes::connect_write::WriteOption,
        crate::routes::connect_write::WriteRequest,
        crate::routes::connect_write::WriteResult,
        kaas_ui_connect::Validation,
        kaas_ui_connect::ValidationError,
        kaas_ui_auth::Operation,
        kaas_ui_auth::Tier,
        kaas_ui_core::dto::ConnectClusterCard,
        kaas_ui_connect::ConnectorConfigEntry,
        kaas_ui_connect::ConnectStatus,
        kaas_ui_connect::ConnectorDetail,
        kaas_ui_connect::ConnectorStatus,
        kaas_ui_connect::ConnectorSummary,
        kaas_ui_connect::ConnectorType,
        kaas_ui_connect::Plugin,
        kaas_ui_connect::RedactedConfig,
        kaas_ui_connect::Redaction,
        kaas_ui_connect::ServerInfo,
        kaas_ui_connect::StoredOffset,
        kaas_ui_connect::TaskConfig,
        kaas_ui_connect::TaskStatus,
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
        kaas_ui_core::tls::BrokerTls,
        kaas_ui_core::tls::TlsSessionDto,
        kaas_ui_core::tls::CertificateDto,
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
        kaas_ui_core::sizing::Assumptions,
        kaas_ui_core::sizing::Change,
        kaas_ui_core::sizing::Diagnostic,
        kaas_ui_core::sizing::DiagnosticCode,
        kaas_ui_core::sizing::Measured,
        kaas_ui_core::sizing::Profile,
        kaas_ui_core::sizing::ProfileAdvice,
        kaas_ui_core::sizing::Recommendation,
        kaas_ui_core::sizing::SettingValue,
        kaas_ui_core::sizing::Severity,
        kaas_ui_core::sizing::SizeEvidence,
        kaas_ui_core::sizing::SizingAdvice,
        kaas_ui_core::sizing::Topology,
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
        (name = "configs", description = "Broker and topic configuration."),
        (name = "groups", description = "Consumer groups, committed offsets and lag."),
        (name = "messages", description = "The message browser."),
        (name = "schemas", description = "The schema registry an environment shares, reached through a cluster."),
        (name = "connect", description = "Kafka Connect clusters, connectors, tasks and plugins, over the Connect REST API. Connector configs are redacted before they leave the client, and every write is gated."),
        (name = "admin", description = "ACLs, quotas, SCRAM users, reassignments, transactions."),
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

    /// Every path that is not a `GET`. Phase 10's writes, one per line, so a
    /// new one is a change to this list that a reviewer sees.
    const WRITES: [(&str, &str); 3] = [
        (
            "post",
            "/api/environments/{env}/connect-clusters/{connect}/connectors",
        ),
        (
            "post",
            "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}/tasks/{task}/restart",
        ),
        (
            "post",
            "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}/{operation}",
        ),
    ];

    #[test]
    fn the_document_builds_and_its_only_writes_are_the_gated_ones() {
        let spec = spec_json().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&spec).unwrap();
        let paths = parsed["paths"].as_object().unwrap();
        assert!(!paths.is_empty());

        // Checked against the generated document rather than against intent:
        // a mutating route that is not on the list above fails here.
        let mut writes = Vec::new();
        for (path, item) in paths {
            for method in item.as_object().unwrap().keys() {
                if method != "get" {
                    writes.push((method.as_str(), path.as_str()));
                }
            }
        }
        writes.sort_unstable();
        let mut expected = WRITES.to_vec();
        expected.sort_unstable();
        assert_eq!(writes, expected, "the non-GET routes changed");
    }
}

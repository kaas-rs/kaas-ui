//! The read-only admin surface: ACLs, quotas, SCRAM users, reassignments and
//! transactions.
//!
//! Six upstream types cross the boundary here — more than any other module —
//! and every one of them gets a DTO. `AclBinding`'s three enums are the reason
//! it matters: they are `kafka-admin`'s spelling of a wire code, and putting
//! one in a `utoipa` schema would make the generated TypeScript a function of
//! which kaas-lib this was built against.
//!
//! **Nothing here can write.** The mutating neighbours of these calls —
//! `CreateAcls`, `AlterClientQuotas`, `AlterUserScramCredentials`,
//! `AlterPartitionReassignments` — exist on `Admin` and are not reachable from
//! any type in this file, because a DTO is a shape for an answer.

use kafka_admin::{
    AclBinding, AclOperation, AclPermission, AclResourceType, OngoingReassignment, PatternType,
    ProducerState, QuotaEntity, ScramCredentialInfo, ScramMechanism, TransactionDescription,
    TransactionListing,
};
use serde::Serialize;
use utoipa::ToSchema;

/// One ACL binding, as the authorizer stores it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Acl {
    /// What kind of thing the binding is about — `topic`, `group`, `cluster`,
    /// `transactionalId`, `delegationToken` or `user`.
    pub resource_type: String,
    /// The resource name, or `*` for every resource of that type.
    pub resource_name: String,
    /// `literal` or `prefixed`, which is the difference between a binding that
    /// covers one name and one that covers a namespace.
    pub pattern_type: String,
    /// The principal, as the authorizer spells it — `User:alice`.
    pub principal: String,
    /// The host it applies from, or `*`.
    pub host: String,
    /// The operation.
    pub operation: String,
    /// `allow` or `deny`. Denies win in Kafka's evaluation order, which is why
    /// this is a column and not a colour.
    pub permission: String,
}

impl From<&AclBinding> for Acl {
    fn from(binding: &AclBinding) -> Self {
        Self {
            resource_type: resource_type_name(binding.resource_type).to_owned(),
            resource_name: binding.resource_name.clone(),
            pattern_type: pattern_type_name(binding.pattern_type).to_owned(),
            principal: binding.principal.clone(),
            host: binding.host.clone(),
            operation: operation_name(binding.operation),
            permission: permission_name(binding.permission).to_owned(),
        }
    }
}

/// One quota entity and the limits configured for it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientQuota {
    /// The entity, one component per type.
    ///
    /// A quota is addressed by a *set* of components rather than by a name:
    /// `user=alice` and `user=alice, client-id=app` are two different quotas
    /// and the second is the more specific match.
    pub entity: Vec<QuotaComponent>,
    /// The configured values, sorted by key.
    pub values: Vec<QuotaValue>,
}

impl ClientQuota {
    /// Build from what `describe_client_quotas` returns for one entity.
    #[must_use]
    pub fn of(entity: &QuotaEntity, values: &[(String, f64)]) -> Self {
        let mut values: Vec<QuotaValue> = values
            .iter()
            .map(|(key, value)| QuotaValue {
                key: key.clone(),
                value: *value,
            })
            .collect();
        values.sort_by(|a, b| a.key.cmp(&b.key));

        Self {
            entity: entity
                .components
                .iter()
                .map(|(entity_type, name)| QuotaComponent {
                    entity_type: entity_type.clone(),
                    name: name.clone(),
                })
                .collect(),
            values,
        }
    }
}

/// One component of a quota entity.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaComponent {
    /// `user`, `client-id` or `ip`.
    pub entity_type: String,
    /// The name, or `None` for the *default* entity of that type.
    ///
    /// Null is not "unset": a null user with a set client-id is the quota that
    /// applies to that client for every user who has no quota of their own.
    /// Rendering it blank loses the whole meaning, so it travels as null and
    /// the UI writes `<default>`.
    pub name: Option<String>,
}

/// One configured limit.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaValue {
    /// The key, as the broker names it — `producer_byte_rate`,
    /// `consumer_byte_rate`, `request_percentage`, `controller_mutation_rate`.
    pub key: String,
    /// The value. A rate in bytes per second, or a percentage, depending on
    /// the key — which is why no unit is baked in here.
    pub value: f64,
}

/// One user with SCRAM credentials, and the mechanisms they hold.
///
/// **Never a credential.** The broker stores a salted hash and cannot return
/// one; this says who can authenticate, not how.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScramUser {
    /// The user name.
    pub user: String,
    /// One entry per mechanism the user has a credential for.
    pub credentials: Vec<ScramCredential>,
}

/// One mechanism a user holds a credential for.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScramCredential {
    /// `SCRAM-SHA-256` or `SCRAM-SHA-512`.
    pub mechanism: String,
    /// The iteration count the credential was stored with. Kafka's floor is
    /// 4096; a lower number is a credential written by something that ignored
    /// it.
    pub iterations: i32,
}

impl From<&ScramCredentialInfo> for ScramCredential {
    fn from(info: &ScramCredentialInfo) -> Self {
        Self {
            mechanism: match info.mechanism {
                ScramMechanism::Sha256 => "SCRAM-SHA-256",
                ScramMechanism::Sha512 => "SCRAM-SHA-512",
            }
            .to_owned(),
            iterations: info.iterations,
        }
    }
}

/// One partition being moved.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reassignment {
    /// The topic.
    pub topic: String,
    /// The partition index.
    pub partition: i32,
    /// The current replica set — it holds both the replicas being added and
    /// those being removed until the move completes, which is why the two
    /// below are separate lists rather than a diff the reader has to take.
    pub replicas: Vec<i32>,
    /// Replicas being added.
    pub adding: Vec<i32>,
    /// Replicas being removed.
    pub removing: Vec<i32>,
}

impl From<&OngoingReassignment> for Reassignment {
    fn from(moving: &OngoingReassignment) -> Self {
        Self {
            topic: moving.topic.clone(),
            partition: moving.partition,
            replicas: moving.replicas.clone(),
            adding: moving.adding.clone(),
            removing: moving.removing.clone(),
        }
    }
}

/// One row of the transaction list.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    /// The transactional id.
    pub transactional_id: String,
    /// The producer id currently holding it.
    pub producer_id: i64,
    /// The state, in the broker's own vocabulary.
    ///
    /// Passed through rather than mapped onto an enum of ours: the set grows
    /// with Kafka releases, and a kaas-ui enum would be missing a state on the
    /// next one and render it as "unknown" on a screen whose whole job is to
    /// say what is happening.
    pub state: String,
    /// When the current transaction started, in epoch milliseconds, where one
    /// is in flight.
    ///
    /// **The start, not the duration.** How long it has been open is the
    /// number that matters, and it is wrong the moment it is serialised —
    /// `open_for_ms` takes a `now` and the browser is the only place with a
    /// useful one. The same decision `snapshotAgeMs` made, for the same reason.
    pub start_time_ms: Option<i64>,
    /// The configured transaction timeout, where the transaction was
    /// described. A transaction open far past this is the one holding up the
    /// last stable offset.
    pub timeout_ms: Option<i32>,
    /// The producer epoch, where the transaction was described.
    pub producer_epoch: Option<i16>,
    /// The partitions enrolled in the current transaction, where it was
    /// described.
    pub partitions: Vec<TransactionPartitions>,
}

impl From<&TransactionListing> for Transaction {
    /// The listing alone — everything the describe adds is `None`.
    fn from(listing: &TransactionListing) -> Self {
        Self {
            transactional_id: listing.transactional_id.clone(),
            producer_id: listing.producer_id,
            state: listing.state.clone(),
            start_time_ms: None,
            timeout_ms: None,
            producer_epoch: None,
            partitions: Vec::new(),
        }
    }
}

impl From<&TransactionDescription> for Transaction {
    fn from(description: &TransactionDescription) -> Self {
        Self {
            transactional_id: description.transactional_id.clone(),
            producer_id: description.producer_id,
            state: description.state.clone(),
            start_time_ms: description.start_time_ms,
            timeout_ms: Some(description.timeout_ms),
            producer_epoch: Some(description.producer_epoch),
            partitions: description
                .partitions
                .iter()
                .map(|(topic, partitions)| TransactionPartitions {
                    topic: topic.clone(),
                    partitions: partitions.clone(),
                })
                .collect(),
        }
    }
}

/// A topic and the partitions of it enrolled in a transaction.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPartitions {
    /// The topic.
    pub topic: String,
    /// The partition indexes.
    pub partitions: Vec<i32>,
}

/// One producer writing to one partition.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Producer {
    /// The topic.
    pub topic: String,
    /// The partition.
    pub partition: i32,
    /// The producer id.
    pub producer_id: i64,
    /// The producer epoch. Not a leader epoch: this one bumps when a producer
    /// is fenced, which is how a zombie is told from the producer that
    /// replaced it.
    pub producer_epoch: i32,
    /// The last sequence number the broker accepted from it.
    pub last_sequence: i32,
    /// When it last wrote, in epoch milliseconds.
    pub last_timestamp: i64,
    /// The coordinator epoch, where the producer is transactional.
    pub coordinator_epoch: i32,
    /// Where this producer's open transaction starts, or `None` when it has
    /// none in flight — which is the field that identifies the producer
    /// holding the last stable offset back.
    pub current_txn_start_offset: Option<i64>,
}

impl Producer {
    /// Build from one `DescribeProducers` entry on a named partition.
    #[must_use]
    pub fn of(topic: &str, partition: i32, state: &ProducerState) -> Self {
        Self {
            topic: topic.to_owned(),
            partition,
            producer_id: state.producer_id,
            producer_epoch: state.producer_epoch,
            last_sequence: state.last_sequence,
            last_timestamp: state.last_timestamp,
            coordinator_epoch: state.coordinator_epoch,
            current_txn_start_offset: state.current_txn_start_offset,
        }
    }
}

/// The wire-ish name of an ACL resource type.
///
/// Written out rather than derived from `Debug`: `Debug` is a rendering for
/// programmers that upstream may change without it being a breaking change,
/// and these strings are in the HTTP contract.
const fn resource_type_name(kind: AclResourceType) -> &'static str {
    match kind {
        AclResourceType::Any => "any",
        AclResourceType::Topic => "topic",
        AclResourceType::Group => "group",
        AclResourceType::Cluster => "cluster",
        AclResourceType::TransactionalId => "transactionalId",
        AclResourceType::DelegationToken => "delegationToken",
        AclResourceType::User => "user",
    }
}

/// `literal` or `prefixed` — the two an authorizer stores. `any` and `match`
/// are filter-only and cannot come back on a binding.
const fn pattern_type_name(pattern: PatternType) -> &'static str {
    match pattern {
        PatternType::Any => "any",
        PatternType::Match => "match",
        PatternType::Literal => "literal",
        PatternType::Prefixed => "prefixed",
    }
}

/// The operation, lower-cased the way Kafka's own tooling prints it.
///
/// An operation this build has no name for renders as `unknown(99)` and is
/// **expected output**, not a gap: naming it here would be a Kafka version
/// table in kaas-ui, which is rule 2 with extra steps. The number is the
/// searchable thing, exactly as it is for an unknown api key.
fn operation_name(operation: AclOperation) -> String {
    match operation {
        AclOperation::Any => "any",
        AclOperation::All => "all",
        AclOperation::Read => "read",
        AclOperation::Write => "write",
        AclOperation::Create => "create",
        AclOperation::Delete => "delete",
        AclOperation::Alter => "alter",
        AclOperation::Describe => "describe",
        AclOperation::ClusterAction => "clusterAction",
        AclOperation::DescribeConfigs => "describeConfigs",
        AclOperation::AlterConfigs => "alterConfigs",
        AclOperation::IdempotentWrite => "idempotentWrite",
        AclOperation::CreateTokens => "createTokens",
        AclOperation::DescribeTokens => "describeTokens",
        AclOperation::Unknown(code) => return format!("unknown({code})"),
    }
    .to_owned()
}

/// `allow` or `deny`.
const fn permission_name(permission: AclPermission) -> &'static str {
    match permission {
        AclPermission::Any => "any",
        AclPermission::Deny => "deny",
        AclPermission::Allow => "allow",
    }
}

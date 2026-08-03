//! The types that reach the wire.
//!
//! kaas-lib's rule — no upstream type in a public signature — one level up.
//! No `kafka_meta::TopicInfo` appears in a `utoipa` schema; the `From` impls
//! below are the boundary, and they are the only place a library bump can
//! break, rather than every screen at once.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use kafka_admin::{
    ClassicGroupMember, ClusterBroker, ClusterDescription, CommittedOffset, ConfigEntry,
    ConfigResource, ConfigSource, ConsumerGroupMember, GroupDescription, GroupListing, GroupState,
    LogDir, LogDirReplica, ShareGroupMember, TopicSize,
};
use kafka_meta::{BrokerInfo, MetadataSnapshot, PartitionInfo, TopicId, TopicInfo};
use kafka_read::{Record, TimestampType};
use serde::Serialize;
use utoipa::ToSchema;

use kaas_ui_auth::{Access, Grant, Grants, Principal};

use crate::health::{ClusterHealth, ClusterStatus};
use crate::registry::ClusterHandle;

/// The caller, for the header and for deciding what to offer them.
///
/// Not "what may I do" — that is per cluster and rides on [`ClusterCard`] as
/// `grants`. This is who the request is from, which the UI needs to render a
/// "signed in as" line and to know whether signing in is even a thing this
/// deployment does.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    /// Whether an identity provider vouched for this caller.
    pub authenticated: bool,
    /// The stable id. `"anonymous"` when nobody signed in.
    pub subject: String,
    /// What to render.
    pub display_name: String,
    /// The roles that covered this caller, in policy order.
    pub roles: Vec<String>,
    /// Whether this deployment applies roles at all.
    ///
    /// `false` is the open deployment: no identity provider, one anonymous
    /// caller, everything visible. The frontend uses it to decide whether to
    /// offer signing in, and it is worth being explicit about rather than
    /// inferring from an empty role list — which is also what a misconfigured
    /// policy looks like.
    pub enforcing: bool,
    /// Whether an identity provider is configured at all.
    ///
    /// Distinct from `enforcing`, and the frontend needs both: this decides
    /// whether to offer a sign-in button, while `enforcing` decides whether
    /// being signed out means seeing nothing. A deployment can have roles and
    /// no provider — which is a misconfiguration the startup log warns about —
    /// or a provider and no roles, which is an open deployment that happens to
    /// know your name.
    pub login_available: bool,
}

impl Identity {
    /// Project a resolved caller.
    pub fn of(who: &Principal, access: &Access, enforcing: bool, login_available: bool) -> Self {
        Self {
            authenticated: who.is_authenticated(),
            subject: who.subject().to_owned(),
            display_name: who.display_name().to_owned(),
            roles: access.role_names().map(str::to_owned).collect(),
            enforcing,
            login_available,
        }
    }
}

/// One cluster on the fleet dashboard.
///
/// Built from `Cluster::snapshot()` and nothing else. The snapshot carries
/// brokers, controller id, cluster id and every partition's replicas, ISR and
/// offline set — everything a card renders — and it works on clusters that do
/// not implement `DescribeCluster`. Enrichment that only some clusters can
/// answer belongs on the detail page, not on the landing page.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterCard {
    /// The configured id.
    pub id: String,
    /// The configured name.
    pub name: String,
    /// Grouping labels.
    pub labels: BTreeMap<String, String>,
    /// Reachability.
    pub status: ClusterStatus,
    /// The failure, when unreachable.
    pub error: Option<String>,
    /// How many attempts have failed.
    pub attempts: u32,
    /// The broker-reported cluster id, when the snapshot carries one.
    pub cluster_id: Option<String>,
    /// The controller, as the snapshot reports it.
    pub controller_id: Option<i32>,
    /// Brokers in the snapshot.
    pub broker_count: usize,
    /// Topics, including internal ones.
    pub topic_count: usize,
    /// Internal topics, of that count.
    pub internal_topic_count: usize,
    /// Partitions across all topics.
    pub partition_count: usize,
    /// Partitions with no leader or a replica on a dead broker.
    pub offline_partition_count: usize,
    /// Partitions whose ISR is smaller than their replica set.
    pub under_replicated_partition_count: usize,
    /// Age of the snapshot this was built from.
    pub snapshot_age_ms: Option<u64>,
    /// The staleness ceiling this cluster was configured with, so the UI can
    /// colour the age rather than guessing a threshold.
    pub max_staleness_ms: u64,
    /// What the caller may do here.
    ///
    /// Projected per cluster rather than per session because that is what a
    /// role grants: `metadata` on prod and `messages` on dev is one caller
    /// with two answers. The frontend hides what it must not offer — a
    /// messages tab that 403s on click is worse than no tab — which is the
    /// same mechanism the capability projection uses for what a *broker*
    /// cannot do.
    ///
    /// `value_type` because the alias is a `BTreeSet`, which utoipa cannot
    /// name — left alone it emits a `$ref` to a `BTreeSet` schema that does
    /// not exist, and the generated client is then broken in a way nothing in
    /// Rust notices.
    #[schema(value_type = Vec<Grant>)]
    pub grants: Grants,
}

impl ClusterCard {
    /// Build a card from a handle, reading the snapshot if there is one.
    ///
    /// Takes the caller's [`Access`] because the card carries what they may do
    /// here, and computing that anywhere but next to the labels it is derived
    /// from is how a card ends up advertising a grant nobody holds.
    pub fn of(handle: &ClusterHandle, who: &Access) -> Self {
        let health = handle.health();
        let (error, attempts) = match health.as_ref() {
            ClusterHealth::Unreachable {
                error, attempts, ..
            } => (Some(error.clone()), *attempts),
            other => (None, other.attempts()),
        };

        let mut card = Self {
            id: handle.id.clone(),
            name: handle.name.clone(),
            labels: handle.labels.clone(),
            status: health.status(),
            error,
            attempts,
            cluster_id: None,
            controller_id: None,
            broker_count: 0,
            topic_count: 0,
            internal_topic_count: 0,
            partition_count: 0,
            offline_partition_count: 0,
            under_replicated_partition_count: 0,
            snapshot_age_ms: None,
            max_staleness_ms: millis(handle.max_staleness()),
            grants: who.grants(&handle.labels),
        };

        if let Some(admin) = handle.admin() {
            let snapshot = admin.cluster().snapshot();
            card.absorb(&snapshot);
        }
        card
    }

    fn absorb(&mut self, snapshot: &MetadataSnapshot) {
        self.cluster_id = snapshot.cluster_id().map(str::to_owned);
        self.controller_id = snapshot.controller_id();
        self.broker_count = snapshot.brokers().len();
        self.topic_count = snapshot.topics().len();
        self.internal_topic_count = snapshot.topics().iter().filter(|t| t.internal).count();
        self.snapshot_age_ms = Some(millis(snapshot.age()));

        for topic in snapshot.topics() {
            for partition in &topic.partitions {
                self.partition_count += 1;
                if partition.leader.is_none() || !partition.offline_replicas.is_empty() {
                    self.offline_partition_count += 1;
                }
                if partition.under_replicated() {
                    self.under_replicated_partition_count += 1;
                }
            }
        }
    }
}

/// A broker, as the snapshot reports it — optionally enriched.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Broker {
    /// Node id.
    pub node_id: i32,
    /// Advertised host.
    pub host: String,
    /// Advertised port.
    pub port: i32,
    /// Rack, when the broker declares one.
    pub rack: Option<String>,
    /// Whether this broker is the controller.
    pub is_controller: bool,
    /// Whether the controller has fenced it.
    ///
    /// `None` on a cluster that does not implement `DescribeCluster`: absent,
    /// not false. Rendering an unknown as "healthy" is the one thing a fleet
    /// dashboard must never do.
    pub is_fenced: Option<bool>,
    /// Partitions this broker leads.
    pub leader_partition_count: usize,
    /// Partition replicas hosted here.
    pub replica_partition_count: usize,
}

impl Broker {
    /// Build the broker list from a snapshot.
    pub fn list(snapshot: &MetadataSnapshot) -> Vec<Self> {
        let mut brokers: Vec<Self> = snapshot
            .brokers()
            .iter()
            .map(|info| Self::from_snapshot(info, snapshot))
            .collect();
        brokers.sort_by_key(|broker| broker.node_id);
        brokers
    }

    fn from_snapshot(info: &BrokerInfo, snapshot: &MetadataSnapshot) -> Self {
        let mut leader_partition_count = 0;
        let mut replica_partition_count = 0;
        for topic in snapshot.topics() {
            for partition in &topic.partitions {
                if partition.leader == Some(info.node_id) {
                    leader_partition_count += 1;
                }
                if partition.replicas.contains(&info.node_id) {
                    replica_partition_count += 1;
                }
            }
        }

        Self {
            node_id: info.node_id,
            host: info.host.clone(),
            port: info.port,
            rack: info.rack.clone(),
            is_controller: snapshot.controller_id() == Some(info.node_id),
            is_fenced: None,
            leader_partition_count,
            replica_partition_count,
        }
    }

    /// Fold `DescribeCluster` in where the cluster answered it.
    ///
    /// The only thing it adds over the snapshot is `is_fenced` and an
    /// authoritative controller id, which is why the fleet view does not wait
    /// for it.
    pub fn enrich(brokers: &mut [Broker], description: &ClusterDescription) {
        for broker in brokers.iter_mut() {
            if let Some(described) = description
                .brokers
                .iter()
                .find(|candidate: &&ClusterBroker| candidate.node_id == broker.node_id)
            {
                broker.is_fenced = Some(described.is_fenced);
            }
            broker.is_controller = description.controller_id == Some(broker.node_id);
        }
    }
}

/// The cluster detail page.
///
/// The card, plus the two things a detail page adds: the broker list, and
/// `DescribeCluster` **where the cluster implements it**. Where it does not,
/// `description` is `None` and the failure travels in the envelope's `errors`
/// as a named resource — the page renders the snapshot data with a note, not
/// an error.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterDetail {
    /// Everything the fleet card shows.
    pub cluster: ClusterCard,
    /// Brokers, from the snapshot, enriched where possible.
    pub brokers: Vec<Broker>,
    /// `DescribeCluster`, where it answered.
    pub description: Option<ClusterDescriptionDto>,
}

/// One resource's configuration.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResourceEntry {
    /// The resource, as it was asked for — `broker:1`, `topic:orders`.
    pub resource: String,
    /// Its type.
    pub resource_type: String,
    /// Its name.
    pub name: String,
    /// The entries, sorted by key.
    pub entries: Vec<ConfigEntryDto>,
}

/// `DescribeCluster`, where the cluster implements it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterDescriptionDto {
    /// The broker-reported cluster id.
    pub cluster_id: String,
    /// The authoritative controller.
    pub controller_id: Option<i32>,
}

impl From<&ClusterDescription> for ClusterDescriptionDto {
    fn from(value: &ClusterDescription) -> Self {
        Self {
            cluster_id: value.cluster_id.clone(),
            controller_id: value.controller_id,
        }
    }
}

/// One row of the topic list.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicSummary {
    /// Topic name.
    pub name: String,
    /// The topic uuid, or `None` on a cluster that does not report one.
    ///
    /// Absent rather than an empty uuid: a column of zeroes reads as missing
    /// data, and the honest render is no column at all.
    pub topic_id: Option<String>,
    /// Whether Kafka considers this internal.
    pub internal: bool,
    /// Partition count.
    pub partition_count: usize,
    /// The smallest replica count across partitions, which is what anyone
    /// means by "replication factor".
    pub replication_factor: usize,
    /// Partitions with no leader or an offline replica.
    pub offline_partition_count: usize,
    /// Partitions whose ISR is short.
    pub under_replicated_partition_count: usize,
    /// Bytes on disk for one copy, when log dirs were asked for.
    pub logical_bytes: Option<i64>,
    /// Bytes on disk across replicas.
    pub replicated_bytes: Option<i64>,
}

impl TopicSummary {
    /// Build from the snapshot's view of a topic.
    pub fn of(topic: &TopicInfo) -> Self {
        Self {
            name: topic.name.clone(),
            topic_id: render_topic_id(&topic.topic_id),
            internal: topic.internal,
            partition_count: topic.partitions.len(),
            replication_factor: topic
                .partitions
                .iter()
                .map(|p| p.replicas.len())
                .min()
                .unwrap_or(0),
            offline_partition_count: topic
                .partitions
                .iter()
                .filter(|p| p.leader.is_none() || !p.offline_replicas.is_empty())
                .count(),
            under_replicated_partition_count: topic
                .partitions
                .iter()
                .filter(|p| p.under_replicated())
                .count(),
            logical_bytes: None,
            replicated_bytes: None,
        }
    }

    /// Attach sizes from `topic_sizes()`.
    pub fn with_size(mut self, size: &TopicSize) -> Self {
        self.logical_bytes = Some(size.logical_bytes);
        self.replicated_bytes = Some(size.replicated_bytes);
        self
    }
}

/// A topic and its partitions.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicDetail {
    /// Topic name.
    pub name: String,
    /// The topic uuid, where reported.
    pub topic_id: Option<String>,
    /// Whether Kafka considers this internal.
    pub internal: bool,
    /// Every partition.
    pub partitions: Vec<Partition>,
    /// The brokers holding replicas, for the placement grid's columns.
    pub broker_ids: Vec<i32>,
}

impl TopicDetail {
    /// Build from a described topic.
    pub fn of(topic: &TopicInfo) -> Self {
        let mut broker_ids: Vec<i32> = topic
            .partitions
            .iter()
            .flat_map(|p| p.replicas.iter().copied())
            .collect();
        broker_ids.sort_unstable();
        broker_ids.dedup();

        Self {
            name: topic.name.clone(),
            topic_id: render_topic_id(&topic.topic_id),
            internal: topic.internal,
            partitions: topic.partitions.iter().map(Partition::of).collect(),
            broker_ids,
        }
    }
}

/// One partition.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Partition {
    /// Partition index.
    pub partition: i32,
    /// Leader, or `None` when the partition has none.
    pub leader: Option<i32>,
    /// Leader epoch.
    pub leader_epoch: i32,
    /// The replica set, in assignment order — the placement grid's rows.
    pub replicas: Vec<i32>,
    /// In-sync replicas.
    pub isr: Vec<i32>,
    /// Replicas on brokers that are down.
    pub offline_replicas: Vec<i32>,
    /// ISR smaller than the replica set.
    pub under_replicated: bool,
    /// The broker's error for this partition, if any.
    pub error: Option<String>,
    /// Earliest available offset, on the detail page only.
    pub earliest_offset: Option<i64>,
    /// Next offset to be written.
    pub latest_offset: Option<i64>,
}

impl Partition {
    fn of(partition: &PartitionInfo) -> Self {
        Self {
            partition: partition.partition,
            leader: partition.leader,
            leader_epoch: partition.leader_epoch,
            replicas: partition.replicas.clone(),
            isr: partition.isr.clone(),
            offline_replicas: partition.offline_replicas.clone(),
            under_replicated: partition.under_replicated(),
            error: partition
                .error
                .and_then(|code| code.name())
                .map(str::to_owned),
            earliest_offset: None,
            latest_offset: None,
        }
    }

    /// Attach an offset range fetched separately.
    ///
    /// Separate because `topic_offset_range` refreshes metadata first, so
    /// calling it per row of a 500-topic list is 500 metadata refreshes.
    pub fn set_offsets(&mut self, earliest: Option<i64>, latest: Option<i64>) {
        self.earliest_offset = earliest;
        self.latest_offset = latest;
    }
}

/// One configuration entry.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntryDto {
    /// Key.
    pub name: String,
    /// Value. `None` when the broker redacted it.
    pub value: Option<String>,
    /// Where the value came from.
    pub source: String,
    /// Whether it was set explicitly rather than inherited.
    pub is_explicit: bool,
    /// Whether the broker redacted the value. Rendered as a redaction, never
    /// as an empty string.
    pub is_sensitive: bool,
    /// Whether the broker refuses to change it.
    pub read_only: bool,
    /// The broker's own documentation, for the tooltip.
    pub documentation: Option<String>,
}

impl From<&ConfigEntry> for ConfigEntryDto {
    fn from(entry: &ConfigEntry) -> Self {
        Self {
            name: entry.name.clone(),
            value: entry.value.clone(),
            source: format!("{:?}", entry.source),
            is_explicit: entry.source.is_explicit(),
            is_sensitive: entry.is_sensitive,
            read_only: entry.read_only,
            documentation: entry.documentation.clone(),
        }
    }
}

/// Render a config resource for the error side of an envelope.
pub fn config_resource_name(resource: &ConfigResource) -> String {
    format!("{:?}:{}", resource.resource_type, resource.name)
}

/// Whether a config source counts as explicitly set.
pub fn is_explicit(source: ConfigSource) -> bool {
    source.is_explicit()
}

/// One log directory on one broker.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogDirDto {
    /// Broker-local path.
    pub path: String,
    /// Total capacity, where the broker reports it.
    pub total_bytes: Option<i64>,
    /// Free capacity.
    pub usable_bytes: Option<i64>,
    /// Replicas stored here.
    pub replicas: Vec<LogDirReplicaDto>,
    /// The directory's own error, if it has one.
    pub error: Option<String>,
}

impl From<&LogDir> for LogDirDto {
    fn from(dir: &LogDir) -> Self {
        Self {
            path: dir.path.clone(),
            total_bytes: dir.total_bytes,
            usable_bytes: dir.usable_bytes,
            replicas: dir.replicas.iter().map(LogDirReplicaDto::from).collect(),
            error: dir.error.and_then(|code| code.name()).map(str::to_owned),
        }
    }
}

/// One replica in a log directory.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogDirReplicaDto {
    /// Topic.
    pub topic: String,
    /// Partition.
    pub partition: i32,
    /// Bytes on disk.
    pub size_bytes: i64,
    /// How far behind the log end this replica is.
    pub offset_lag: i64,
    /// Whether this is a future replica being moved in.
    pub is_future: bool,
}

impl From<&LogDirReplica> for LogDirReplicaDto {
    fn from(replica: &LogDirReplica) -> Self {
        Self {
            topic: replica.topic.clone(),
            partition: replica.partition,
            size_bytes: replica.size_bytes,
            offset_lag: replica.offset_lag,
            is_future: replica.is_future,
        }
    }
}

/// One row of the group list.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    /// Group id.
    pub group_id: String,
    /// State, as the broker names it.
    pub state: String,
    /// Group type. Empty on brokers too old to report one — which is not the
    /// same as unknown, and takes the classic path.
    pub group_type: String,
    /// Protocol type.
    pub protocol_type: String,
    /// Whether this group can be described at all.
    pub describable: bool,
}

impl From<&GroupListing> for GroupSummary {
    fn from(listing: &GroupListing) -> Self {
        Self {
            group_id: listing.group_id.clone(),
            state: render_group_state(&listing.state),
            group_type: listing.group_type.clone(),
            protocol_type: listing.protocol_type.clone(),
            describable: listing.describable(),
        }
    }
}

/// A described group.
///
/// Four kinds, not one struct with optional fields. `Unrecognized` is a
/// **successful** description of an undescribable group: it exists, it is
/// listed, and the UI can say what it is. Flattening the four into one
/// all-optional shape moves that knowledge somewhere the compiler cannot
/// check it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GroupDetail {
    /// A classic consumer group.
    #[serde(rename_all = "camelCase")]
    Classic {
        /// Group id.
        group_id: String,
        /// State.
        state: String,
        /// Protocol type.
        protocol_type: String,
        /// Assignment protocol.
        protocol: String,
        /// Members.
        members: Vec<GroupMember>,
    },
    /// A KIP-848 consumer group.
    #[serde(rename_all = "camelCase")]
    Consumer {
        /// Group id.
        group_id: String,
        /// State.
        state: String,
        /// Group epoch.
        group_epoch: i32,
        /// Assignment epoch.
        assignment_epoch: i32,
        /// Server-side assignor.
        assignor: String,
        /// Members.
        members: Vec<GroupMember>,
    },
    /// A share group.
    #[serde(rename_all = "camelCase")]
    Share {
        /// Group id.
        group_id: String,
        /// State.
        state: String,
        /// Group epoch.
        group_epoch: i32,
        /// Assignment epoch.
        assignment_epoch: i32,
        /// Server-side assignor.
        assignor: String,
        /// Members.
        members: Vec<GroupMember>,
    },
    /// A group of a kind this build has no schema for — a streams group,
    /// today. Successfully described as "exists, cannot be opened".
    #[serde(rename_all = "camelCase")]
    Unrecognized {
        /// Group id.
        group_id: String,
        /// The type string the broker reported.
        group_type: String,
        /// State.
        state: String,
    },
}

impl From<&GroupDescription> for GroupDetail {
    fn from(description: &GroupDescription) -> Self {
        match description {
            GroupDescription::Classic {
                group_id,
                state,
                protocol_type,
                protocol,
                members,
            } => Self::Classic {
                group_id: group_id.clone(),
                state: render_group_state(state),
                protocol_type: protocol_type.clone(),
                protocol: protocol.clone(),
                members: members.iter().map(GroupMember::from_classic).collect(),
            },
            GroupDescription::Consumer {
                group_id,
                state,
                group_epoch,
                assignment_epoch,
                assignor,
                members,
            } => Self::Consumer {
                group_id: group_id.clone(),
                state: render_group_state(state),
                group_epoch: *group_epoch,
                assignment_epoch: *assignment_epoch,
                assignor: assignor.clone(),
                members: members.iter().map(GroupMember::from_consumer).collect(),
            },
            GroupDescription::Share {
                group_id,
                state,
                group_epoch,
                assignment_epoch,
                assignor,
                members,
            } => Self::Share {
                group_id: group_id.clone(),
                state: render_group_state(state),
                group_epoch: *group_epoch,
                assignment_epoch: *assignment_epoch,
                assignor: assignor.clone(),
                members: members.iter().map(GroupMember::from_share).collect(),
            },
            GroupDescription::Unrecognized {
                group_id,
                group_type,
                state,
            } => Self::Unrecognized {
                group_id: group_id.clone(),
                group_type: group_type.clone(),
                state: render_group_state(state),
            },
        }
    }
}

/// One member of a group, in the shape all three describable kinds share.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    /// Member id.
    pub member_id: String,
    /// Static membership instance id.
    pub instance_id: Option<String>,
    /// Client id.
    pub client_id: String,
    /// Client host.
    pub client_host: String,
    /// Rack, where reported.
    pub rack_id: Option<String>,
    /// Member epoch, for the kinds that have one.
    pub member_epoch: Option<i32>,
    /// Topics the member subscribed to, where the protocol reports them.
    pub subscribed_topics: Vec<String>,
    /// The member's assignment, where the protocol reports it decoded.
    pub assignment: Vec<TopicPartitions>,
}

impl GroupMember {
    fn from_classic(member: &ClassicGroupMember) -> Self {
        Self {
            member_id: member.member_id.clone(),
            instance_id: member.group_instance_id.clone(),
            client_id: member.client_id.clone(),
            client_host: member.client_host.clone(),
            rack_id: None,
            member_epoch: None,
            subscribed_topics: Vec::new(),
            // The classic protocol carries an opaque, assignor-defined blob.
            // kaas-lib hands it over undecoded and kaas-ui does not guess at
            // it: a wrong assignment table is worse than no assignment table.
            assignment: Vec::new(),
        }
    }

    fn from_consumer(member: &ConsumerGroupMember) -> Self {
        Self {
            member_id: member.member_id.clone(),
            instance_id: member.instance_id.clone(),
            client_id: member.client_id.clone(),
            client_host: member.client_host.clone(),
            rack_id: member.rack_id.clone(),
            member_epoch: Some(member.member_epoch),
            subscribed_topics: member.subscribed_topics.clone(),
            assignment: member.assignment.iter().map(TopicPartitions::of).collect(),
        }
    }

    fn from_share(member: &ShareGroupMember) -> Self {
        Self {
            member_id: member.member_id.clone(),
            instance_id: None,
            client_id: member.client_id.clone(),
            client_host: member.client_host.clone(),
            rack_id: member.rack_id.clone(),
            member_epoch: Some(member.member_epoch),
            subscribed_topics: member.subscribed_topics.clone(),
            assignment: member.assignment.iter().map(TopicPartitions::of).collect(),
        }
    }
}

/// A topic and the partitions of it assigned to a member.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicPartitions {
    /// Topic.
    pub topic: String,
    /// Partitions.
    pub partitions: Vec<i32>,
}

impl TopicPartitions {
    fn of(pair: &(String, Vec<i32>)) -> Self {
        Self {
            topic: pair.0.clone(),
            partitions: pair.1.clone(),
        }
    }
}

/// A group's committed offset for one partition, with lag.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupOffset {
    /// Topic.
    pub topic: String,
    /// Partition.
    pub partition: i32,
    /// The committed offset, where there is one.
    pub committed_offset: Option<i64>,
    /// The partition's next offset.
    pub latest_offset: Option<i64>,
    /// Commit metadata.
    pub metadata: Option<String>,
    /// Lag, in its four distinguishable states.
    pub lag: Lag,
}

/// Lag has four states and they must not all render as `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Lag {
    /// The group has never committed here. Not zero lag — no data at all.
    NoCommit,
    /// The partition is empty, so there is nothing to be behind.
    EmptyPartition,
    /// Committed at the log end.
    CaughtUp,
    /// Behind by a known amount.
    #[serde(rename_all = "camelCase")]
    Lagging {
        /// How many records.
        records: i64,
    },
    /// The partition's end offset could not be read, so lag is unknown —
    /// which is not the same as zero.
    Unknown,
}

impl Lag {
    /// Classify a committed offset against a log end.
    pub fn of(committed: Option<i64>, earliest: Option<i64>, latest: Option<i64>) -> Self {
        match (committed, latest) {
            (None, _) => Self::NoCommit,
            (Some(_), None) => Self::Unknown,
            (Some(committed), Some(latest)) => {
                if earliest == Some(latest) {
                    return Self::EmptyPartition;
                }
                match latest.checked_sub(committed) {
                    Some(behind) if behind > 0 => Self::Lagging { records: behind },
                    Some(_) => Self::CaughtUp,
                    None => Self::Unknown,
                }
            }
        }
    }
}

/// Build a group offset row.
pub fn group_offset(
    topic: String,
    partition: i32,
    committed: Option<&CommittedOffset>,
    earliest: Option<i64>,
    latest: Option<i64>,
) -> GroupOffset {
    // Kafka's "no committed offset" sentinel is -1, and rendering it as an
    // offset would put a negative number in a column of positions.
    let committed_offset = committed.map(|c| c.offset).filter(|offset| *offset >= 0);
    GroupOffset {
        topic,
        partition,
        committed_offset,
        latest_offset: latest,
        metadata: committed
            .and_then(|c| c.metadata.clone())
            .filter(|m| !m.is_empty()),
        lag: Lag::of(committed_offset, earliest, latest),
    }
}

/// One record in the message browser.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Partition.
    pub partition: i32,
    /// Offset.
    pub offset: i64,
    /// Timestamp, in milliseconds.
    pub timestamp: i64,
    /// Whether the timestamp is the producer's or the broker's.
    pub timestamp_type: String,
    /// The key, rendered.
    pub key: Option<Payload>,
    /// The value, rendered. `None` is a tombstone, which is not the same as
    /// an empty value.
    pub value: Option<Payload>,
    /// Headers.
    pub headers: Vec<Header>,
    /// Whether the record was written transactionally.
    pub transactional: bool,
    /// Size of the value in bytes, before rendering.
    pub size_bytes: usize,
}

impl Message {
    /// Render a record, choosing how much of each payload to include.
    fn render(record: &Record, payload: fn(&[u8]) -> Payload) -> Self {
        Self {
            partition: record.partition,
            offset: record.offset,
            timestamp: record.timestamp,
            timestamp_type: render_timestamp_type(record.timestamp_type),
            key: record.key.as_ref().map(|bytes| payload(bytes)),
            value: record.value.as_ref().map(|bytes| payload(bytes)),
            headers: record
                .headers
                .iter()
                .map(|(name, value)| Header {
                    name: name.clone(),
                    value: value.as_ref().map(|bytes| payload(bytes)),
                })
                .collect(),
            transactional: record.transactional,
            size_bytes: record.payload_len(),
        }
    }

    /// A record for the one-shot tail, where a list of them is returned.
    pub fn of(record: &Record) -> Self {
        Self::render(record, Payload::of)
    }

    /// A record for the detail panel, where exactly one was asked for.
    pub fn full(record: &Record) -> Self {
        Self::render(record, Payload::full)
    }
}

impl From<&Record> for Message {
    fn from(record: &Record) -> Self {
        Self::of(record)
    }
}

/// A record header.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: Option<Payload>,
}

/// A key or value, rendered with the encoding that was used said out loud.
///
/// Auto-detection that cannot be seen is worse than none: the reader has to
/// know whether they are looking at text the producer wrote or at kaas-ui's
/// guess.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    /// `utf8` or `hex`.
    pub encoding: String,
    /// The rendering.
    pub text: String,
    /// Length in bytes of the original.
    pub bytes: usize,
    /// Whether `text` was cut short.
    pub truncated: bool,
}

/// Above this, a payload is cut short: one oversized record must not be able
/// to blow up a browser tab that asked for five hundred of them.
const MAX_PAYLOAD_CHARS: usize = 8192;

/// The ceiling on a payload that rides in a *stream*.
///
/// Much smaller than [`MAX_PAYLOAD_CHARS`], and not a tuning knob. A topic
/// carrying 1 KB values at ten thousand records a second is 10 MB/s the
/// browser would parse, hold in a ring buffer and never draw — the list shows
/// one truncated line per row whatever arrives. The rest is fetched for the
/// one record someone actually selected.
const PREVIEW_CHARS: usize = 256;

/// The ceiling on the one payload someone actually opened.
///
/// A whole megabyte, because this is the answer to "show me this record" and
/// cutting it at the list's budget would make the detail panel useless for the
/// large records that are the reason anyone opens it. Still a ceiling: a
/// response is not allowed to be as large as a producer felt like being.
const DETAIL_PAYLOAD_CHARS: usize = 1024 * 1024;

impl Payload {
    /// Render bytes as text where they are text, and as hex where they are not.
    pub fn of(bytes: &[u8]) -> Self {
        Self::rendered(bytes, MAX_PAYLOAD_CHARS)
    }

    /// The same rendering, cut to what a single list row can show.
    pub fn preview(bytes: &[u8]) -> Self {
        Self::rendered(bytes, PREVIEW_CHARS)
    }

    /// The same rendering, for the one record that was selected.
    pub fn full(bytes: &[u8]) -> Self {
        Self::rendered(bytes, DETAIL_PAYLOAD_CHARS)
    }

    fn rendered(bytes: &[u8], ceiling: usize) -> Self {
        let len = bytes.len();
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                let (text, truncated) = truncate(text, ceiling);
                Self {
                    encoding: "utf8".to_owned(),
                    text,
                    bytes: len,
                    truncated,
                }
            }
            Err(_) => {
                let mut hex = String::new();
                let mut truncated = false;
                for byte in bytes {
                    if hex.len() >= ceiling {
                        truncated = true;
                        break;
                    }
                    // Writing into a String cannot fail.
                    let _ = write!(hex, "{byte:02x}");
                }
                Self {
                    encoding: "hex".to_owned(),
                    text: hex,
                    bytes: len,
                    truncated,
                }
            }
        }
    }
}

fn truncate(text: &str, ceiling: usize) -> (String, bool) {
    if text.len() <= ceiling {
        return (text.to_owned(), false);
    }
    let mut end = ceiling;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    match text.get(..end) {
        Some(head) => (head.to_owned(), true),
        None => (String::new(), true),
    }
}

/// One row of the message list, as it crosses an SSE connection.
///
/// Two variants, never conflated. A batch that would not decode at the
/// protocol level is a **row**: kaas-lib's decoder keeps going past it, and
/// surfacing it is the entire reason that design exists. Folding it into the
/// stream's `error` event would throw away the one thing the reader needs,
/// which is *where* in the topic the damage is.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StreamRow {
    /// A record that decoded.
    Record(StreamRecord),
    /// A batch that did not. The scan continued past it.
    Malformed(MalformedRow),
}

/// A decoded record, previewed rather than whole.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamRecord {
    /// Partition.
    pub partition: i32,
    /// Offset.
    pub offset: i64,
    /// Timestamp, in milliseconds.
    pub timestamp: i64,
    /// Whether the timestamp is the producer's or the broker's.
    pub timestamp_type: String,
    /// The key, cut to [`PREVIEW_CHARS`]. `None` is a keyless record.
    pub key: Option<Payload>,
    /// The value, cut to [`PREVIEW_CHARS`]. `None` is a **tombstone**, which
    /// is not the same as an empty value — compaction turns on the difference.
    pub value: Option<Payload>,
    /// Whether the record was written transactionally.
    pub transactional: bool,
}

/// A batch that would not decode, as a row.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MalformedRow {
    /// Partition.
    pub partition: i32,
    /// The first offset the batch claimed.
    pub offset: i64,
    /// The last offset it claimed, or its own base where the header did not
    /// say — so the row always names a range it can render.
    pub last_offset: i64,
    /// Why it did not decode.
    pub reason: String,
}

impl StreamRow {
    /// A row for a decoded record.
    pub fn of(record: &Record) -> Self {
        Self::Record(StreamRecord {
            partition: record.partition,
            offset: record.offset,
            timestamp: record.timestamp,
            timestamp_type: render_timestamp_type(record.timestamp_type),
            key: record.key.as_ref().map(|bytes| Payload::preview(bytes)),
            value: record.value.as_ref().map(|bytes| Payload::preview(bytes)),
            transactional: record.transactional,
        })
    }

    /// A row for a batch that did not decode.
    pub fn malformed(
        partition: i32,
        offset: i64,
        last_offset: Option<i64>,
        reason: impl std::fmt::Display,
    ) -> Self {
        Self::Malformed(MalformedRow {
            partition,
            offset,
            // A header that did not survive leaves no end; the batch still
            // covers at least its own base offset, and a range of one is
            // honest where a range of zero would render as a gap.
            last_offset: last_offset.unwrap_or(offset).max(offset),
            reason: reason.to_string(),
        })
    }

    /// `{partition}-{offset}` — the id the whole feature keys on, in the row,
    /// in React, in the query cache and in the SSE `id:` field alike.
    pub fn id(&self) -> String {
        match self {
            Self::Record(record) => format!("{}-{}", record.partition, record.offset),
            Self::Malformed(row) => format!("{}-{}", row.partition, row.offset),
        }
    }
}

/// How far a bounded scan has got.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamProgress {
    /// `0.0`–`1.0`, or `None` before the total is known. A live tail has no
    /// end, so it never has a fraction.
    pub fraction: Option<f64>,
    /// Records emitted so far.
    pub records_emitted: u64,
    /// Records read, including those a filter dropped.
    pub records_scanned: u64,
    /// Batches that did not decode.
    pub malformed_batches: u64,
    /// Partitions still producing.
    pub partitions_active: usize,
    /// Whether the merge is no longer producing a total order across
    /// partitions.
    pub ordering_degraded: bool,
    /// Roughly how many records apart two partitions may be reordered.
    ///
    /// Derived from the scan's buffer ceiling spread over the partitions still
    /// running: the merge picks the oldest buffered head, so the window it can
    /// see is what bounds how far out of order the result can be. It is a
    /// caveat to render next to the list, not a promise.
    pub reorder_window: usize,
}

/// Where a stream is in its life.
///
/// A backward window emits `seeking` and nothing else until the whole walk
/// finishes, because [`kafka_read::tail`] returns a `Vec` — there are no
/// partial results to show, and a spinner that never changes looks identical
/// to a hang.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StreamPhase {
    /// Resolving offsets, or walking backwards. Nothing has been emitted.
    Seeking,
    /// Records are arriving.
    Streaming,
    /// The window is exhausted, or the stream hit its lifetime. The client
    /// decides whether to reopen.
    Done,
}

/// What an instant actually resolved to, per partition.
///
/// Emitted for the two time modes and for no other reason than that a
/// timestamp seek can be answered correctly and still not land where the
/// reader expected. `ListOffsets` reports "the first offset at or after this
/// instant", and a broker with no timestamp index answers **nothing at all** —
/// a legitimate response that is indistinguishable from "nothing was written
/// after that time". kaas-ui cannot tell those apart and must not guess; what
/// it can do is show the answer it got, so an empty window reads as "this
/// cluster resolved 14:30 to nothing" rather than as a broken seek.
///
/// The `kaas` broker in the development environment does exactly this, and
/// Strimzi does not. See `docs/reference/environment.md`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSeek {
    /// The instant that was asked about, in epoch milliseconds.
    pub timestamp: i64,
    /// What each partition answered.
    pub partitions: Vec<ResolvedPartition>,
    /// Whether no partition resolved to an offset.
    ///
    /// Precomputed because it is the case worth saying out loud, and a UI that
    /// has to derive it will derive it in three places and differently.
    pub unresolved: bool,
}

/// One partition's answer to a timestamp lookup.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPartition {
    /// Partition.
    pub partition: i32,
    /// The offset the instant resolved to, or `None` where the broker
    /// reported none.
    pub offset: Option<i64>,
    /// The timestamp of the record at that offset, where the broker says.
    pub timestamp: Option<i64>,
    /// Why the lookup failed, where it failed rather than answered.
    pub error: Option<String>,
}

/// How many records the server dropped rather than stall the scan.
///
/// Silently losing records in a debugging tool is worse than showing a gap,
/// so this is emitted whenever the count changes and never suppressed.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    /// Records dropped since the stream opened.
    pub count: u64,
}

/// The answer to "show me this one record".
///
/// Tagged the same way [`StreamRow`] is, and for the same reason: the row a
/// reader selected might be a batch that would not decode, and the panel that
/// opens has to be able to say so with the raw bytes rather than showing an
/// empty record.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MessageDetail {
    /// The record, with its key, value and headers whole.
    Record(Box<Message>),
    /// The batch that covered the offset, as hex.
    Malformed(MalformedDetail),
}

/// A batch that would not decode, with its bytes.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MalformedDetail {
    /// Partition.
    pub partition: i32,
    /// The first offset the batch claimed.
    pub offset: i64,
    /// The last offset it claimed.
    pub last_offset: i64,
    /// Why it did not decode.
    pub reason: String,
    /// The raw batch, as hex. The only way to see what is actually on disk.
    pub raw: Payload,
}

impl MessageDetail {
    /// A detail for a record.
    pub fn of(record: &Record) -> Self {
        Self::Record(Box::new(Message::full(record)))
    }
}

fn render_timestamp_type(timestamp_type: TimestampType) -> String {
    match timestamp_type {
        TimestampType::Creation => "createTime".to_owned(),
        TimestampType::LogAppend => "logAppendTime".to_owned(),
    }
}

/// Milliseconds, saturating rather than wrapping.
pub fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Render a group state the way the broker named it.
fn render_group_state(state: &GroupState) -> String {
    match state {
        GroupState::Other(raw) => raw.clone(),
        other => format!("{other:?}"),
    }
}

/// A topic uuid, or `None` where the cluster does not report one.
fn render_topic_id(id: &TopicId) -> Option<String> {
    if id.is_zero() {
        return None;
    }
    let mut out = String::with_capacity(36);
    for (index, byte) in id.as_bytes().iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{byte:02x}");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_has_four_distinguishable_states() {
        assert_eq!(Lag::of(None, Some(0), Some(10)), Lag::NoCommit);
        assert_eq!(Lag::of(Some(0), Some(0), Some(0)), Lag::EmptyPartition);
        assert_eq!(Lag::of(Some(10), Some(0), Some(10)), Lag::CaughtUp);
        assert_eq!(
            Lag::of(Some(4), Some(0), Some(10)),
            Lag::Lagging { records: 6 }
        );
        assert_eq!(Lag::of(Some(4), Some(0), None), Lag::Unknown);
    }

    #[test]
    fn lag_states_serialise_distinguishably() {
        let json = serde_json::to_value(Lag::NoCommit).unwrap();
        assert_eq!(json["state"], "noCommit");
        let json = serde_json::to_value(Lag::Lagging { records: 6 }).unwrap();
        assert_eq!(json["state"], "lagging");
        assert_eq!(json["records"], 6);
    }

    #[test]
    fn a_cluster_reporting_no_topic_ids_gets_no_column() {
        assert_eq!(render_topic_id(&TopicId::ZERO), None);
        let id = TopicId::from_bytes([
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ]);
        assert_eq!(
            render_topic_id(&id).as_deref(),
            Some("01234567-89ab-cdef-0123-456789abcdef")
        );
    }

    #[test]
    fn text_payloads_are_text_and_binary_payloads_are_hex() {
        let text = Payload::of(b"hello");
        assert_eq!(text.encoding, "utf8");
        assert_eq!(text.text, "hello");

        let binary = Payload::of(&[0xff, 0x00, 0x10]);
        assert_eq!(binary.encoding, "hex");
        assert_eq!(binary.text, "ff0010");
        assert_eq!(binary.bytes, 3);
    }

    #[test]
    fn an_oversized_payload_is_cut_at_a_char_boundary() {
        let long = "é".repeat(MAX_PAYLOAD_CHARS);
        let payload = Payload::of(long.as_bytes());
        assert!(payload.truncated);
        assert!(payload.text.len() <= MAX_PAYLOAD_CHARS);
    }

    #[test]
    fn a_never_committed_offset_is_not_rendered_as_minus_one() {
        let committed = CommittedOffset {
            offset: -1,
            leader_epoch: None,
            metadata: None,
        };
        let row = group_offset("orders".into(), 0, Some(&committed), Some(0), Some(10));
        assert_eq!(row.committed_offset, None);
        assert_eq!(row.lag, Lag::NoCommit);
    }
}

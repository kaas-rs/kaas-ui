//! The types that reach the wire.
//!
//! kaas-lib's rule — no upstream type in a public signature — one level up.
//! No `kafka_meta::TopicInfo` appears in a `utoipa` schema; the `From` impls
//! below are the boundary, and they are the only place a library bump can
//! break, rather than every screen at once.

use std::collections::{BTreeMap, HashMap};
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

use kaas_ui_auth::{Access, Action, Principal, Resource};
use kaas_ui_serde::{DETAIL_PAYLOAD_CHARS, MAX_PAYLOAD_CHARS, PREVIEW_CHARS};

use crate::config::{EnvironmentEntry, ResourceEntry, ResourceKind};
use crate::decode::{DecodedRecord, PayloadDecoder};
use crate::health::{ClusterHealth, ClusterStatus};
use crate::registry::{ClusterHandle, Registry};

/// A key or value, rendered with the codec that was used said out loud.
///
/// Defined in `kaas-ui-serde` rather than here, and re-exported so the wire
/// contract still reads as one document. That crate is where the conversion
/// from an upstream value happens — an `apache_avro::types::Value` becomes
/// `serde_json`, then text — so this **is** kaas-ui's own decoded-value type
/// rather than a rename of somebody else's.
pub use kaas_ui_serde::Payload;

/// One schema registry, as the browser and the fleet render it.
///
/// A view of a **registry**, reached through a cluster. Two clusters that
/// reference `dev` show the same subjects, so the card says which registry is
/// answering rather than implying the subjects belong to the cluster whose nav
/// you arrived through.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCard {
    /// The configured id, which is what a cluster's `schema_registry:` names.
    pub id: String,
    /// The name to render.
    pub name: String,
    /// Where it is. Shown because a registry that is answering the wrong API
    /// is diagnosed by looking at its url.
    pub url: String,
    /// Whether it has been reached, and how it failed if it has not.
    pub status: kaas_ui_serde::RegistryStatus,
    /// What went wrong, where something did.
    pub error: Option<String>,
}

impl RegistryCard {
    /// Project a live handle.
    #[must_use]
    pub fn of(registry: &kaas_ui_serde::RegistryHandle) -> Self {
        let health = registry.health();
        Self {
            id: registry.id().to_owned(),
            name: registry.name().to_owned(),
            url: registry.url().to_owned(),
            status: health.status(),
            error: health.error().map(str::to_owned),
        }
    }
}

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
    /// The named ways to sign in, if this deployment lists any.
    ///
    /// Empty is the common case and means "one unlabelled sign-in button" —
    /// the provider is then responsible for asking which connector, which is
    /// what Dex does when it has more than one. A deployment that would rather
    /// ask that question itself, on its own screen, lists them in `auth
    /// .connectors`, and these are what it listed.
    ///
    /// Deliberately here rather than on its own endpoint. It is the same
    /// question `login_available` answers — *what should the sign-in control
    /// look like* — and splitting it across two fetches would let the two
    /// halves disagree while one of them was stale.
    pub connectors: Vec<LoginConnector>,
}

/// One way to sign in, as the sign-in screen renders it.
///
/// The id is opaque and only means something to the provider. Nothing in
/// kaas-ui interprets it — see [`kaas_ui_auth::Connector`], the configuration
/// this mirrors.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginConnector {
    /// Passed back as `/auth/login?connector=<id>`.
    pub id: String,
    /// What the button says.
    pub name: String,
}

impl From<&kaas_ui_auth::Connector> for LoginConnector {
    fn from(connector: &kaas_ui_auth::Connector) -> Self {
        Self {
            id: connector.id.clone(),
            name: connector.name.clone(),
        }
    }
}

impl Identity {
    /// Project a resolved caller.
    ///
    /// `provider` is the deployment's identity provider, if it has one. Both
    /// [`login_available`](Self::login_available) and
    /// [`connectors`](Self::connectors) are read off it here rather than passed
    /// in separately, so there is no arrangement of arguments that reports a
    /// connector list for a deployment that cannot log anybody in.
    pub fn of(
        who: &Principal,
        access: &Access,
        enforcing: bool,
        provider: Option<&kaas_ui_auth::Provider>,
    ) -> Self {
        Self {
            authenticated: who.is_authenticated(),
            subject: who.subject().to_owned(),
            display_name: who.display_name().to_owned(),
            roles: access.role_names().map(str::to_owned).collect(),
            enforcing,
            login_available: provider.is_some(),
            connectors: provider
                .map(|provider| provider.connectors().iter().map(Into::into).collect())
                .unwrap_or_default(),
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
    /// The environment holding it — the first segment of every URL that
    /// reaches it, and half of what identifies it.
    pub environment: String,
    /// The configured id, unique within that environment.
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
    /// `value_type` because the map is keyed by an enum, which utoipa cannot
    /// name on its own — left alone it emits a `$ref` to a schema that does
    /// not exist, and the generated client is broken in a way nothing in Rust
    /// notices.
    #[schema(value_type = std::collections::BTreeMap<Resource, Vec<Action>>)]
    pub grants: std::collections::BTreeMap<Resource, std::collections::BTreeSet<Action>>,
    /// The schema registry this cluster references, by its configured id.
    ///
    /// `None` is a normal path — a kaas instance beside a Strimzi cluster in
    /// the same environment — and the sidebar reads it to decide whether a
    /// schemas item exists at all, rather than offering one that explains its
    /// own absence.
    pub schema_registry: Option<String>,
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
            environment: handle.environment.clone(),
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
            grants: who.permissions(&handle.id, &handle.labels),
            schema_registry: handle
                .schema_registry()
                .map(|registry| registry.id().to_owned()),
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

/// One thing in an environment that is not a Kafka cluster.
///
/// **There is no status field, and that is the design.** kaas-ui dials none of
/// these: it knows a schema registry is configured, not that it is up. A card
/// carrying `status: "ready"` because a URL was typed correctly would be the
/// one thing a fleet view must never do — see [`ClusterStatus`], whose
/// `Connecting` exists so that unknown is not rendered as healthy. The UI says
/// "not probed" because that is the whole truth kaas-ui has.
///
/// Phase 6 connects to the schema registry, and *that* is when this type earns
/// a health field — one filled in from an attempt, on the one kind that makes
/// attempts.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCard {
    /// The configured id.
    pub id: String,
    /// The name to render.
    pub name: String,
    /// What it is: the icon and the wording, nothing else.
    pub kind: ResourceKind,
    /// Where it is, when the configuration says.
    pub endpoint: Option<String>,
    /// One line of context.
    pub note: Option<String>,
    /// Labels, including the `env` its section is keyed by.
    pub labels: BTreeMap<String, String>,
}

impl ResourceCard {
    /// Project an inventory entry, given the environment it sits in.
    ///
    /// The environment is a parameter rather than a field on the entry now:
    /// nesting says where it is, so the only way for the card's `env` label to
    /// disagree with its section would be for this call to be passed the wrong
    /// one, and there is one caller.
    pub fn of(environment: &str, entry: &ResourceEntry) -> Self {
        Self {
            id: entry.id.clone(),
            name: entry.display_name().to_owned(),
            kind: entry.kind,
            endpoint: entry.endpoint.clone(),
            note: entry.note.clone(),
            labels: entry.effective_labels(environment),
        }
    }
}

/// One section of the fleet: an environment and everything in it.
///
/// Assembled on the server rather than grouped in the browser, because the
/// order is configuration — declared environments run in declared order, and
/// nothing else can recover "dev before staging before prod" from three
/// strings.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSection {
    /// The configured id, and the first segment of every URL beneath it.
    ///
    /// There is no longer an unnamed section: an environment is a declared
    /// block, so a cluster cannot arrive in one nobody wrote down.
    pub id: String,
    /// The name to render.
    pub name: String,
    /// The declared description, where there is one.
    pub description: Option<String>,
    /// The Kafka clusters in it, in configured order.
    pub clusters: Vec<ClusterCard>,
    /// The schema registries in it that this caller may read.
    pub schema_registries: Vec<EnvironmentRegistry>,
    /// Everything else, in configured order.
    pub resources: Vec<ResourceCard>,
}

/// A schema registry inside an environment.
///
/// Distinct from the [`ResourceCard`] beside it: that is an inventory line an
/// operator typed, with its own id and no connection to anything. This one is
/// the registry that decodes payloads, and it is addressable —
/// `/api/environments/{env}/schema-registries/{id}`.
///
/// It gained that URL when environments did. A registry id is not a global
/// namespace even so: it is reachable only under an environment the caller can
/// already see, and only when they can see a cluster there that references it,
/// which is the same rule `Registry::schema_registry` enforces on the way in.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRegistry {
    /// Id, name, url and health.
    pub registry: RegistryCard,
    /// The clusters in this environment that resolve payloads against it.
    ///
    /// Rendered as "who uses this", and it is only ever the visible ones —
    /// the list is built from cards this caller already received.
    pub used_by: Vec<String>,
}

impl EnvironmentSection {
    /// Project one environment for one caller, or `None` if they see nothing
    /// in it.
    ///
    /// Takes the cards rather than the handles because building one nudges an
    /// unreachable cluster to retry, and a function that decides page layout
    /// should not also be poking connectors.
    ///
    /// **An empty section is `None`**, which is a visibility property and not
    /// tidiness: rendering "Production" with nothing under it tells a caller
    /// who may not see prod that prod exists, and the 404-not-403 rule that
    /// keeps cluster ids unenumerable would have been undone one heading up.
    pub fn of(
        entry: &EnvironmentEntry,
        cards: Vec<ClusterCard>,
        registry: &Registry,
        who: &Access,
    ) -> Option<Self> {
        let resources: Vec<ResourceCard> = registry
            .resources_visible(who)
            .filter(|(environment, _)| *environment == entry.id)
            .map(|(environment, resource)| ResourceCard::of(environment, resource))
            .collect();

        if cards.is_empty() && resources.is_empty() {
            return None;
        }

        Some(Self {
            id: entry.id.clone(),
            name: entry.display_name().to_owned(),
            description: entry.description.clone(),
            schema_registries: EnvironmentRegistry::of(entry, &cards, registry, who),
            clusters: cards,
            resources,
        })
    }

    /// Every environment this caller can see, in declaration order.
    ///
    /// Declaration order is the whole reason this is assembled on the server:
    /// "dev before staging before prod" is configuration, and nothing in the
    /// browser can recover it from three strings.
    pub fn arrange(cards: Vec<ClusterCard>, registry: &Registry, who: &Access) -> Vec<Self> {
        let mut by_environment: BTreeMap<String, Vec<ClusterCard>> = BTreeMap::new();
        for card in cards {
            by_environment
                .entry(card.environment.clone())
                .or_default()
                .push(card);
        }

        registry
            .environments()
            .iter()
            .filter_map(|entry| {
                let members = by_environment.remove(&entry.id).unwrap_or_default();
                Self::of(entry, members, registry, who)
            })
            .collect()
    }
}

impl EnvironmentRegistry {
    /// The registries of one environment that this caller may read.
    ///
    /// Listed from the *configuration* rather than from what the clusters
    /// happen to reference, so a registry declared beside them is visible as
    /// itself. Whether this caller may see it is
    /// [`Registry::schema_registry`]'s decision and only its — the two used to
    /// each hold half of the rule and disagreed about a registry nobody
    /// references, which is exactly the case a reader most needs to see.
    ///
    /// `used_by` stays a *display* field: the visible clusters that decode
    /// against it, empty when none do. An empty list is a real answer here —
    /// "declared, and nothing uses it" — not a reason to drop the row.
    fn of(
        entry: &EnvironmentEntry,
        members: &[ClusterCard],
        registry: &Registry,
        who: &Access,
    ) -> Vec<Self> {
        entry
            .schema_registries
            .iter()
            .filter_map(|declared| {
                let handle = registry.schema_registry(&entry.id, &declared.id, who)?;
                let used_by: Vec<String> = members
                    .iter()
                    .filter(|card| card.schema_registry.as_deref() == Some(declared.id.as_str()))
                    .filter(|card| {
                        card.grants
                            .get(&Resource::Topic)
                            .is_some_and(|actions| actions.contains(&Action::View))
                    })
                    .map(|card| card.id.clone())
                    .collect();
                Some(Self {
                    registry: RegistryCard::of(handle),
                    used_by,
                })
            })
            .collect()
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
    /// Records retained across every partition, when metrics were asked for.
    ///
    /// `latest - earliest` summed, which is what is *retained* rather than
    /// what was ever written — the same distinction the partition table draws.
    /// `None` when the metric was not requested, and also when any one
    /// partition failed to answer: a sum missing a partition is a smaller
    /// number, not a marked one, and nothing in the column would say so.
    pub message_count: Option<i64>,
    /// Bytes on disk for one copy, when metrics were asked for.
    pub logical_bytes: Option<i64>,
    /// Bytes on disk across replicas.
    pub replicated_bytes: Option<i64>,
    /// Which subjects in the cluster's registry name this topic, when
    /// `?schemas=true` was asked for.
    ///
    /// `None` is "not answered" and not "none registered": the question was not
    /// asked, the cluster reads no registry, or the registry would not hand
    /// over its subject list. A topic with no schema is `Some` with both sides
    /// empty, which is what lets a column say `—` on the topics that have none
    /// without saying it on the ones nobody asked about.
    pub schemas: Option<TopicSchemas>,
}

/// The subjects of one topic, by the side of the record they decode.
///
/// Read from the subject *names* alone, which resolves `TopicNameStrategy` and
/// nothing else. `{topic}-{record}` hides its seam in the schema rather than in
/// the name — recovering it is a registry call per subject, which is a cost
/// that scales with the registry to fill a column that scales with the page.
/// The topic page answers that one: it searches for the topic and describes the
/// handful of subjects that come back. See [`kaas_ui_serde::SubjectNaming`].
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicSchemas {
    /// The registry the subjects are registered in, addressed within the
    /// environment the request already names.
    pub registry: String,
    /// `{topic}-key`, where it is registered.
    pub key: Option<String>,
    /// `{topic}-value`, where it is registered.
    ///
    /// Both sides, because a key schema and a value schema are two subjects and
    /// showing whichever sorted first would be picking one at random.
    pub value: Option<String>,
}

/// The smallest replica count across partitions, which is what anyone means
/// by "replication factor".
///
/// The minimum and not the first partition's or the maximum — a topic
/// mid-reassignment has partitions at different counts, and the honest single
/// number is the guarantee every partition actually meets. One implementation,
/// because [`TopicSummary`] and [`TopicDetail`] both carry the answer and two
/// copies of a *decision* drift apart in a way two copies of arithmetic do not.
fn replication_factor(topic: &TopicInfo) -> usize {
    topic
        .partitions
        .iter()
        .map(|p| p.replicas.len())
        .min()
        .unwrap_or(0)
}

/// Partitions with no leader or an offline replica.
fn offline_partition_count(topic: &TopicInfo) -> usize {
    topic
        .partitions
        .iter()
        .filter(|p| p.leader.is_none() || !p.offline_replicas.is_empty())
        .count()
}

/// Partitions whose ISR is short.
fn under_replicated_partition_count(topic: &TopicInfo) -> usize {
    topic
        .partitions
        .iter()
        .filter(|p| p.under_replicated())
        .count()
}

impl TopicSummary {
    /// Build from the snapshot's view of a topic.
    pub fn of(topic: &TopicInfo) -> Self {
        Self {
            name: topic.name.clone(),
            topic_id: render_topic_id(&topic.topic_id),
            internal: topic.internal,
            partition_count: topic.partitions.len(),
            replication_factor: replication_factor(topic),
            offline_partition_count: offline_partition_count(topic),
            under_replicated_partition_count: under_replicated_partition_count(topic),
            message_count: None,
            logical_bytes: None,
            replicated_bytes: None,
            schemas: None,
        }
    }

    /// Attach sizes from `topic_sizes()`.
    ///
    /// A setter rather than a consuming `with_size`: enrichment walks a page of
    /// rows it already owns, and a consuming builder there costs a full clone
    /// of the row — two `String`s and six counters — to write two `i64`s.
    /// `Partition::set_offsets` is the same shape for the same reason.
    pub fn set_size(&mut self, size: &TopicSize) {
        self.logical_bytes = Some(size.logical_bytes);
        self.replicated_bytes = Some(size.replicated_bytes);
    }

    /// Attach the retained record count.
    pub fn set_message_count(&mut self, records: i64) {
        self.message_count = Some(records);
    }

    /// Attach the subjects naming this topic, empty sides included.
    ///
    /// Called for every row of a page that asked, because "the registry holds
    /// nothing for this topic" is an answer and has to arrive as one.
    pub fn set_schemas(&mut self, schemas: TopicSchemas) {
        self.schemas = Some(schemas);
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
    /// The smallest replica count across partitions — the same rule, and the
    /// same implementation, as [`TopicSummary::replication_factor`].
    ///
    /// Carried here so the overview card does not re-derive it in TypeScript:
    /// the minimum-across-partitions rule is a decision, and a decision that
    /// exists twice in two languages drifts.
    pub replication_factor: usize,
    /// Partitions with no leader or an offline replica.
    pub offline_partition_count: usize,
    /// Partitions whose ISR is short.
    pub under_replicated_partition_count: usize,
    /// Records retained across every partition, when offsets were fetched.
    ///
    /// `latest - earliest` summed — and absent, not smaller, when any
    /// partition is missing an end: a sum short one partition is an unmarked
    /// wrong number. The same rule `TopicSummary` applies, in the same place.
    pub message_count: Option<i64>,
    /// Bytes on disk for one copy, when `?size=true` asked for them.
    pub logical_bytes: Option<i64>,
    /// Bytes on disk across replicas.
    ///
    /// `None` means the caller did not ask, or `DescribeLogDirs` did not
    /// answer for this topic — never that the topic is empty. A zero would be
    /// a claim, and this is the absence of one.
    pub replicated_bytes: Option<i64>,
    /// Log-directory entries holding a copy of this topic.
    ///
    /// One per replica per directory, which is what kafbat-ui labels a
    /// "segment count" — `DescribeLogDirs` reports no segment files at all,
    /// so the name is borrowed and wrong. Carried under the name it earns,
    /// and future replicas count: a directory move in flight is an entry on a
    /// disk, whatever the totals above exclude.
    pub log_dir_entry_count: Option<usize>,
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
            replication_factor: replication_factor(topic),
            offline_partition_count: offline_partition_count(topic),
            under_replicated_partition_count: under_replicated_partition_count(topic),
            message_count: None,
            logical_bytes: None,
            replicated_bytes: None,
            log_dir_entry_count: None,
        }
    }

    /// Derive the retained record count from the offsets already attached.
    ///
    /// Called after `set_offsets` has run over the partitions; before that,
    /// every partition is missing both ends and the answer is rightly `None`.
    /// A partition missing either end makes the whole number absent rather
    /// than smaller — see the field's own doc for why.
    pub fn set_message_count_from_offsets(&mut self) {
        let mut total: i64 = 0;
        for partition in &self.partitions {
            match (partition.earliest_offset, partition.latest_offset) {
                (Some(low), Some(high)) => {
                    total = total.saturating_add(high.saturating_sub(low));
                }
                _ => return,
            }
        }
        self.message_count = Some(total);
    }

    /// Attach sizes from `topic_sizes()`.
    ///
    /// A setter rather than a consuming `with_size`, for the same reason
    /// `TopicSummary::set_size` is one: the caller already owns the row.
    pub fn set_size(&mut self, size: &TopicSize) {
        self.logical_bytes = Some(size.logical_bytes);
        self.replicated_bytes = Some(size.replicated_bytes);
        self.log_dir_entry_count = Some(size.replicas.len());

        // Joined by index rather than by position: `size.partitions` holds one
        // entry per partition *some broker reported a copy of*, which is not
        // the same list as the describe's, and zipping the two would put one
        // partition's bytes on another's row.
        let by_index: HashMap<i32, i64> = size
            .partitions
            .iter()
            .map(|partition| (partition.partition, partition.replicated_bytes))
            .collect();
        for partition in &mut self.partitions {
            if let Some(bytes) = by_index.get(&partition.partition) {
                partition.set_size(*bytes);
            }
        }

        // The worst follower per partition, from rows the same describe
        // already paid for. `offset_lag` was fetched per broker and rendered
        // nowhere against a topic, and it is the direct answer to "how far
        // behind is this replica" — which the table otherwise only implies
        // via ISR membership. Leaders lag nothing by definition, and a future
        // replica's lag describes a directory move rather than replication.
        let mut worst: HashMap<i32, i64> = HashMap::new();
        for replica in size
            .replicas
            .iter()
            .filter(|r| !r.is_leader && !r.is_future)
        {
            let entry = worst.entry(replica.partition).or_insert(0);
            *entry = (*entry).max(replica.offset_lag);
        }
        for partition in &mut self.partitions {
            if let Some(lag) = worst.get(&partition.partition) {
                partition.max_follower_lag = Some(*lag);
            }
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
    /// Every non-future copy of this partition summed, when `?size=true`
    /// asked for it — what the disks hold for this one partition.
    ///
    /// The replicated figure rather than the leader's copy, because the
    /// question a per-partition size answers is "which one is the big one",
    /// and because the leader's copy reads `0` on a leaderless partition
    /// rather than declining to answer.
    pub replicated_bytes: Option<i64>,
    /// The worst follower's offset lag, when `?size=true` asked for log dirs.
    ///
    /// `None` when sizes were not fetched *and* on a partition with no
    /// followers — a single-copy partition has nobody to lag. `Some(0)` is a
    /// claim: every follower is caught up.
    pub max_follower_lag: Option<i64>,
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
            replicated_bytes: None,
            max_follower_lag: None,
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

    /// Attach the on-disk size, fetched separately for the same reason.
    pub fn set_size(&mut self, replicated: i64) {
        self.replicated_bytes = Some(replicated);
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
    /// Render a record whose payloads have already been decoded.
    ///
    /// Decoding is separate because it is **async** — it can resolve a schema
    /// id against a registry — and because the payload filter reads the
    /// decoded value before this row is built, or in place of building it.
    pub fn render(record: &Record, decoded: DecodedRecord) -> Self {
        Self {
            partition: record.partition,
            offset: record.offset,
            timestamp: record.timestamp,
            timestamp_type: render_timestamp_type(record.timestamp_type),
            key: decoded.key.map(|decoded| decoded.payload),
            value: decoded.value.map(|decoded| decoded.payload),
            headers: decoded.headers,
            transactional: record.transactional,
            size_bytes: record.payload_len(),
        }
    }

    /// A record for the one-shot tail, where a list of them is returned.
    pub async fn of(record: &Record, decoder: &PayloadDecoder) -> Self {
        Self::render(record, decoder.record(record, MAX_PAYLOAD_CHARS).await)
    }

    /// A record for the detail panel, where exactly one was asked for.
    pub async fn full(record: &Record, decoder: &PayloadDecoder) -> Self {
        Self::render(record, decoder.record(record, DETAIL_PAYLOAD_CHARS).await)
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
    ///
    /// Boxed because a decoded payload is much larger than a malformed row —
    /// it carries the schema it resolved against and the bytes it came from —
    /// and a `Vec<StreamRow>` of a thousand rows should not pay a record's
    /// size for every batch that did not decode.
    Record(Box<StreamRecord>),
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
    /// A row for a record whose payloads have already been decoded.
    ///
    /// Split from [`Self::of`] because the payload filter runs *between*
    /// decoding and rendering: it searches the decoded value, and a record it
    /// rejects never becomes a row at all.
    #[must_use]
    pub fn render(record: &Record, decoded: DecodedRecord) -> Self {
        Self::Record(Box::new(StreamRecord {
            partition: record.partition,
            offset: record.offset,
            timestamp: record.timestamp,
            timestamp_type: render_timestamp_type(record.timestamp_type),
            key: decoded.key.map(|decoded| decoded.payload),
            value: decoded.value.map(|decoded| decoded.payload),
            transactional: record.transactional,
        }))
    }

    /// A row for a decoded record, cut to what a single list row can show.
    pub async fn of(record: &Record, decoder: &PayloadDecoder) -> Self {
        Self::render(record, decoder.record(record, PREVIEW_CHARS).await)
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
    Malformed(Box<MalformedDetail>),
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
    pub async fn of(record: &Record, decoder: &PayloadDecoder) -> Self {
        Self::Record(Box::new(Message::full(record, decoder).await))
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

    use crate::config::Config;

    fn partition_info(index: i32, replicas: Vec<i32>, isr: Vec<i32>) -> PartitionInfo {
        PartitionInfo {
            partition: index,
            leader: replicas.first().copied(),
            leader_epoch: 0,
            replicas,
            isr,
            offline_replicas: Vec::new(),
            error: None,
        }
    }

    fn topic_info(partitions: Vec<PartitionInfo>) -> TopicInfo {
        TopicInfo {
            name: "orders".to_owned(),
            topic_id: TopicId::ZERO,
            internal: false,
            partitions,
            error: None,
        }
    }

    /// The rule that used to exist twice, in two languages.
    ///
    /// The overview card derived these in TypeScript from the partition list
    /// while `TopicSummary::of` derived them in Rust for the topic list, and
    /// nothing kept the two in step. Now `TopicDetail` carries them from the
    /// same three functions the summary uses — this pins that they agree.
    #[test]
    fn the_detail_and_the_summary_answer_replication_identically() {
        // Mid-reassignment: partition 0 at three replicas, partition 1 at two
        // and short one in-sync. The factor is the minimum, not the first or
        // the largest.
        let info = topic_info(vec![
            partition_info(0, vec![1, 2, 3], vec![1, 2, 3]),
            partition_info(1, vec![1, 2], vec![1]),
        ]);
        let summary = TopicSummary::of(&info);
        let detail = TopicDetail::of(&info);
        assert_eq!(summary.replication_factor, 2);
        assert_eq!(detail.replication_factor, summary.replication_factor);
        assert_eq!(detail.under_replicated_partition_count, 1);
        assert_eq!(
            detail.under_replicated_partition_count,
            summary.under_replicated_partition_count
        );
        assert_eq!(
            detail.offline_partition_count,
            summary.offline_partition_count
        );
    }

    #[test]
    fn a_message_count_missing_a_partition_is_absent_not_smaller() {
        let info = topic_info(vec![
            partition_info(0, vec![1], vec![1]),
            partition_info(1, vec![1], vec![1]),
        ]);
        let mut detail = TopicDetail::of(&info);

        // Nothing attached yet: no claim.
        detail.set_message_count_from_offsets();
        assert_eq!(detail.message_count, None);

        // One partition answered, one did not — the sum would be short by an
        // unknowable amount, and a smaller number carries no mark saying so.
        detail.partitions[0].set_offsets(Some(100), Some(400));
        detail.set_message_count_from_offsets();
        assert_eq!(detail.message_count, None);

        // Both answered: the retained count, `latest - earliest` summed.
        detail.partitions[1].set_offsets(Some(0), Some(50));
        detail.set_message_count_from_offsets();
        assert_eq!(detail.message_count, Some(350));
    }

    /// A fleet with one empty environment, one holding clusters and
    /// inventory, and one holding inventory alone.
    fn fleet() -> (Registry, Access) {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    name: Development
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        schema_registry: apicurio
    resources:
      - id: apicurio-dev
        kind: schema_registry
  - id: staging
  - id: prod
    name: Production
    resources:
      - id: mosquitto
        kind: mqtt_broker
"#,
        )
        .unwrap();
        (Registry::from_config(&config).unwrap(), Access::admin())
    }

    fn arrange(registry: &Registry, who: &Access) -> Vec<EnvironmentSection> {
        let cards = registry
            .visible(who)
            .map(|handle| ClusterCard::of(handle, who))
            .collect();
        EnvironmentSection::arrange(cards, registry, who)
    }

    #[test]
    fn sections_run_in_declared_order() {
        let (registry, who) = fleet();
        let sections = arrange(&registry, &who);
        let ids: Vec<&str> = sections.iter().map(|section| section.id.as_str()).collect();
        // `staging` holds nothing at all, so it is absent. There is no
        // discovered section and no nameless one any more: a cluster cannot
        // arrive in an environment nobody wrote down, because there is nowhere
        // to declare one outside an environment.
        assert_eq!(ids, ["dev", "prod"]);
    }

    #[test]
    fn an_environment_holding_nothing_visible_is_absent_entirely() {
        // Not cosmetic. A heading over an empty grid tells a caller who cannot
        // see prod that prod exists, which is the 404-not-403 rule undone one
        // level up — and now also the rule that makes every URL beneath the
        // environment unprobeable.
        let (registry, who) = fleet();
        let sections = arrange(&registry, &who);
        assert!(sections.iter().all(|section| section.id != "staging"));
    }

    #[test]
    fn a_section_carries_its_clusters_its_registries_and_its_other_resources() {
        let (registry, who) = fleet();
        let sections = arrange(&registry, &who);

        let dev = sections.iter().find(|s| s.id == "dev").unwrap();
        assert_eq!(dev.name, "Development");
        assert_eq!(dev.clusters.len(), 1);
        assert_eq!(dev.resources.len(), 1);
        // The registry is a peer of the cluster now, not something reached
        // through it, and it names the clusters that decode against it.
        assert_eq!(dev.schema_registries.len(), 1);
        assert_eq!(dev.schema_registries[0].registry.id, "apicurio");
        assert_eq!(dev.schema_registries[0].used_by, vec!["kaas".to_owned()]);

        // An environment can hold no cluster at all and still be a section:
        // prod here is a broker kaas-ui does not speak.
        let prod = sections.iter().find(|s| s.id == "prod").unwrap();
        assert!(prod.clusters.is_empty());
        assert!(prod.schema_registries.is_empty());
        assert_eq!(prod.resources[0].kind, ResourceKind::MqttBroker);
    }

    #[test]
    fn a_cluster_card_carries_the_environment_that_addresses_it() {
        let (registry, who) = fleet();
        let sections = arrange(&registry, &who);
        let card = &sections.iter().find(|s| s.id == "dev").unwrap().clusters[0];
        // Half of its identity, and the first segment of every URL that
        // reaches it. A client cannot build one without this.
        assert_eq!(card.environment, "dev");
        assert_eq!(card.id, "kaas");
    }

    #[test]
    fn a_registry_nobody_references_is_still_listed() {
        // The bug this pins. "No *visible* cluster uses it" and "no cluster
        // uses it at all" were one branch, so declaring a second registry made
        // it silently vanish — which is the opposite of what a reader needs
        // from a registry nothing decodes against. It names no cluster, so
        // there is nothing to leak by showing it.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
      - id: apicurio2
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        schema_registry: apicurio
"#,
        )
        .unwrap();
        let registry = Registry::from_config(&config).unwrap();
        let who = Access::admin();

        let dev = &arrange(&registry, &who)[0];
        let listed: Vec<(&str, usize)> = dev
            .schema_registries
            .iter()
            .map(|entry| (entry.registry.id.as_str(), entry.used_by.len()))
            .collect();
        assert_eq!(listed, vec![("apicurio", 1), ("apicurio2", 0)]);
        // And it is reachable, not merely rendered.
        assert!(registry.schema_registry("dev", "apicurio2", &who).is_some());
    }

    #[test]
    fn a_registry_only_hidden_clusters_use_stays_hidden() {
        // The other half, and the reason the branch existed at all: this one
        // *is* referenced, and every cluster referencing it is invisible to
        // this caller. Naming it would say a cluster is there.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: open
        url: http://open:8080/apis/ccompat/v7
      - id: secret
        url: http://secret:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        labels: { tier: public }
        schema_registry: open
      - id: hidden
        bootstrap: ["b:9092"]
        labels: { tier: private }
        schema_registry: secret

roles:
  - name: public-only
    subjects: ["everyone"]
    cluster_labels: { tier: public }
    permissions:
      - resource: topic
        actions: [view]
"#,
        )
        .unwrap();
        let registry = Registry::from_config(&config).unwrap();
        let policy = kaas_ui_auth::Policy::enforcing(config.roles.clone());
        let who = policy.access(&Principal::new("u").with_groups(["everyone".to_owned()]));

        let dev = &arrange(&registry, &who)[0];
        let listed: Vec<&str> = dev
            .schema_registries
            .iter()
            .map(|entry| entry.registry.id.as_str())
            .collect();
        assert_eq!(listed, vec!["open"]);
        assert!(registry.schema_registry("dev", "secret", &who).is_none());
    }

    #[test]
    fn seeing_a_cluster_is_not_permission_to_read_its_subjects() {
        // `Resource::Topic` + `view` guards a topic name, and a subject name
        // is metadata of the same kind. The lookup carries the whole decision
        // for the subject list, so it has to carry this half too — visibility
        // alone would have been a weaker gate than the one it replaced.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://apicurio:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        schema_registry: apicurio

roles:
  - name: configs-only
    subjects: ["ops"]
    cluster_labels: { env: dev }
    permissions:
      - resource: cluster_config
        actions: [view]
"#,
        )
        .unwrap();
        let registry = Registry::from_config(&config).unwrap();
        let policy = kaas_ui_auth::Policy::enforcing(config.roles.clone());
        let who = policy.access(&Principal::new("u").with_groups(["ops".to_owned()]));

        // The cluster is visible to them; the registry is not.
        assert!(registry.get("dev", "kaas", &who).is_some());
        assert!(registry.schema_registry("dev", "apicurio", &who).is_none());
    }

    #[test]
    fn a_caller_who_cannot_see_prod_is_not_told_that_prod_exists() {
        // The leak this filtering exists to prevent: the clusters are hidden by
        // the label selector, and a schema registry sitting in the same
        // environment would announce it right back if resources and registries
        // were not filtered by the same test.
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    schema_registries:
      - id: apicurio
        url: http://dev:8080/apis/ccompat/v7
    kafka_clusters:
      - id: kaas
        bootstrap: ["a:9092"]
        schema_registry: apicurio
    resources:
      - id: apicurio-dev
        kind: schema_registry
  - id: prod
    schema_registries:
      - id: apicurio
        url: http://prod:8080/apis/ccompat/v7
    kafka_clusters:
      - id: prod-eu
        bootstrap: ["b:9092"]
        schema_registry: apicurio
    resources:
      - id: apicurio-prod
        kind: schema_registry

roles:
  - name: dev-only
    subjects: ["dev-team"]
    cluster_labels: { env: dev }
    permissions:
      - resource: topic
        actions: [view]
"#,
        )
        .unwrap();

        let registry = Registry::from_config(&config).unwrap();
        let policy = kaas_ui_auth::Policy::enforcing(config.roles.clone());
        let who = policy.access(&Principal::new("u").with_groups(["dev-team".to_owned()]));

        let sections = arrange(&registry, &who);
        let ids: Vec<&str> = sections.iter().map(|section| section.id.as_str()).collect();
        assert_eq!(ids, ["dev"]);
        assert_eq!(sections[0].resources.len(), 1);
        assert_eq!(sections[0].resources[0].id, "apicurio-dev");
        // Both environments declare an `apicurio`. The id is scoped, so seeing
        // dev's says nothing about prod's — and the lookup refuses prod's to
        // this caller even though the id is one they know.
        assert_eq!(sections[0].schema_registries.len(), 1);
        assert!(registry.schema_registry("prod", "apicurio", &who).is_none());
        assert!(registry.schema_registry("dev", "apicurio", &who).is_some());
    }

    #[test]
    fn a_resource_card_has_no_status_to_render_green() {
        // kaas-ui dials none of these. The absence of the field is the
        // guarantee — see `ResourceCard`.
        let (registry, who) = fleet();
        let sections = arrange(&registry, &who);
        let card = &sections.iter().find(|s| s.id == "dev").unwrap().resources[0];
        let json = serde_json::to_value(card).unwrap();
        assert!(json.get("status").is_none(), "{json}");
        assert_eq!(json["kind"], "schema_registry");
        assert_eq!(json["labels"]["env"], "dev");
    }

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

    // Payload rendering itself is tested where it lives, in `kaas-ui-serde`:
    // the codecs, the truncation and the Confluent framing are that crate's,
    // and testing them again through a re-export would only assert that the
    // re-export exists.

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

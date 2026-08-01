//! The capability projection.
//!
//! **This file contains no version logic.** It maps kaas-lib's
//! already-negotiated table onto UI features and does nothing else: one table,
//! one `match`, no arithmetic on version numbers, and no knowledge anywhere of
//! which Kafka release added what. If it ever needed such knowledge, the
//! knowledge would belong in kaas-lib — see `docs/reference/upstream-asks.md`.
//!
//! ## The source field is not decoration
//!
//! kaas-lib's version table is **per connection**, deliberately: brokers
//! mid-rolling-upgrade genuinely disagree, and a cluster-wide table would be
//! wrong during exactly the window when being right matters. There is
//! therefore no `cluster.capabilities()` to project from, and fabricating one
//! by picking whichever broker answered produces a UI whose tabs flicker.
//!
//! So the table is read from an **explicitly named** broker and the answer
//! says which one. A user who sees a surprising tab set can at least tell
//! where it came from.

use kafka_conn::{ApiKey, ApiVersions};
use serde::Serialize;
use utoipa::ToSchema;

/// A thing the UI can show, named in kaas-ui's vocabulary rather than Kafka's.
///
/// These names are a UI contract: adding an api key to an existing feature
/// must not rename it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    /// The cluster detail panel's authoritative broker list.
    ClusterDescription,
    /// Topic detail via the newer per-partition api.
    TopicPartitions,
    /// Per-broker log directories and topic sizes.
    LogDirs,
    /// Broker and topic configuration.
    Configs,
    /// Consumer groups, the classic path.
    ConsumerGroups,
    /// Consumer groups, the KIP-848 path.
    ModernConsumerGroups,
    /// Share groups.
    ShareGroups,
    /// Committed offsets, and therefore lag.
    CommittedOffsets,
    /// The ACL viewer.
    Acls,
    /// Client quotas.
    Quotas,
    /// SCRAM credentials.
    ScramUsers,
    /// Partition reassignments in flight.
    Reassignments,
    /// Transactions.
    Transactions,
    /// Producer state per partition.
    Producers,
    /// The KRaft quorum panel.
    Quorum,
    /// The message browser.
    Messages,
}

impl Feature {
    /// The api keys a feature needs. All of them, not any of them.
    const fn keys(self) -> &'static [ApiKey] {
        match self {
            Self::ClusterDescription => &[ApiKey::DescribeCluster],
            Self::TopicPartitions => &[ApiKey::DescribeTopicPartitions],
            Self::LogDirs => &[ApiKey::DescribeLogDirs],
            Self::Configs => &[ApiKey::DescribeConfigs],
            Self::ConsumerGroups => &[ApiKey::ListGroups, ApiKey::DescribeGroups],
            Self::ModernConsumerGroups => &[ApiKey::ConsumerGroupDescribe],
            Self::ShareGroups => &[ApiKey::ShareGroupDescribe],
            Self::CommittedOffsets => &[ApiKey::OffsetFetch],
            Self::Acls => &[ApiKey::DescribeAcls],
            Self::Quotas => &[ApiKey::DescribeClientQuotas],
            Self::ScramUsers => &[ApiKey::DescribeUserScramCredentials],
            Self::Reassignments => &[ApiKey::ListPartitionReassignments],
            Self::Transactions => &[ApiKey::ListTransactions, ApiKey::DescribeTransactions],
            Self::Producers => &[ApiKey::DescribeProducers],
            Self::Quorum => &[ApiKey::DescribeQuorum],
            Self::Messages => &[ApiKey::Fetch, ApiKey::ListOffsets],
        }
    }

    /// Every feature, in a stable order.
    pub const ALL: &'static [Feature] = &[
        Feature::ClusterDescription,
        Feature::TopicPartitions,
        Feature::LogDirs,
        Feature::Configs,
        Feature::ConsumerGroups,
        Feature::ModernConsumerGroups,
        Feature::ShareGroups,
        Feature::CommittedOffsets,
        Feature::Acls,
        Feature::Quotas,
        Feature::ScramUsers,
        Feature::Reassignments,
        Feature::Transactions,
        Feature::Producers,
        Feature::Quorum,
        Feature::Messages,
    ];
}

/// Whether a feature can be shown, and if not, why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum FeatureState {
    /// Every api key the feature needs negotiated a version.
    Available,
    /// At least one did not. Both ranges travel with the answer, because the
    /// pair is the diagnosis: `broker: null` means the cluster does not
    /// implement it, `ours: null` means this build has no schema for it, and
    /// two disjoint ranges mean the cluster is behind.
    #[serde(rename_all = "camelCase")]
    Unsupported {
        /// The api key that decided it.
        api: String,
        /// Its number.
        api_key: i16,
        /// What the broker advertises.
        broker: Option<[i16; 2]>,
        /// What this build speaks.
        ours: Option<[i16; 2]>,
    },
}

impl FeatureState {
    /// Whether the UI may offer this.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Where the version table came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CapabilitySource {
    /// One named broker answered. Interim: see the module comment.
    #[serde(rename_all = "camelCase")]
    Broker {
        /// Which broker, rendered by the UI as "as reported by broker 1".
        node_id: Option<i32>,
        /// Its address, for when the node id is not known.
        peer: String,
    },
}

/// One api key as the broker and this build see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyEntry {
    /// The key's name, or `Unknown(n)` where this build has none.
    pub name: String,
    /// The key's number.
    pub key: i16,
    /// The broker's advertised range.
    pub broker: Option<[i16; 2]>,
    /// This build's range.
    pub ours: Option<[i16; 2]>,
    /// The version that would be used.
    pub negotiated: Option<i16>,
    /// Whether the broker offers a newer version than this build can encode.
    pub broker_ahead: bool,
}

/// The capability answer.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Feature name to state.
    pub features: Vec<FeatureEntry>,
    /// Which broker answered.
    pub source: CapabilitySource,
    /// The whole table, for the api-versions panel.
    pub api_keys: Vec<ApiKeyEntry>,
    /// How many keys the broker advertises that this build cannot encode.
    ///
    /// Not a failure — a fact about a broker newer than the codec. Two of
    /// these on the Strimzi cluster today cannot even be named.
    pub broker_ahead_count: usize,
}

/// One row of the feature table.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeatureEntry {
    /// The feature.
    pub feature: Feature,
    /// Its state.
    #[serde(flatten)]
    pub state: FeatureState,
}

/// Project a connection's negotiated table onto the feature list.
pub fn project(versions: &ApiVersions, source: CapabilitySource) -> Capabilities {
    let features = Feature::ALL
        .iter()
        .map(|feature| FeatureEntry {
            feature: *feature,
            state: state_of(*feature, versions),
        })
        .collect();

    let mut api_keys: Vec<ApiKeyEntry> = versions
        .entries()
        .map(|entry| ApiKeyEntry {
            name: entry.api_key.name().to_owned(),
            key: entry.api_key.code(),
            broker: Some([entry.broker.min, entry.broker.max]),
            ours: entry.ours.map(|r| [r.min, r.max]),
            negotiated: entry.negotiated(),
            broker_ahead: entry.broker_ahead(),
        })
        .collect();
    api_keys.sort_by_key(|entry| entry.key);

    let broker_ahead_count = api_keys.iter().filter(|entry| entry.broker_ahead).count();

    Capabilities {
        features,
        source,
        api_keys,
        broker_ahead_count,
    }
}

/// A feature is available when every key it needs negotiated a version.
fn state_of(feature: Feature, versions: &ApiVersions) -> FeatureState {
    for key in feature.keys() {
        match versions.get(*key) {
            Some(entry) => match entry.negotiated() {
                Some(_) => continue,
                // Advertised, but no overlap with what we speak.
                None => {
                    return FeatureState::Unsupported {
                        api: key.name().to_owned(),
                        api_key: key.code(),
                        broker: Some([entry.broker.min, entry.broker.max]),
                        ours: entry.ours.map(|r| [r.min, r.max]),
                    };
                }
            },
            // Not advertised at all: the cluster does not implement it.
            None => {
                return FeatureState::Unsupported {
                    api: key.name().to_owned(),
                    api_key: key.code(),
                    broker: None,
                    ours: kafka_conn::our_range(*key).map(|r| [r.min, r.max]),
                };
            }
        }
    }
    FeatureState::Available
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> CapabilitySource {
        CapabilitySource::Broker {
            node_id: Some(1),
            peer: "broker-1:9092".to_owned(),
        }
    }

    /// The shape of the `kaas` cluster: it advertises a strict subset, and
    /// `DescribeCluster` is one of the keys it does not have.
    #[test]
    fn a_key_the_cluster_does_not_advertise_is_unsupported_with_a_null_broker_range() {
        let versions = ApiVersions::from_triples([(3, 0, 12), (18, 0, 3)]);
        let capabilities = project(&versions, source());

        let cluster_description = capabilities
            .features
            .iter()
            .find(|entry| entry.feature == Feature::ClusterDescription)
            .unwrap();

        match &cluster_description.state {
            FeatureState::Unsupported {
                api, broker, ours, ..
            } => {
                assert_eq!(api, "DescribeCluster");
                assert_eq!(*broker, None, "the cluster does not implement it");
                assert!(ours.is_some(), "but this build speaks it");
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }

    #[test]
    fn a_feature_needing_two_keys_needs_both() {
        // ListTransactions without DescribeTransactions is not a transactions
        // tab: the list would render and every row would fail to open.
        let versions = ApiVersions::from_triples([(66, 0, 1)]);
        let capabilities = project(&versions, source());
        let transactions = capabilities
            .features
            .iter()
            .find(|entry| entry.feature == Feature::Transactions)
            .unwrap();
        assert!(!transactions.state.is_available());
    }

    #[test]
    fn an_advertised_key_is_available() {
        let versions = ApiVersions::from_triples([(29, 0, 3)]);
        let capabilities = project(&versions, source());
        let acls = capabilities
            .features
            .iter()
            .find(|entry| entry.feature == Feature::Acls)
            .unwrap();
        assert!(acls.state.is_available());
    }

    #[test]
    fn a_broker_ahead_of_the_codec_is_counted_not_hidden() {
        // A broker advertising a version beyond anything this build can encode
        // is a fact worth surfacing, not an error.
        let versions = ApiVersions::from_triples([(2, 0, 99), (3, 0, 12)]);
        let capabilities = project(&versions, source());
        assert!(capabilities.broker_ahead_count >= 1);
    }

    #[test]
    fn the_feature_vocabulary_is_stable_and_camel_case() {
        let json = serde_json::to_value(Feature::ConsumerGroups).unwrap();
        assert_eq!(json, "consumerGroups");
        let json = serde_json::to_value(Feature::ScramUsers).unwrap();
        assert_eq!(json, "scramUsers");
    }
}

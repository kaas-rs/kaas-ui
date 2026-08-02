//! What a caller is allowed to see.
//!
//! A [`Policy`] is a list of [`Role`]s from the config file. Resolving one
//! against a [`Principal`] gives an [`Access`], and every question the rest of
//! the workspace asks — is this cluster visible, may this caller read a
//! payload — is asked of that.
//!
//! # Two grants, and the second is the one that matters
//!
//! Browsing a topic's configuration is not the same act as reading customer
//! data out of a payload. Payloads carry PII, tokens and order data, so
//! [`Grant::Metadata`] and [`Grant::Messages`] are separate and a role may
//! hold the first without the second.
//!
//! # Absence is 404, not 403
//!
//! [`Access::sees`] is consulted inside the registry lookup, so a cluster a
//! caller has no role for does not exist as far as that caller is concerned.
//! A 403 would confirm the id, and confirming ids is how a registry becomes
//! enumerable.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::identity::Principal;

/// What a role permits. Two, because reading is the only verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    /// Cluster, broker, topic, config and group *description*. No payloads.
    Metadata,
    /// Record keys, values and headers — the sensitive surface.
    Messages,
}

/// A set of grants, which is how they are carried and projected.
pub type Grants = BTreeSet<Grant>;

/// One entry in the `roles:` list.
///
/// ```yaml
/// roles:
///   - name: prod-support
///     subjects: ["kaas-rs:support"]   # a subject, or a group the provider asserted
///     clusters: { env: prod }         # label selector; every pair must match
///     grants: [metadata, messages]
///     topics: ["public-*"]            # payload access scoped by pattern
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default, rename_all = "snake_case")]
pub struct Role {
    /// What this role is called. Appears in `/api/me` and in the audit log.
    pub name: String,
    /// Subjects and groups this role applies to.
    ///
    /// A bare `"*"` matches any *authenticated* caller — never the anonymous
    /// one, because a rule that accidentally grants the world is the one
    /// mistake this file must not make easy.
    pub subjects: Vec<String>,
    /// A label selector over clusters. Empty matches every cluster.
    pub clusters: BTreeMap<String, String>,
    /// What the role permits on those clusters.
    pub grants: Grants,
    /// Topic patterns the [`Grant::Messages`] grant is limited to.
    ///
    /// Empty means every topic. `*` is the only wildcard, and it may appear
    /// anywhere: `public-*`, `*-events`, `*`.
    pub topics: Vec<String>,
}

impl Role {
    /// Whether this role applies to a caller.
    fn covers(&self, who: &Principal) -> bool {
        self.subjects.iter().any(|subject| {
            if subject == "*" {
                return who.is_authenticated();
            }
            who.identifiers().any(|id| id == subject)
        })
    }

    /// Whether this role's selector matches a cluster's labels.
    fn selects(&self, labels: &BTreeMap<String, String>) -> bool {
        self.clusters
            .iter()
            .all(|(key, value)| labels.get(key).is_some_and(|found| found == value))
    }

    /// Whether a topic is inside this role's patterns.
    fn covers_topic(&self, topic: &str) -> bool {
        self.topics.is_empty() || self.topics.iter().any(|pattern| matches(pattern, topic))
    }
}

/// The `roles:` list, plus whether it is being enforced at all.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    roles: Vec<Arc<Role>>,
    enforcing: bool,
}

impl Policy {
    /// No authentication configured: everyone sees everything.
    ///
    /// The honest name for what a deployment without an `auth` block does. It
    /// is a mode, not a fallback — a policy that failed to load must never
    /// arrive here, and nothing in this crate produces it by accident.
    #[must_use]
    pub fn open() -> Self {
        Self {
            roles: Vec::new(),
            enforcing: false,
        }
    }

    /// Enforce these roles.
    #[must_use]
    pub fn enforcing(roles: Vec<Role>) -> Self {
        Self {
            roles: roles.into_iter().map(Arc::new).collect(),
            enforcing: true,
        }
    }

    /// Whether roles are being applied.
    #[must_use]
    pub fn is_enforcing(&self) -> bool {
        self.enforcing
    }

    /// How many roles are configured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.roles.len()
    }

    /// Whether the role list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty()
    }

    /// Resolve this policy for one caller.
    ///
    /// Every role that covers them, kept whole: the cluster and topic
    /// questions are answered per cluster later, and flattening them here
    /// would lose which grant came with which selector.
    #[must_use]
    pub fn access(&self, who: &Principal) -> Access {
        if !self.enforcing {
            return Access::unrestricted();
        }
        Access {
            roles: self
                .roles
                .iter()
                .filter(|role| role.covers(who))
                .map(Arc::clone)
                .collect(),
            unrestricted: false,
        }
    }
}

/// One caller's resolved view of the fleet.
#[derive(Debug, Clone, Default)]
pub struct Access {
    roles: Vec<Arc<Role>>,
    unrestricted: bool,
}

impl Access {
    /// Everything, for a deployment with no authentication configured.
    #[must_use]
    pub fn unrestricted() -> Self {
        Self {
            roles: Vec::new(),
            unrestricted: true,
        }
    }

    /// Nothing at all — no cluster exists for this caller.
    ///
    /// What an authenticated user in no matching role gets. They see an empty
    /// fleet, which is a true answer rather than an error.
    #[must_use]
    pub fn none() -> Self {
        Self {
            roles: Vec::new(),
            unrestricted: false,
        }
    }

    /// Whether this caller is subject to any role at all.
    #[must_use]
    pub fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    /// Whether a cluster with these labels exists for this caller.
    ///
    /// Visibility is any grant at all: a role with `grants: []` selects a
    /// cluster it can say nothing about, which is a config mistake rather than
    /// a way to hide one, and hiding it here would make that mistake silent.
    #[must_use]
    pub fn sees(&self, labels: &BTreeMap<String, String>) -> bool {
        self.unrestricted || self.roles.iter().any(|role| role.selects(labels))
    }

    /// Everything this caller may do on a cluster with these labels.
    #[must_use]
    pub fn grants(&self, labels: &BTreeMap<String, String>) -> Grants {
        if self.unrestricted {
            return [Grant::Metadata, Grant::Messages].into_iter().collect();
        }
        self.roles
            .iter()
            .filter(|role| role.selects(labels))
            .flat_map(|role| role.grants.iter().copied())
            .collect()
    }

    /// Whether one grant is held on a cluster with these labels.
    #[must_use]
    pub fn may(&self, labels: &BTreeMap<String, String>, grant: Grant) -> bool {
        self.unrestricted
            || self
                .roles
                .iter()
                .any(|role| role.selects(labels) && role.grants.contains(&grant))
    }

    /// Whether this caller may read payloads from one named topic.
    ///
    /// The [`Grant::Messages`] grant *and* the topic patterns, in one place,
    /// because they are one question. A role granting `messages` on
    /// `public-*` says nothing about `payments`, and asking the two halves
    /// separately is how that turns into a leak.
    #[must_use]
    pub fn may_read_topic(&self, labels: &BTreeMap<String, String>, topic: &str) -> bool {
        self.unrestricted
            || self.roles.iter().any(|role| {
                role.selects(labels)
                    && role.grants.contains(&Grant::Messages)
                    && role.covers_topic(topic)
            })
    }

    /// The names of the roles that covered this caller, for `/api/me` and the
    /// audit log.
    pub fn role_names(&self) -> impl Iterator<Item = &str> {
        self.roles.iter().map(|role| role.name.as_str())
    }
}

/// Glob matching, with `*` as the only wildcard.
///
/// Hand-written rather than pulled in: the pattern language is one character
/// wide, and a dependency here would be a larger surface than the function.
/// Text either side of a `*` is anchored — `public-*` does not match
/// `not-public-orders` — because a topic pattern that matches in the middle of
/// a name is a pattern nobody can review.
fn matches(pattern: &str, value: &str) -> bool {
    let mut segments = pattern.split('*');

    let Some(first) = segments.next() else {
        return pattern == value;
    };
    let Some(mut rest) = value.strip_prefix(first) else {
        return false;
    };

    let tail: Vec<&str> = segments.collect();
    let Some((last, middle)) = tail.split_last() else {
        // No `*` in the pattern at all: it was an exact match, and the prefix
        // strip above only proves it is a prefix.
        return rest.is_empty();
    };

    for segment in middle {
        if segment.is_empty() {
            continue;
        }
        let Some(found) = rest.find(segment) else {
            return false;
        };
        let Some(remainder) = rest.get(found + segment.len()..) else {
            return false;
        };
        rest = remainder;
    }

    // Whatever follows the final `*` is anchored to the end.
    last.is_empty() || rest.ends_with(last)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn role(name: &str, subjects: &[&str], selector: &[(&str, &str)], grants: &[Grant]) -> Role {
        Role {
            name: name.to_owned(),
            subjects: subjects.iter().map(|s| (*s).to_owned()).collect(),
            clusters: labels(selector),
            grants: grants.iter().copied().collect(),
            topics: Vec::new(),
        }
    }

    fn member(groups: &[&str]) -> Principal {
        Principal::new("sub-1", None, groups.iter().map(|g| (*g).to_owned()))
    }

    #[test]
    fn an_open_policy_grants_everything_to_anyone() {
        let access = Policy::open().access(&Principal::anonymous());
        assert!(access.is_unrestricted());
        assert!(access.sees(&labels(&[("env", "prod")])));
        assert!(access.may(&labels(&[("env", "prod")]), Grant::Messages));
        assert!(access.may_read_topic(&labels(&[]), "payments"));
    }

    #[test]
    fn an_enforcing_policy_hides_clusters_no_role_selects() {
        let policy = Policy::enforcing(vec![role(
            "dev",
            &["kaas-rs"],
            &[("env", "dev")],
            &[Grant::Metadata, Grant::Messages],
        )]);
        let access = policy.access(&member(&["kaas-rs"]));

        assert!(access.sees(&labels(&[("env", "dev"), ("kind", "kaas")])));
        assert!(!access.sees(&labels(&[("env", "prod")])));
        // Absent labels are not a wildcard: a cluster with no `env` at all is
        // not selected by `env: dev`.
        assert!(!access.sees(&labels(&[("kind", "kaas")])));
    }

    #[test]
    fn a_caller_in_no_role_sees_an_empty_fleet_rather_than_an_error() {
        let policy = Policy::enforcing(vec![role("dev", &["kaas-rs"], &[], &[Grant::Metadata])]);
        let access = policy.access(&member(&["someone-else"]));

        assert!(!access.is_unrestricted());
        assert!(!access.sees(&labels(&[("env", "dev")])));
        assert_eq!(access.role_names().count(), 0);
    }

    #[test]
    fn an_empty_selector_matches_every_cluster() {
        let policy = Policy::enforcing(vec![role("all", &["*"], &[], &[Grant::Metadata])]);
        let access = policy.access(&member(&[]));
        assert!(access.sees(&labels(&[("env", "prod")])));
        assert!(access.sees(&labels(&[])));
    }

    #[test]
    fn a_star_subject_never_covers_the_anonymous_caller() {
        let policy = Policy::enforcing(vec![role("all", &["*"], &[], &[Grant::Metadata])]);
        assert!(!policy.access(&Principal::anonymous()).sees(&labels(&[])));
        assert!(policy.access(&member(&[])).sees(&labels(&[])));
    }

    #[test]
    fn metadata_without_messages_is_the_whole_point() {
        let policy = Policy::enforcing(vec![role(
            "oncall",
            &["kaas-rs:platform"],
            &[("env", "prod")],
            &[Grant::Metadata],
        )]);
        let access = policy.access(&member(&["kaas-rs:platform"]));
        let prod = labels(&[("env", "prod")]);

        assert!(access.sees(&prod));
        assert!(access.may(&prod, Grant::Metadata));
        assert!(!access.may(&prod, Grant::Messages));
        assert!(!access.may_read_topic(&prod, "anything"));
        assert_eq!(
            access.grants(&prod),
            [Grant::Metadata].into_iter().collect()
        );
    }

    #[test]
    fn grants_from_several_matching_roles_are_unioned() {
        let policy = Policy::enforcing(vec![
            role(
                "reader",
                &["kaas-rs"],
                &[("env", "dev")],
                &[Grant::Metadata],
            ),
            role("payloads", &["kaas-rs:platform"], &[], &[Grant::Messages]),
        ]);
        let access = policy.access(&member(&["kaas-rs", "kaas-rs:platform"]));
        let dev = labels(&[("env", "dev")]);

        assert_eq!(
            access.grants(&dev),
            [Grant::Metadata, Grant::Messages].into_iter().collect()
        );
        assert_eq!(access.role_names().count(), 2);
    }

    #[test]
    fn topic_patterns_scope_the_messages_grant() {
        let mut scoped = role(
            "support",
            &["kaas-rs:support"],
            &[("env", "prod")],
            &[Grant::Metadata, Grant::Messages],
        );
        scoped.topics = vec!["public-*".to_owned()];
        let policy = Policy::enforcing(vec![scoped]);
        let access = policy.access(&member(&["kaas-rs:support"]));
        let prod = labels(&[("env", "prod")]);

        assert!(access.may_read_topic(&prod, "public-orders"));
        assert!(!access.may_read_topic(&prod, "payments"));
        // The grant is still held — it is the topic that is out of scope, and
        // the projection to the frontend says so at cluster level.
        assert!(access.may(&prod, Grant::Messages));
    }

    #[test]
    fn a_pattern_is_anchored_at_both_ends() {
        assert!(matches("public-*", "public-orders"));
        assert!(!matches("public-*", "not-public-orders"));
        assert!(matches("*-events", "app-events"));
        assert!(!matches("*-events", "app-events-dlq"));
        assert!(matches("*", "anything at all"));
        assert!(matches("exact", "exact"));
        assert!(!matches("exact", "exactly"));
        assert!(!matches("exact", "ex"));
    }

    #[test]
    fn a_pattern_may_have_several_wildcards() {
        assert!(matches("a*b*c", "a-b-c"));
        assert!(matches("a*b*c", "abc"));
        assert!(!matches("a*b*c", "a-c"));
        assert!(!matches("a*b*c", "a-b-c-d"));
    }

    #[test]
    fn a_pattern_never_matches_across_a_missing_prefix() {
        // The regression this function exists to avoid: `find` alone would let
        // a middle segment match anywhere, so a pattern could quietly cover a
        // topic nobody meant to include.
        assert!(!matches("orders-*", "shadow-orders-eu"));
    }

    #[test]
    fn a_role_deserializes_from_the_documented_shape() {
        // The field names and the grant spellings are a config-file contract.
        // YAML is the config crate's business; the shape is this crate's, and
        // JSON exercises the same derive.
        let parsed: Role = serde_json::from_str(
            r#"{
                "name": "prod-support",
                "subjects": ["kaas-rs:support"],
                "clusters": {"env": "prod"},
                "grants": ["metadata", "messages"],
                "topics": ["public-*"]
            }"#,
        )
        .unwrap();

        assert_eq!(parsed.name, "prod-support");
        assert_eq!(parsed.grants, [Grant::Metadata, Grant::Messages].into());
        assert_eq!(parsed.topics, ["public-*"]);
        assert_eq!(parsed.clusters.get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn a_role_needs_nothing_but_a_name() {
        let parsed: Role = serde_json::from_str(r#"{"name": "bare"}"#).unwrap();
        assert!(parsed.subjects.is_empty());
        assert!(parsed.grants.is_empty());
        // A role nobody is in and which grants nothing is useless, not
        // dangerous — it selects every cluster and permits nothing on them.
        assert!(
            !Policy::enforcing(vec![parsed])
                .access(&member(&["kaas-rs"]))
                .sees(&labels(&[]))
        );
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_ignored() {
        // `grant:` for `grants:` must not silently produce a role that
        // permits nothing.
        let parsed = serde_json::from_str::<Role>(r#"{"name": "typo", "grant": ["messages"]}"#);
        assert!(parsed.is_err());
    }
}

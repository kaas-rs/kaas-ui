//! What a caller is allowed to see.
//!
//! A [`Policy`] is a list of [`Role`]s from the config file. Resolving one
//! against a [`Principal`] gives an [`Access`], and every question the rest of
//! the workspace asks is asked of that.
//!
//! # The shape is kafbat-ui's; the verbs cannot be
//!
//! Roles carry subjects, clusters and permissions of `resource` + `value` +
//! `actions`, which is the model anyone arriving from kafbat-ui already knows.
//! What does not carry over is most of its vocabulary: `create`, `edit`,
//! `delete`, `messages_produce`, `reset_offsets` and the rest describe writes,
//! and kaas-ui has no code path that could perform one. Offering them here
//! would be a config surface that grants nothing — worse than absent, because
//! it would read as protection.
//!
//! So there are two actions. [`Action::View`] is the metadata surface, and
//! [`Action::MessagesRead`] is the payloads — the boundary that matters,
//! because browsing a topic's configuration is not the same act as reading
//! customer data out of it.
//!
//! # Absence is 404, not 403
//!
//! [`Access::sees`] is consulted inside the registry lookup, so a cluster no
//! role covers does not exist as far as that caller is concerned. A 403 would
//! confirm the id, and confirming ids is how a registry becomes enumerable.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::identity::Principal;

/// What a permission is about.
///
/// Only what exists. A schema registry and an ACL viewer are Phases 6 and 7,
/// and their resources arrive with them rather than ahead of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    /// The cluster itself: brokers, configs, capabilities, log dirs.
    ClusterConfig,
    /// Topics — their list, their description, their configs and offsets, and
    /// with [`Action::MessagesRead`], the records inside them.
    Topic,
    /// Consumer groups, their members and their committed offsets.
    Consumer,
}

impl Resource {
    /// Every resource, for a role that says `all`.
    #[must_use]
    pub fn every() -> [Self; 3] {
        [Self::ClusterConfig, Self::Topic, Self::Consumer]
    }

    /// Whether a `value` pattern means anything here.
    ///
    /// Topics and groups are named, so a role can be scoped to `public-*`. A
    /// cluster's own configuration is not a set of things with names, and a
    /// pattern against it would silently match nothing.
    #[must_use]
    pub fn is_named(self) -> bool {
        matches!(self, Self::Topic | Self::Consumer)
    }
}

/// What may be done to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// See that it exists and read its description. No payloads.
    View,
    /// Read record keys, values and headers — the sensitive surface.
    ///
    /// Only meaningful on [`Resource::Topic`].
    MessagesRead,
    /// Every action this resource has. `all` in the config file.
    All,
}

impl Action {
    /// The concrete actions `self` stands for on a resource.
    fn expand(self, resource: Resource) -> BTreeSet<Self> {
        match self {
            Self::All if resource == Resource::Topic => {
                [Self::View, Self::MessagesRead].into_iter().collect()
            }
            Self::All => [Self::View].into_iter().collect(),
            other => [other].into_iter().collect(),
        }
    }
}

/// One line of a role's `permissions` list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Permission {
    /// What this is about.
    pub resource: Resource,
    /// Which ones, by name. `*` is the only wildcard and may appear anywhere.
    ///
    /// Absent means every one of them. Ignored for a resource that has no
    /// names — see [`Resource::is_named`].
    #[serde(default)]
    pub value: Option<String>,
    /// What may be done.
    pub actions: Vec<Action>,
}

impl Permission {
    /// Everything, on everything — what a role called `admin` is.
    #[must_use]
    pub fn all() -> Vec<Self> {
        Resource::every()
            .into_iter()
            .map(|resource| Self {
                resource,
                value: None,
                actions: vec![Action::All],
            })
            .collect()
    }

    /// Whether this permission covers one action on one named thing.
    fn covers(&self, resource: Resource, action: Action, name: Option<&str>) -> bool {
        if self.resource != resource {
            return false;
        }
        if !self
            .actions
            .iter()
            .any(|held| held.expand(resource).contains(&action))
        {
            return false;
        }
        match (&self.value, name) {
            // A pattern only constrains a resource that has names.
            (Some(pattern), Some(name)) if resource.is_named() => matches(pattern, name),
            _ => true,
        }
    }
}

/// One entry in the `roles:` list.
///
/// ```yaml
/// roles:
///   - name: prod-support
///     subjects: ["kaas-rs:support"]   # a subject, a group, a login, an email
///     clusters: ["prod-*"]            # cluster ids; `*` matches every one
///     permissions:
///       - resource: topic
///         value: "public-*"
///         actions: [view, messages_read]
///       - resource: consumer
///         actions: [view]
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
    /// Cluster ids, with `*` as a wildcard. Empty matches every cluster.
    pub clusters: Vec<String>,
    /// An additional label selector over those clusters. Every pair must
    /// match.
    ///
    /// kafbat-ui names clusters and stops there; a fleet grown past a handful
    /// wants `env: prod` as well, and both are cheap to honour.
    pub cluster_labels: BTreeMap<String, String>,
    /// What the role permits on them.
    pub permissions: Vec<Permission>,
}

impl Role {
    /// A role that permits everything, everywhere.
    #[must_use]
    pub fn admin(name: impl Into<String>, subjects: Vec<String>) -> Self {
        Self {
            name: name.into(),
            subjects,
            clusters: vec!["*".to_owned()],
            cluster_labels: BTreeMap::new(),
            permissions: Permission::all(),
        }
    }

    /// Whether this role applies to a caller.
    fn covers(&self, who: &Principal) -> bool {
        self.subjects.iter().any(|subject| {
            if subject == "*" {
                return who.is_authenticated();
            }
            who.identifiers().any(|id| id == subject)
        })
    }

    /// Whether this role selects a cluster.
    fn selects(&self, cluster: &str, labels: &BTreeMap<String, String>) -> bool {
        let by_name = self.clusters.is_empty()
            || self
                .clusters
                .iter()
                .any(|pattern| matches(pattern, cluster));
        let by_label = self
            .cluster_labels
            .iter()
            .all(|(key, value)| labels.get(key).is_some_and(|found| found == value));
        by_name && by_label
    }
}

/// The `roles:` list, plus whether it is being enforced at all.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    roles: Vec<Arc<Role>>,
    enforcing: bool,
}

impl Policy {
    /// No authentication configured: one anonymous caller, and they are an
    /// administrator.
    ///
    /// The honest name for what a deployment without an identity provider
    /// does. Every development instance runs this way, and it is what kaas-ui
    /// did before any of this existed.
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
    #[must_use]
    pub fn access(&self, who: &Principal) -> Access {
        if !self.enforcing {
            return Access::admin();
        }
        Access {
            roles: self
                .roles
                .iter()
                .filter(|role| role.covers(who))
                .map(Arc::clone)
                .collect(),
            administrator: false,
        }
    }

    /// Rebuild an access from role names a previous resolution produced.
    ///
    /// A session cookie carries role *names* rather than the groups claim they
    /// came from — a 4 KB budget and an organisation with three hundred teams
    /// do not fit in the same cookie. The cost is that editing `roles:` takes
    /// effect at the next login rather than the next request: this is a login
    /// system, not a live permission bus. A name that no longer matches a
    /// configured role is dropped, so deleting a role revokes it for sessions
    /// already open.
    #[must_use]
    pub fn access_for_roles(&self, names: &[String]) -> Access {
        if !self.enforcing {
            return Access::admin();
        }
        Access {
            roles: self
                .roles
                .iter()
                .filter(|role| names.iter().any(|name| *name == role.name))
                .map(Arc::clone)
                .collect(),
            administrator: false,
        }
    }
}

/// One caller's resolved view of the fleet.
#[derive(Debug, Clone, Default)]
pub struct Access {
    roles: Vec<Arc<Role>>,
    administrator: bool,
}

impl Access {
    /// Everything, everywhere — the caller on a deployment with no identity
    /// provider configured.
    #[must_use]
    pub fn admin() -> Self {
        Self {
            roles: Vec::new(),
            administrator: true,
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
            administrator: false,
        }
    }

    /// Whether this caller holds everything by virtue of there being no
    /// policy to hold them to.
    #[must_use]
    pub fn is_administrator(&self) -> bool {
        self.administrator
    }

    /// Whether a cluster exists for this caller.
    ///
    /// Any permission at all makes it visible. A role that selects a cluster
    /// and permits nothing on it is a config mistake rather than a way to hide
    /// one, and hiding it here would make that mistake silent.
    #[must_use]
    pub fn sees(&self, cluster: &str, labels: &BTreeMap<String, String>) -> bool {
        self.administrator
            || self
                .roles
                .iter()
                .any(|role| role.selects(cluster, labels) && !role.permissions.is_empty())
    }

    /// Whether one action is permitted on one named thing.
    ///
    /// `name` is the topic or group; `None` asks about the resource in
    /// general, which is what a list endpoint needs.
    #[must_use]
    pub fn may(
        &self,
        cluster: &str,
        labels: &BTreeMap<String, String>,
        resource: Resource,
        action: Action,
        name: Option<&str>,
    ) -> bool {
        self.administrator
            || self.roles.iter().any(|role| {
                role.selects(cluster, labels)
                    && role
                        .permissions
                        .iter()
                        .any(|permission| permission.covers(resource, action, name))
            })
    }

    /// Whether this caller may read payloads out of one named topic.
    ///
    /// The action *and* the value pattern in one question, because they are
    /// one question: a role granting `messages_read` on `public-*` says
    /// nothing about `payments`, and asking the halves separately is how that
    /// becomes a leak.
    #[must_use]
    pub fn may_read_topic(
        &self,
        cluster: &str,
        labels: &BTreeMap<String, String>,
        topic: &str,
    ) -> bool {
        self.may(
            cluster,
            labels,
            Resource::Topic,
            Action::MessagesRead,
            Some(topic),
        )
    }

    /// What this caller may do on a cluster, projected for the frontend.
    ///
    /// Per resource, so the UI can hide a section it must not offer — a tab
    /// that 403s on click is worse than no tab, which is the same rule the
    /// capability projection follows for what a *broker* cannot answer.
    #[must_use]
    pub fn permissions(
        &self,
        cluster: &str,
        labels: &BTreeMap<String, String>,
    ) -> BTreeMap<Resource, BTreeSet<Action>> {
        let mut held: BTreeMap<Resource, BTreeSet<Action>> = BTreeMap::new();
        for resource in Resource::every() {
            for action in [Action::View, Action::MessagesRead] {
                if action == Action::MessagesRead && resource != Resource::Topic {
                    continue;
                }
                // `None`: this is the cluster-level answer. A role scoped to
                // `public-*` still reports `messages_read` here, and the
                // per-topic check refuses the topics outside it.
                if self.may(cluster, labels, resource, action, None) {
                    held.entry(resource).or_default().insert(action);
                }
            }
        }
        held
    }

    /// The names of the roles that covered this caller.
    pub fn role_names(&self) -> impl Iterator<Item = &str> {
        self.roles.iter().map(|role| role.name.as_str())
    }
}

/// Glob matching, with `*` as the only wildcard.
///
/// Hand-written rather than pulled in: the pattern language is one character
/// wide, and a dependency here would be a larger surface than the function.
/// Text either side of a `*` is anchored — `public-*` does not match
/// `not-public-orders` — because a pattern that matches in the middle of a
/// name is a pattern nobody can review.
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
        // No `*` in the pattern at all: the prefix strip above only proves it
        // is a prefix.
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
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// Someone whose provider put them in these groups.
    fn member(groups: &[&str]) -> Principal {
        Principal::new("sub-1").with_groups(groups.iter().map(|g| (*g).to_owned()))
    }

    /// Someone the provider knows by these other names — a login, an email.
    fn person(aliases: &[&str]) -> Principal {
        Principal::new("sub-1").with_aliases(aliases.iter().map(|a| (*a).to_owned()))
    }

    fn view_topics(name: &str, subjects: &[&str], clusters: &[&str]) -> Role {
        Role {
            name: name.to_owned(),
            subjects: subjects.iter().map(|s| (*s).to_owned()).collect(),
            clusters: clusters.iter().map(|c| (*c).to_owned()).collect(),
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Topic,
                value: None,
                actions: vec![Action::View],
            }],
        }
    }

    #[test]
    fn no_policy_means_an_administrator() {
        // The default this whole file bends around: a deployment with no
        // identity provider has one caller and they can do everything.
        let access = Policy::open().access(&Principal::anonymous());
        assert!(access.is_administrator());
        assert!(access.sees("prod", &labels(&[])));
        assert!(access.may_read_topic("prod", &labels(&[]), "payments"));
        assert!(access.may(
            "prod",
            &labels(&[]),
            Resource::Consumer,
            Action::View,
            Some("anything")
        ));
    }

    #[test]
    fn an_admin_role_permits_every_resource_and_action() {
        let policy = Policy::enforcing(vec![Role::admin("admin", vec!["Woestebanaan".to_owned()])]);
        let access = policy.access(&Principal::new("sub"));
        assert!(!access.is_administrator(), "held by role, not by default");

        // The subject did not match — the principal has no identifiers here.
        assert!(!access.sees("kaas", &labels(&[])));

        // The login is an *identity*, not the display name: a name somebody
        // chose must never grant access, which is why `identifiers` covers the
        // subject, the aliases and the groups and stops there.
        let mine = policy.access(
            &Principal::new("sub")
                .with_name(Some("Ben".to_owned()))
                .with_aliases(["Woestebanaan".to_owned()]),
        );
        assert!(mine.sees("kaas", &labels(&[])));
        assert!(mine.may_read_topic("kaas", &labels(&[]), "payments"));
        assert!(mine.may(
            "kaas",
            &labels(&[]),
            Resource::ClusterConfig,
            Action::View,
            None
        ));
        assert!(mine.may("kaas", &labels(&[]), Resource::Consumer, Action::View, None));
    }

    #[test]
    fn a_login_or_an_email_names_a_subject_as_well_as_the_sub_claim() {
        // Dex's `sub` is an opaque blob; a role names a person by the login or
        // the email, which is what `identifiers` carries.
        let policy = Policy::enforcing(vec![Role::admin("admin", vec!["Woestebanaan".to_owned()])]);
        let by_login =
            Principal::new("CgVhZG1pbhIFbG9jYWw").with_aliases(["Woestebanaan".to_owned()]);
        assert!(policy.access(&by_login).sees("kaas", &labels(&[])));
    }

    /// The point of reading the `groups` claim at all.
    ///
    /// A role naming a *set* rather than a person — an Entra group through
    /// Dex's `microsoft` connector, `org:team` through its GitHub one. Until
    /// the claim was read this matched nobody, and did so silently: the config
    /// validated, the login succeeded, and the fleet came back empty.
    #[test]
    fn a_group_names_a_subject_just_as_a_login_does() {
        let policy = Policy::enforcing(vec![view_topics("platform", &["platform-team"], &["*"])]);

        assert!(
            policy
                .access(&member(&["platform-team"]))
                .sees("kaas", &labels(&[]))
        );

        // And the same string as somebody's login rather than their group is
        // still a match — `identifiers` is one flat set on purpose.
        assert!(
            policy
                .access(&person(&["platform-team"]))
                .sees("kaas", &labels(&[]))
        );

        // Belonging to a different group grants nothing.
        assert!(
            !policy
                .access(&member(&["some-other-team"]))
                .sees("kaas", &labels(&[]))
        );
    }

    #[test]
    fn view_without_messages_read_is_the_boundary_that_matters() {
        let policy = Policy::enforcing(vec![view_topics("readers", &["*"], &["*"])]);
        let access = policy.access(&member(&[]));

        assert!(access.sees("kaas", &labels(&[])));
        assert!(access.may("kaas", &labels(&[]), Resource::Topic, Action::View, None));
        assert!(!access.may_read_topic("kaas", &labels(&[]), "anything"));
        // And a resource this role says nothing about is not permitted.
        assert!(!access.may("kaas", &labels(&[]), Resource::Consumer, Action::View, None));
    }

    #[test]
    fn a_value_pattern_scopes_payloads_to_named_topics() {
        let policy = Policy::enforcing(vec![Role {
            name: "support".to_owned(),
            subjects: vec!["*".to_owned()],
            clusters: vec!["*".to_owned()],
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Topic,
                value: Some("public-*".to_owned()),
                actions: vec![Action::All],
            }],
        }]);
        let access = policy.access(&member(&[]));

        assert!(access.may_read_topic("kaas", &labels(&[]), "public-orders"));
        assert!(!access.may_read_topic("kaas", &labels(&[]), "payments"));
        // The cluster-level projection still advertises the action, because
        // the tab exists for the topics it covers.
        assert!(
            access.permissions("kaas", &labels(&[]))[&Resource::Topic]
                .contains(&Action::MessagesRead)
        );
    }

    #[test]
    fn clusters_are_matched_by_id_and_optionally_by_label() {
        let mut role = view_topics("prod", &["*"], &["prod-*"]);
        role.cluster_labels = labels(&[("env", "prod")]);
        let policy = Policy::enforcing(vec![role]);
        let access = policy.access(&member(&[]));

        assert!(access.sees("prod-eu", &labels(&[("env", "prod")])));
        // Right name, wrong label.
        assert!(!access.sees("prod-eu", &labels(&[("env", "dev")])));
        // Right label, wrong name.
        assert!(!access.sees("staging", &labels(&[("env", "prod")])));
    }

    #[test]
    fn a_caller_in_no_role_sees_an_empty_fleet_rather_than_an_error() {
        let policy = Policy::enforcing(vec![view_topics("dev", &["kaas-rs"], &["*"])]);
        let access = policy.access(&member(&["someone-else"]));

        assert!(!access.is_administrator());
        assert!(!access.sees("kaas", &labels(&[])));
        assert_eq!(access.role_names().count(), 0);
    }

    #[test]
    fn a_star_subject_never_covers_the_anonymous_caller() {
        let policy = Policy::enforcing(vec![view_topics("all", &["*"], &["*"])]);
        assert!(
            !policy
                .access(&Principal::anonymous())
                .sees("kaas", &labels(&[]))
        );
        assert!(policy.access(&member(&[])).sees("kaas", &labels(&[])));
    }

    #[test]
    fn a_session_is_rebuilt_from_its_role_names_and_a_deleted_role_stops_applying() {
        let policy = Policy::enforcing(vec![view_topics("dev", &["*"], &["*"])]);
        assert!(
            policy
                .access_for_roles(&["dev".to_owned()])
                .sees("kaas", &labels(&[]))
        );
        // Revocation without waiting for a session to expire.
        assert!(
            !policy
                .access_for_roles(&["gone".to_owned()])
                .sees("kaas", &labels(&[]))
        );
    }

    #[test]
    fn all_means_every_action_the_resource_has_and_no_more() {
        assert_eq!(
            Action::All.expand(Resource::Topic),
            [Action::View, Action::MessagesRead].into_iter().collect()
        );
        // A consumer group has no payloads to read.
        assert_eq!(
            Action::All.expand(Resource::Consumer),
            [Action::View].into_iter().collect()
        );
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
        assert!(!matches("orders-*", "shadow-orders-eu"));
    }

    #[test]
    fn a_role_deserializes_from_the_documented_shape() {
        let parsed: Role = serde_json::from_str(
            r#"{
                "name": "prod-support",
                "subjects": ["kaas-rs:support"],
                "clusters": ["prod-*"],
                "permissions": [
                    {"resource": "topic", "value": "public-*", "actions": ["view", "messages_read"]},
                    {"resource": "consumer", "actions": ["view"]}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(parsed.name, "prod-support");
        assert_eq!(parsed.permissions.len(), 2);
        assert_eq!(parsed.permissions[0].resource, Resource::Topic);
        assert_eq!(parsed.permissions[1].actions, [Action::View]);
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_ignored() {
        // `permission:` for `permissions:` must not produce a role that grants
        // nothing while looking like it grants something.
        assert!(serde_json::from_str::<Role>(r#"{"name": "typo", "permission": []}"#).is_err());
        // And an action that does not exist here — every write verb kafbat-ui
        // has — is refused by name rather than ignored.
        assert!(
            serde_json::from_str::<Permission>(
                r#"{"resource": "topic", "actions": ["messages_produce"]}"#
            )
            .is_err()
        );
    }
}

//! The write gate: the only place a [`Grant`] is made.
//!
//! A write happens when three things agree, each configured somewhere
//! different so no single mistake opens all of them:
//!
//! * **the ceiling** — which operations the target's `writes.allow` admits at
//!   all. Absent, there is nothing to agree with and no grant exists;
//! * **a role** — which callers may perform the operation on which named
//!   things, asked with [`Access::role_permits`] so the open deployment's
//!   anonymous administrator holds nothing here;
//! * **ownership** — whether Git owns the named thing. Declared state that Git
//!   owns refuses edits through the UI; operating on it (a restart) does not
//!   change what Git declares and is allowed.
//!
//! [`Grant`] has a private field, so this module is the only place one can be
//! constructed, and every mutating client method in the workspace takes one.
//! The type system then says authorization happened — and `cargo xtask ci`
//! greps that `Grant {` is built nowhere else.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::identity::Principal;
use crate::policy::{Access, Action, Resource};

/// One write kaas-ui can perform. A closed set: the catalogue in
/// `docs/13-phase-10-administration.md`, landing one action at a time.
///
/// The serialised names are what `writes.allow` lists and what `/api` reports
/// as available, so a config file and a page speak the same words.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, ToSchema,
)]
pub enum Operation {
    /// Restart a connector, optionally its tasks, optionally only failed ones.
    #[serde(rename = "connector.restart")]
    ConnectorRestart,
    /// Pause a connector: tasks keep their resources and process nothing.
    #[serde(rename = "connector.pause")]
    ConnectorPause,
    /// Resume a paused or stopped connector.
    #[serde(rename = "connector.resume")]
    ConnectorResume,
    /// Stop a connector: tasks are shut down and release their resources.
    #[serde(rename = "connector.stop")]
    ConnectorStop,
}

/// How much a write can cost, which decides confirmation and the GitOps rule —
/// never authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    /// Not state; reversible; nothing lost. Allowed on things Git owns.
    Operate,
    /// Changes declared state. Refused on things Git owns.
    Edit,
    /// Loses data or position. Refused on things Git owns; typed confirmation.
    Destroy,
}

impl Operation {
    /// Every operation, in catalogue order.
    pub const ALL: [Self; 4] = [
        Self::ConnectorRestart,
        Self::ConnectorPause,
        Self::ConnectorResume,
        Self::ConnectorStop,
    ];

    /// The tier.
    ///
    /// Pause, resume and stop are **edit**, not operate: under Strimzi they are
    /// `spec.state` on the `KafkaConnector` — declared state the operator puts
    /// back. A restart is not state; Strimzi models it as an annotation.
    #[must_use]
    pub fn tier(self) -> Tier {
        match self {
            Self::ConnectorRestart => Tier::Operate,
            Self::ConnectorPause | Self::ConnectorResume | Self::ConnectorStop => Tier::Edit,
        }
    }

    /// The role permission this operation needs.
    #[must_use]
    pub fn permission(self) -> (Resource, Action) {
        match self {
            Self::ConnectorRestart
            | Self::ConnectorPause
            | Self::ConnectorResume
            | Self::ConnectorStop => (Resource::Connector, Action::Operate),
        }
    }

    /// The name, as config and the API spell it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ConnectorRestart => "connector.restart",
            Self::ConnectorPause => "connector.pause",
            Self::ConnectorResume => "connector.resume",
            Self::ConnectorStop => "connector.stop",
        }
    }
}

/// What a write is aimed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The environment.
    pub environment: String,
    /// The cluster a role's `clusters:` selects against — for a connector, the
    /// Kafka cluster its Connect cluster keeps state in.
    pub cluster: String,
    /// That cluster's labels, including `env`.
    pub labels: BTreeMap<String, String>,
    /// The addressed host of the thing: a Connect cluster id, or the Kafka
    /// cluster's own id.
    pub host: String,
    /// The named thing, where there is one: a connector, a topic.
    pub name: Option<String>,
}

/// Who owns a named thing in Git, when somebody does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Owned {
    /// Where it lives, as configured — a repository path, for a link.
    pub source: Option<String>,
}

/// Why a write was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// Nobody signed in, or the deployment has no identity provider. The open
    /// deployment views everything and writes nothing.
    Anonymous,
    /// The target's `writes.allow` does not admit this operation.
    NotEnabled(Operation),
    /// No role of the caller's permits it on this target.
    Forbidden(Operation),
    /// Git owns the named thing and the operation changes declared state.
    Owned(Operation, Owned),
}

impl Denied {
    /// The sentence to render.
    #[must_use]
    pub fn message(&self, target: &Target) -> String {
        let what = target.name.as_deref().unwrap_or(&target.host);
        match self {
            Self::Anonymous => "writing needs a signed-in caller; this deployment's anonymous \
                                caller can view everything and change nothing"
                .to_owned(),
            Self::NotEnabled(op) => format!(
                "{} is not enabled on {}: its `writes.allow` does not list it",
                op.name(),
                target.host
            ),
            Self::Forbidden(op) => format!(
                "{} on {what:?} needs `{:?}` on `{:?}` for cluster {} in a role of yours",
                op.name(),
                op.permission().1,
                op.permission().0,
                target.cluster
            ),
            Self::Owned(op, owned) => format!(
                "{what:?} is managed in Git{}: {} changes declared state, so change it there — the \
                 operator would put back an edit made here",
                owned
                    .source
                    .as_deref()
                    .map(|source| format!(" ({source})"))
                    .unwrap_or_default(),
                op.name()
            ),
        }
    }
}

/// Proof that one operation on one target was authorized for one caller.
///
/// Not `Clone`, not constructible outside this module. A mutating client
/// method takes `&Grant` and checks the operation and target it carries, so a
/// grant for restarting `a` cannot stop `b`.
#[derive(Debug)]
pub struct Grant {
    operation: Operation,
    target: Target,
    subject: String,
    display_name: String,
    roles: Vec<String>,
    id: String,
    _seal: Seal,
}

/// What makes [`Grant`] unconstructible elsewhere.
#[derive(Debug)]
struct Seal;

impl Grant {
    /// The operation authorized.
    #[must_use]
    pub fn operation(&self) -> Operation {
        self.operation
    }

    /// The target authorized.
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// Who, as the audit records them.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// What the subject renders as.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The roles that covered the caller.
    #[must_use]
    pub fn roles(&self) -> &[String] {
        &self.roles
    }

    /// The id the intent and outcome audit lines share.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Access {
    /// Whether every gate would agree, without minting anything.
    ///
    /// What a page uses to decide which buttons to draw — never what a write
    /// is performed with.
    pub fn check(
        &self,
        operation: Operation,
        target: &Target,
        ceiling: &[Operation],
        owned: Option<&Owned>,
    ) -> Result<(), Denied> {
        if self.is_administrator() {
            return Err(Denied::Anonymous);
        }
        if !ceiling.contains(&operation) {
            return Err(Denied::NotEnabled(operation));
        }
        let (resource, action) = operation.permission();
        if !self.role_permits(
            &target.cluster,
            &target.labels,
            resource,
            action,
            target.name.as_deref(),
        ) {
            return Err(Denied::Forbidden(operation));
        }
        if let Some(owned) = owned
            && operation.tier() != Tier::Operate
        {
            return Err(Denied::Owned(operation, owned.clone()));
        }
        Ok(())
    }

    /// Authorize one write, or say which gate refused it.
    ///
    /// **The only constructor of [`Grant`].**
    pub fn authorize(
        &self,
        who: &Principal,
        operation: Operation,
        target: Target,
        ceiling: &[Operation],
        owned: Option<&Owned>,
    ) -> Result<Grant, Denied> {
        if !who.is_authenticated() {
            return Err(Denied::Anonymous);
        }
        self.check(operation, &target, ceiling, owned)?;
        Ok(Grant {
            operation,
            target,
            subject: who.subject().to_owned(),
            display_name: who.display_name().to_owned(),
            roles: self.role_names().map(str::to_owned).collect(),
            id: next_id(),
            _seal: Seal,
        })
    }
}

/// An id unique within this process and ordered by time: milliseconds since
/// the epoch and a counter. Enough to join an intent line to its outcome.
fn next_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("w-{millis}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Permission, Policy, Role};

    fn target(name: &str) -> Target {
        Target {
            environment: "dev".into(),
            cluster: "strimzi".into(),
            labels: [("env".to_owned(), "dev".to_owned())].into(),
            host: "connect".into(),
            name: Some(name.into()),
        }
    }

    fn operator(pattern: &str) -> Policy {
        Policy::enforcing(vec![Role {
            name: "oncall".into(),
            subjects: vec!["alice".into()],
            clusters: vec!["strimzi".into()],
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Connector,
                value: Some(pattern.into()),
                actions: vec![Action::Operate],
            }],
        }])
    }

    const ALL: &[Operation] = &Operation::ALL;

    #[test]
    fn the_open_deployment_views_everything_and_writes_nothing() {
        let anonymous = Principal::anonymous();
        let access = Policy::open().access(&anonymous);
        assert!(access.is_administrator());
        let refused = access
            .authorize(
                &anonymous,
                Operation::ConnectorRestart,
                target("x"),
                ALL,
                None,
            )
            .unwrap_err();
        assert_eq!(refused, Denied::Anonymous);
    }

    #[test]
    fn all_three_gates_must_agree() {
        let alice = Principal::new("alice");
        let access = operator("gtfs-*").access(&alice);

        // Ceiling, role and ownership all agree.
        let grant = access
            .authorize(
                &alice,
                Operation::ConnectorRestart,
                target("gtfs-a"),
                ALL,
                None,
            )
            .unwrap();
        assert_eq!(grant.operation(), Operation::ConnectorRestart);
        assert_eq!(grant.roles(), ["oncall"]);

        // The ceiling does not list it.
        assert_eq!(
            access
                .authorize(
                    &alice,
                    Operation::ConnectorRestart,
                    target("gtfs-a"),
                    &[],
                    None
                )
                .unwrap_err(),
            Denied::NotEnabled(Operation::ConnectorRestart)
        );
        // The role's pattern does not cover the name.
        assert_eq!(
            access
                .authorize(
                    &alice,
                    Operation::ConnectorRestart,
                    target("mqtt"),
                    ALL,
                    None
                )
                .unwrap_err(),
            Denied::Forbidden(Operation::ConnectorRestart)
        );
    }

    #[test]
    fn git_ownership_refuses_edits_and_permits_operating() {
        let alice = Principal::new("alice");
        let access = operator("*").access(&alice);
        let owned = Owned {
            source: Some("k3s-cluster/apps/strimzi".into()),
        };
        assert!(
            access
                .authorize(
                    &alice,
                    Operation::ConnectorRestart,
                    target("a"),
                    ALL,
                    Some(&owned)
                )
                .is_ok(),
            "a restart is not declared state"
        );
        assert!(matches!(
            access
                .authorize(
                    &alice,
                    Operation::ConnectorPause,
                    target("a"),
                    ALL,
                    Some(&owned)
                )
                .unwrap_err(),
            Denied::Owned(Operation::ConnectorPause, _)
        ));
    }

    #[test]
    fn a_role_that_only_views_operates_nothing() {
        let alice = Principal::new("alice");
        let access =
            Policy::enforcing(vec![Role::admin("admin", vec!["alice".into()])]).access(&alice);
        // `all` on every resource includes operate on connectors: an admin
        // role is an admin role.
        assert!(
            access
                .check(Operation::ConnectorStop, &target("a"), ALL, None)
                .is_ok()
        );

        let viewer = Policy::enforcing(vec![Role {
            name: "viewer".into(),
            subjects: vec!["alice".into()],
            clusters: vec!["*".into()],
            cluster_labels: BTreeMap::new(),
            permissions: vec![Permission {
                resource: Resource::Topic,
                value: None,
                actions: vec![Action::All],
            }],
        }])
        .access(&alice);
        assert_eq!(
            viewer
                .check(Operation::ConnectorRestart, &target("a"), ALL, None)
                .unwrap_err(),
            Denied::Forbidden(Operation::ConnectorRestart)
        );
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(next_id(), next_id());
    }

    #[test]
    fn the_names_are_the_config_words() {
        for operation in Operation::ALL {
            let json = serde_json::to_string(&operation).unwrap();
            assert_eq!(json, format!("\"{}\"", operation.name()));
        }
    }
}

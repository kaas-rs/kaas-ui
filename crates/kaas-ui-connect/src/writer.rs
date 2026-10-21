//! The one file in this crate that can change a Connect cluster.
//!
//! [`ConnectWriter`]'s only constructor, [`ConnectClient::writer`], takes a
//! [`Grant`], and a `Grant` is minted only by the gate in `kaas-ui-auth` —
//! after the Connect cluster's `writes.allow`, a role, and Git ownership have
//! all agreed. Every method then checks that the grant is for *this*
//! operation on *this* connector, so a grant to restart `a` cannot stop `b`
//! or restart `c`. A mismatch is a kaas-ui bug and fails as one.
//!
//! Configs travel as [`PreparedConfig`]: built here, from what a caller
//! submitted, with withheld values put back from the worker's own copy — and
//! with no `Serialize`, so the secrets it holds can reach the worker and
//! nowhere else. What leaves for display or audit is [`ConnectWriter::describe`],
//! redacted by the same rules as every read.
//!
//! `cargo xtask ci` lets a non-GET be spelled here and nowhere else in the
//! crate.

use std::collections::BTreeMap;

use kaas_ui_auth::{Grant, Operation};

use crate::client::{ConnectClient, segment};
use crate::fault::ConnectFault;
use crate::model::{RedactedConfig, StoredOffset, Validation};
use crate::redact::REDACTED;
use crate::wire;

/// Writes to one connector, as one grant authorized.
#[derive(Debug)]
pub struct ConnectWriter<'a> {
    client: &'a ConnectClient,
    grant: &'a Grant,
}

/// A connector config ready to send: what was submitted, with every withheld
/// value restored from the worker's copy.
///
/// Deliberately not `Serialize` and not `Clone`. It holds secrets, and the
/// only way out is to the worker, through a writer.
pub struct PreparedConfig {
    name: String,
    config: BTreeMap<String, String>,
}

impl std::fmt::Debug for PreparedConfig {
    /// The keys, never the values: the leak that happens is `?config` in a
    /// log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedConfig")
            .field("name", &self.name)
            .field("keys", &self.config.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PreparedConfig {
    /// The connector this config is for.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `connector.class`, where set.
    #[must_use]
    pub fn class(&self) -> Option<&str> {
        self.config.get("connector.class").map(String::as_str)
    }

    /// `tasks.max`, where set.
    #[must_use]
    pub fn tasks_max(&self) -> Option<&str> {
        self.config.get("tasks.max").map(String::as_str)
    }
}

impl ConnectClient {
    /// The writer for a granted operation on this Connect cluster.
    ///
    /// # Errors
    ///
    /// [`ConnectFault::Rejected`] when the grant names a different Connect
    /// cluster — which only a kaas-ui bug can produce.
    pub fn writer<'a>(&'a self, grant: &'a Grant) -> Result<ConnectWriter<'a>, ConnectFault> {
        if grant.target().host != self.id() {
            return Err(ConnectFault::Rejected(format!(
                "a grant for {:?} was presented to Connect cluster {:?}",
                grant.target().host,
                self.id()
            )));
        }
        Ok(ConnectWriter {
            client: self,
            grant,
        })
    }
}

impl ConnectWriter<'_> {
    /// `POST /connectors/{c}/restart?includeTasks=&onlyFailed=`.
    ///
    /// With both flags false Connect restarts the connector instance alone;
    /// `includeTasks` adds its tasks, and `onlyFailed` narrows either to the
    /// ones that failed — which, on a healthy connector, restarts nothing.
    pub async fn restart(
        &self,
        include_tasks: bool,
        only_failed: bool,
    ) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorRestart])?;
        self.send(
            reqwest::Method::POST,
            &format!(
                "/connectors/{}/restart?includeTasks={include_tasks}&onlyFailed={only_failed}",
                segment(name)
            ),
            None,
        )
        .await
        .map(drop)
    }

    /// `POST /connectors/{c}/tasks/{t}/restart`.
    pub async fn restart_task(&self, task: i32) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorRestartTask])?;
        self.send(
            reqwest::Method::POST,
            &format!("/connectors/{}/tasks/{task}/restart", segment(name)),
            None,
        )
        .await
        .map(drop)
    }

    /// `PUT /connectors/{c}/pause`.
    pub async fn pause(&self) -> Result<(), ConnectFault> {
        self.state(Operation::ConnectorPause, "pause").await
    }

    /// `PUT /connectors/{c}/resume`.
    pub async fn resume(&self) -> Result<(), ConnectFault> {
        self.state(Operation::ConnectorResume, "resume").await
    }

    /// `PUT /connectors/{c}/stop`.
    pub async fn stop(&self) -> Result<(), ConnectFault> {
        self.state(Operation::ConnectorStop, "stop").await
    }

    /// `PUT /connectors/{c}/topics/reset` — forget the active-topic set. The
    /// topics themselves are untouched.
    pub async fn reset_topics(&self) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorResetTopics])?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/topics/reset", segment(name)),
            None,
        )
        .await
        .map(drop)
    }

    /// An edited config, with every `******` put back from the worker's copy.
    ///
    /// The edit form shows what a read shows — secrets withheld — and a key
    /// left at `******` means "keep the current value". The value comes from
    /// the worker, here, and never travelled to the browser.
    ///
    /// # Errors
    ///
    /// [`ConnectFault::NotFound`] if the connector is gone;
    /// [`ConnectFault::Rejected`] for a `******` on a key the connector does
    /// not hold, or a `name` that is not this connector's.
    pub async fn prepare_edit(
        &self,
        submitted: BTreeMap<String, String>,
    ) -> Result<PreparedConfig, ConnectFault> {
        let name = self.connector(&[Operation::ConnectorEdit])?.to_owned();
        let current = self.client.raw_config(&name).await?;
        let mut config = BTreeMap::new();
        for (key, value) in submitted {
            if value == REDACTED {
                let kept = current.get(&key).ok_or_else(|| {
                    ConnectFault::Invalid(format!(
                        "{key:?} is `{REDACTED}`, which keeps a withheld value — and {name:?} \
                         holds no value for it to keep"
                    ))
                })?;
                config.insert(key, kept.clone());
            } else {
                config.insert(key, value);
            }
        }
        check_name(&name, &config)?;
        Ok(PreparedConfig { name, config })
    }

    /// A new connector's config, as submitted.
    ///
    /// # Errors
    ///
    /// [`ConnectFault::Rejected`] for a missing `connector.class`, a `name`
    /// key that disagrees, or a `******` — there is no current value to keep.
    pub fn prepare_create(
        &self,
        submitted: BTreeMap<String, String>,
    ) -> Result<PreparedConfig, ConnectFault> {
        let name = self.connector(&[Operation::ConnectorCreate])?.to_owned();
        if let Some((key, _)) = submitted.iter().find(|(_, value)| *value == REDACTED) {
            return Err(ConnectFault::Invalid(format!(
                "{key:?} is `{REDACTED}`, the placeholder for a withheld value; a new connector \
                 has none to keep"
            )));
        }
        if !submitted.contains_key("connector.class") {
            return Err(ConnectFault::Invalid(
                "a connector needs `connector.class`".to_owned(),
            ));
        }
        check_name(&name, &submitted)?;
        Ok(PreparedConfig {
            name,
            config: submitted,
        })
    }

    /// The config as it may be shown and audited: redacted like every read.
    pub async fn describe(&self, prepared: &PreparedConfig) -> RedactedConfig {
        self.client.redacted(&prepared.config).await
    }

    /// `PUT /connector-plugins/{class}/config/validate` — what the plugin
    /// thinks of a config, applying nothing.
    ///
    /// A `PUT` that writes nothing, here because it is the dry run of an edit
    /// or a create and is spelled like a write.
    pub async fn validate(&self, prepared: &PreparedConfig) -> Result<Validation, ConnectFault> {
        self.connector(&[Operation::ConnectorEdit, Operation::ConnectorCreate])?;
        let class = prepared.class().ok_or_else(|| {
            ConnectFault::Invalid("a connector needs `connector.class`".to_owned())
        })?;
        let mut config = prepared.config.clone();
        config.insert("name".to_owned(), prepared.name.clone());
        let body = serde_json::to_value(&config)
            .map_err(|error| ConnectFault::Rejected(error.to_string()))?;
        let answer = self
            .send(
                reqwest::Method::PUT,
                &format!("/connector-plugins/{}/config/validate", segment(class)),
                Some(&body),
            )
            .await?;
        let infos: wire::ConfigInfos =
            serde_json::from_value(answer.unwrap_or_default()).map_err(|error| {
                ConnectFault::Misconfigured(format!(
                    "the validation answer is not the Connect REST API's: {error}"
                ))
            })?;
        Ok(Validation::from(infos))
    }

    /// `PUT /connectors/{c}/config` — replace an existing connector's config.
    pub async fn edit(&self, prepared: PreparedConfig) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorEdit])?;
        if prepared.name != name {
            return Err(mismatch(name, &prepared.name));
        }
        let body = serde_json::to_value(&prepared.config)
            .map_err(|error| ConnectFault::Rejected(error.to_string()))?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/config", segment(name)),
            Some(&body),
        )
        .await
        .map(drop)
    }

    /// `POST /connectors` — create a connector.
    pub async fn create(&self, prepared: PreparedConfig) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorCreate])?;
        if prepared.name != name {
            return Err(mismatch(name, &prepared.name));
        }
        let body = serde_json::json!({ "name": prepared.name, "config": prepared.config });
        self.send(reqwest::Method::POST, "/connectors", Some(&body))
            .await
            .map(drop)
    }

    /// `DELETE /connectors/{c}`.
    pub async fn delete(&self) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorDelete])?;
        self.send(
            reqwest::Method::DELETE,
            &format!("/connectors/{}", segment(name)),
            None,
        )
        .await
        .map(drop)
    }

    /// `DELETE /connectors/{c}/offsets`. The worker refuses unless the
    /// connector is stopped, and that refusal is passed through verbatim.
    pub async fn reset_offsets(&self) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorResetOffsets])?;
        self.send(
            reqwest::Method::DELETE,
            &format!("/connectors/{}/offsets", segment(name)),
            None,
        )
        .await
        .map(drop)
    }

    /// `PATCH /connectors/{c}/offsets`. Stopped connectors only, as above.
    pub async fn alter_offsets(&self, offsets: Vec<StoredOffset>) -> Result<(), ConnectFault> {
        let name = self.connector(&[Operation::ConnectorAlterOffsets])?;
        let body = serde_json::json!({ "offsets": offsets });
        self.send(
            reqwest::Method::PATCH,
            &format!("/connectors/{}/offsets", segment(name)),
            Some(&body),
        )
        .await
        .map(drop)
    }

    async fn state(&self, operation: Operation, verb: &str) -> Result<(), ConnectFault> {
        let name = self.connector(&[operation])?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/{verb}", segment(name)),
            None,
        )
        .await
        .map(drop)
    }

    /// The connector this grant is for, if it is for one of `expected`.
    fn connector(&self, expected: &[Operation]) -> Result<&str, ConnectFault> {
        let granted = self.grant.operation();
        if !expected.contains(&granted) {
            return Err(ConnectFault::Rejected(format!(
                "a grant for {} was used for {}",
                granted.name(),
                expected
                    .iter()
                    .map(|operation| operation.name())
                    .collect::<Vec<_>>()
                    .join(" or ")
            )));
        }
        self.grant.target().name.as_deref().ok_or_else(|| {
            ConnectFault::Rejected(format!("a grant for {} names no connector", granted.name()))
        })
    }

    /// **The only mutating request in this crate.** Answers with the body, if
    /// the worker sent one.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Option<serde_json::Value>, ConnectFault> {
        let client = self.client;
        let mut request = client
            .http()
            .request(method, client.url_for(path))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let request = client.authenticate(request);
        client.count();
        let response = request
            .send()
            .await
            .map_err(|error| client.unreachable(&error))?;
        // Whatever happened, the cached list may now be wrong.
        client.invalidate();
        if !response.status().is_success() {
            return Err(client.fault_of(response, path).await);
        }
        Ok(response.json::<serde_json::Value>().await.ok())
    }
}

/// A `name` key, where present, must be the connector's.
fn check_name(name: &str, config: &BTreeMap<String, String>) -> Result<(), ConnectFault> {
    match config.get("name") {
        Some(named) if named != name => Err(ConnectFault::Invalid(format!(
            "this change is for connector {name:?}, and its config names {named:?}"
        ))),
        _ => Ok(()),
    }
}

fn mismatch(expected: &str, found: &str) -> ConnectFault {
    ConnectFault::Rejected(format!(
        "this change is for connector {expected:?}, and its config names {found:?}"
    ))
}

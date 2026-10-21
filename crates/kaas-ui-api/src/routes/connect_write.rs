//! Writes to Kafka Connect — Phase 10, on Phase 9d's catalogue.
//!
//! Every handler here runs one sequence, in [`perform`]:
//!
//! 1. **the lookup** — the Connect cluster as this caller may see it, or 404;
//! 2. **the gate** — `Access::authorize` mints a `Grant` or names the gate
//!    that refused: 403 for the ceiling, a role, or nobody signed in;
//! 3. **confirmation** — a destroy-tier write must carry the connector's name
//!    in `confirm`, typed;
//! 4. **the intent**, audited — a write that cannot be recorded is not sent;
//! 5. **the write**, through a `ConnectWriter` that checks the grant again;
//! 6. **the outcome**, audited.
//!
//! An edit or a create can be a **dry run** instead: the plugin validates the
//! config and nothing is applied or audited, because nothing changed.
//!
//! One shape for every route — a `POST` carrying a JSON body — rather than
//! the REST verbs Connect itself uses: a destroy-tier write carries a typed
//! confirmation, and `DELETE` bodies are badly supported by the clients and
//! proxies between a browser and here. The openapi test holds the exact set.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use kaas_ui_auth::{Change, Denied, Operation, Tier};
use kaas_ui_connect::{ConnectFault, RedactedConfig, StoredOffset, Validation};
use kaas_ui_core::registry::ConnectHandle;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{ApiError, ApiResult, AppState, Caller};

/// One write a page could offer.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteOption {
    /// The catalogue operation.
    pub operation: Operation,
    /// Operate, edit or destroy.
    pub tier: Tier,
    /// Whether every gate agrees for this caller now.
    pub allowed: bool,
    /// Which gate refuses, where one does.
    pub refused_by: Option<Refusal>,
    /// The sentence a refusal renders.
    pub reason: Option<String>,
}

/// Which gate refused a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// Nobody is signed in.
    Anonymous,
    /// The ceiling does not list it.
    NotEnabled,
    /// No role of the caller's permits it.
    Forbidden,
}

impl Refusal {
    fn of(denied: &Denied) -> Self {
        match denied {
            Denied::Anonymous => Self::Anonymous,
            Denied::NotEnabled(_) => Self::NotEnabled,
            Denied::Forbidden(_) => Self::Forbidden,
        }
    }
}

fn option(
    caller: &Caller,
    handle: &ConnectHandle,
    target: &kaas_ui_auth::Target,
    operation: Operation,
) -> WriteOption {
    let checked = caller.access().check(operation, target, handle.ceiling());
    WriteOption {
        operation,
        tier: operation.tier(),
        allowed: checked.is_ok(),
        refused_by: checked.as_ref().err().map(Refusal::of),
        reason: checked.err().map(|denied| denied.message(target)),
    }
}

/// The writes a connector page can offer: everything on the Connect cluster's
/// ceiling but `create`, checked against this caller. Empty without a
/// ceiling.
pub(crate) fn write_options(
    state: &AppState,
    handle: &ConnectHandle,
    connector: &str,
    caller: &Caller,
) -> Vec<WriteOption> {
    if handle.ceiling().is_empty() {
        return Vec::new();
    }
    let target = state.registry().connect_target(handle, Some(connector));
    Operation::ALL
        .into_iter()
        .filter(|operation| *operation != Operation::ConnectorCreate)
        .filter(|operation| handle.ceiling().contains(operation))
        .map(|operation| option(caller, handle, &target, operation))
        .collect()
}

/// Creating a connector on this Connect cluster, where the ceiling lists it.
pub(crate) fn create_option(
    state: &AppState,
    handle: &ConnectHandle,
    caller: &Caller,
) -> Option<WriteOption> {
    if !handle.ceiling().contains(&Operation::ConnectorCreate) {
        return None;
    }
    let target = state.registry().connect_target(handle, None);
    Some(option(caller, handle, &target, Operation::ConnectorCreate))
}

/// What a write to an existing connector carries. Every field is optional;
/// each operation reads the ones it needs and is refused without them.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteRequest {
    /// `restart`: its tasks too.
    #[serde(default)]
    pub include_tasks: bool,
    /// `restart`: only what has failed — on a healthy connector, nothing.
    #[serde(default)]
    pub only_failed: bool,
    /// `edit`: the whole config. A value left at `******` keeps the current
    /// one, which the server restores from the worker; a key left out is
    /// removed.
    #[serde(default)]
    pub config: Option<BTreeMap<String, String>>,
    /// `alter-offsets`: the offsets to set.
    #[serde(default)]
    pub offsets: Option<Vec<StoredOffset>>,
    /// Destroy tier: the connector's name, typed.
    #[serde(default)]
    pub confirm: Option<String>,
    /// `edit`: validate against the plugin and apply nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A new connector.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRequest {
    /// Its name.
    pub name: String,
    /// Its config, `connector.class` included.
    pub config: BTreeMap<String, String>,
    /// Validate against the plugin and create nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What a write did — or, for a dry run, what it would have met.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// The id the intent and outcome audit lines share. `None` for a dry run,
    /// which changed nothing and is not audited.
    pub audit_id: Option<String>,
    /// Whether anything was sent that changes the connector.
    pub applied: bool,
    /// What was done.
    pub operation: Operation,
    /// To which connector.
    pub connector: String,
    /// For a dry run: what the plugin made of the config.
    pub validation: Option<Validation>,
    /// For an edit or a create: the config as sent, redacted.
    pub config: Option<RedactedConfig>,
}

/// The path segment each operation is addressed by.
fn operation_of(segment: &str) -> Option<Operation> {
    Some(match segment {
        "restart" => Operation::ConnectorRestart,
        "pause" => Operation::ConnectorPause,
        "resume" => Operation::ConnectorResume,
        "stop" => Operation::ConnectorStop,
        "reset-topics" => Operation::ConnectorResetTopics,
        "edit" => Operation::ConnectorEdit,
        "delete" => Operation::ConnectorDelete,
        "reset-offsets" => Operation::ConnectorResetOffsets,
        "alter-offsets" => Operation::ConnectorAlterOffsets,
        _ => return None,
    })
}

/// `POST /api/environments/{env}/connect-clusters/{connect}/connectors/{name}/{operation}`
///
/// `operation` is one of `restart`, `pause`, `resume`, `stop`,
/// `reset-topics`, `edit`, `delete`, `reset-offsets`, `alter-offsets`.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}/{operation}",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("name" = String, Path, description = "Connector name"),
        ("operation" = String, Path, description = "restart | pause | resume | stop | reset-topics | edit | delete | reset-offsets | alter-offsets"),
        ("X-Kaas-Ui" = String, Header, description = "Must be `1`: proves the request came from kaas-ui's own page"),
    ),
    request_body(content = Option<WriteRequest>, description = "What the operation needs; most need nothing"),
    responses(
        (status = 200, description = "Done and audited, or a dry run's verdict", body = WriteResult),
        (status = 400, description = "Missing input, an unconfirmed destroy, or the worker refused the request as given", body = crate::error::ApiErrorBody),
        (status = 403, description = "Not enabled here, not permitted, or not signed in", body = crate::error::ApiErrorBody),
        (status = 404, description = "No such Connect cluster, connector or operation"),
        (status = 502, description = "The workers refused or could not be reached"),
    ),
    tag = "connect",
)]
pub async fn write(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, name, segment)): Path<(String, String, String, String)>,
    body: Option<Json<WriteRequest>>,
) -> ApiResult<Json<WriteResult>> {
    let operation = operation_of(&segment).ok_or_else(|| {
        ApiError::not_found(format!(
            "no connector operation {segment:?}: restart, pause, resume, stop, reset-topics, \
             edit, delete, reset-offsets or alter-offsets"
        ))
    })?;
    let request = body.map(|Json(request)| request).unwrap_or_default();
    perform(&state, &caller, &env, &id, &name, operation, request, None).await
}

/// `POST /api/environments/{env}/connect-clusters/{connect}/connectors/{name}/tasks/{task}/restart`
#[utoipa::path(
    post,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors/{name}/tasks/{task}/restart",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("name" = String, Path, description = "Connector name"),
        ("task" = i32, Path, description = "Task number"),
        ("X-Kaas-Ui" = String, Header, description = "Must be `1`"),
    ),
    responses(
        (status = 200, description = "Restarted and audited", body = WriteResult),
        (status = 403, description = "Not enabled here, not permitted, or not signed in", body = crate::error::ApiErrorBody),
        (status = 404, description = "No such Connect cluster, connector or task"),
        (status = 502, description = "The workers refused or could not be reached"),
    ),
    tag = "connect",
)]
pub async fn write_task(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id, name, task)): Path<(String, String, String, i32)>,
) -> ApiResult<Json<WriteResult>> {
    perform(
        &state,
        &caller,
        &env,
        &id,
        &name,
        Operation::ConnectorRestartTask,
        WriteRequest::default(),
        Some(task),
    )
    .await
}

/// `POST /api/environments/{env}/connect-clusters/{connect}/connectors`
///
/// Creates a connector — or, with `dryRun`, asks the plugin what it makes of
/// the config.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/connect-clusters/{connect}/connectors",
    params(
        ("env" = String, Path, description = "Environment id"),
        ("connect" = String, Path, description = "Connect cluster id"),
        ("X-Kaas-Ui" = String, Header, description = "Must be `1`"),
    ),
    request_body = CreateRequest,
    responses(
        (status = 200, description = "Created and audited, or a dry run's verdict", body = WriteResult),
        (status = 400, description = "An unusable config, or the worker refused it", body = crate::error::ApiErrorBody),
        (status = 403, description = "Not enabled here, not permitted, or not signed in", body = crate::error::ApiErrorBody),
        (status = 502, description = "The workers refused or could not be reached"),
    ),
    tag = "connect",
)]
pub async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Path((env, id)): Path<(String, String)>,
    Json(request): Json<CreateRequest>,
) -> ApiResult<Json<WriteResult>> {
    let name = request.name.trim().to_owned();
    if name.is_empty() {
        return Err(ApiError::bad_request("a connector needs a name"));
    }
    let write = WriteRequest {
        config: Some(request.config),
        dry_run: request.dry_run,
        ..WriteRequest::default()
    };
    perform(
        &state,
        &caller,
        &env,
        &id,
        &name,
        Operation::ConnectorCreate,
        write,
        None,
    )
    .await
}

/// The one sequence every write runs. See the module header.
#[allow(clippy::too_many_arguments)]
async fn perform(
    state: &AppState,
    caller: &Caller,
    env: &str,
    id: &str,
    name: &str,
    operation: Operation,
    request: WriteRequest,
    task: Option<i32>,
) -> ApiResult<Json<WriteResult>> {
    let handle = state.connect_cluster(env, id, caller)?;
    let client = handle.client();
    let target = state.registry().connect_target(&handle, Some(name));

    let grant = caller
        .access()
        .authorize(
            caller.principal(),
            operation,
            target.clone(),
            handle.ceiling(),
        )
        .map_err(|denied| ApiError::denied(&denied, &target))?;

    if operation.tier() == Tier::Destroy && request.confirm.as_deref() != Some(name) {
        return Err(ApiError::bad_request(format!(
            "{} cannot be undone: send the connector's name, {name:?}, as `confirm`",
            operation.name()
        )));
    }
    if request.dry_run
        && !matches!(
            operation,
            Operation::ConnectorEdit | Operation::ConnectorCreate
        )
    {
        return Err(ApiError::bad_request(format!(
            "{} has no dry run; only an edit or a create can be validated first",
            operation.name()
        )));
    }

    if operation == Operation::ConnectorAlterOffsets && request.offsets.is_none() {
        return Err(ApiError::bad_request(
            "connector.alter_offsets needs `offsets`",
        ));
    }

    let writer = client.writer(&grant)?;
    let prepared = match operation {
        Operation::ConnectorEdit | Operation::ConnectorCreate => {
            let config = request.config.clone().ok_or_else(|| {
                ApiError::bad_request(format!("{} needs a `config`", operation.name()))
            })?;
            Some(if operation == Operation::ConnectorEdit {
                writer.prepare_edit(config).await?
            } else {
                writer.prepare_create(config)?
            })
        }
        _ => None,
    };
    let described = match &prepared {
        Some(prepared) => Some(writer.describe(prepared).await),
        None => None,
    };

    if request.dry_run {
        let validation = match &prepared {
            Some(prepared) => Some(writer.validate(prepared).await?),
            None => None,
        };
        return Ok(Json(WriteResult {
            audit_id: None,
            applied: false,
            operation,
            connector: name.to_owned(),
            validation,
            config: described,
        }));
    }

    let parameters = match operation {
        Operation::ConnectorRestart => serde_json::json!({
            "includeTasks": request.include_tasks,
            "onlyFailed": request.only_failed,
        }),
        Operation::ConnectorRestartTask => serde_json::json!({ "task": task }),
        Operation::ConnectorEdit | Operation::ConnectorCreate => serde_json::json!({
            "config": described.as_ref().map(|config| {
                config
                    .entries
                    .iter()
                    .map(|entry| (entry.key.clone(), entry.value.clone()))
                    .collect::<BTreeMap<_, _>>()
            }),
        }),
        Operation::ConnectorAlterOffsets => serde_json::json!({ "offsets": request.offsets }),
        _ => serde_json::Value::Null,
    };
    let intent = Change::intent(&grant, parameters);
    state
        .audit()
        .record(&intent)
        .map_err(|error| ApiError::intent_unrecorded(&error.to_string()))?;

    let outcome = match (operation, prepared) {
        (Operation::ConnectorRestart, _) => {
            writer
                .restart(request.include_tasks, request.only_failed)
                .await
        }
        (Operation::ConnectorRestartTask, _) => writer.restart_task(task.unwrap_or_default()).await,
        (Operation::ConnectorPause, _) => writer.pause().await,
        (Operation::ConnectorResume, _) => writer.resume().await,
        (Operation::ConnectorStop, _) => writer.stop().await,
        (Operation::ConnectorResetTopics, _) => writer.reset_topics().await,
        (Operation::ConnectorEdit, Some(prepared)) => writer.edit(prepared).await,
        (Operation::ConnectorCreate, Some(prepared)) => writer.create(prepared).await,
        (Operation::ConnectorDelete, _) => writer.delete().await,
        (Operation::ConnectorResetOffsets, _) => writer.reset_offsets().await,
        (Operation::ConnectorAlterOffsets, _) => {
            writer
                .alter_offsets(request.offsets.unwrap_or_default())
                .await
        }
        (Operation::ConnectorEdit | Operation::ConnectorCreate, None) => Err(
            ConnectFault::Invalid(format!("{} needs a `config`", operation.name())),
        ),
    };
    state
        .audit()
        .record(&intent.outcome(outcome.as_ref().map(|_| ()).map_err(ToString::to_string)))
        .map_err(|error| ApiError::outcome_unrecorded(&error.to_string()))?;
    outcome?;

    Ok(Json(WriteResult {
        audit_id: Some(grant.id().to_owned()),
        applied: true,
        operation,
        connector: name.to_owned(),
        validation: None,
        config: described,
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use arc_swap::ArcSwap;
    use axum::http::StatusCode;
    use kaas_ui_auth::{Action, Audit, Permission, Policy, Principal, Resource, Role};
    use kaas_ui_core::{Config, Registry};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const SECRET: &str = "hunter2-do-not-leak";

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
        fn read(&self) -> Vec<serde_json::Value> {
            self.text()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    /// One request the worker saw.
    #[derive(Debug, Clone)]
    struct Seen {
        line: String,
        body: String,
    }

    #[derive(Clone, Default)]
    struct Worker(Arc<Mutex<Vec<Seen>>>);

    impl Worker {
        /// The requests that were not reads.
        fn writes(&self) -> Vec<Seen> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|seen| !seen.line.starts_with("GET "))
                .cloned()
                .collect()
        }
    }

    fn reply(method: &str, path: &str) -> (u16, String) {
        match (method, path) {
            ("GET", "/") => (200, r#"{"version":"stub","kafka_cluster_id":"k"}"#.into()),
            ("GET", "/connectors/mqtt") => (
                200,
                format!(
                    r#"{{"name":"mqtt","config":{{"connector.class":"io.x.Mqtt",
                        "connect.mqtt.password":"{SECRET}","connect.mqtt.hosts":"tcp://a"}},
                        "tasks":[],"type":"source"}}"#
                ),
            ),
            ("GET", "/connector-plugins/io.x.Mqtt/config") => (
                200,
                r#"[{"name":"connect.mqtt.password","type":"PASSWORD","group":"Connection"}]"#
                    .into(),
            ),
            ("PUT", "/connector-plugins/io.x.Mqtt/config/validate") => (
                200,
                r#"{"error_count":0,"configs":[{"value":{"name":"tasks.max","errors":[]}}]}"#
                    .into(),
            ),
            ("DELETE", "/connectors/running/offsets") => (
                400,
                r#"{"error_code":400,"message":"Connectors must be in the STOPPED state before their offsets can be modified"}"#
                    .into(),
            ),
            _ => (202, "{}".into()),
        }
    }

    async fn worker() -> (String, Worker) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = Worker::default();
        let seen = worker.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    let head_end = loop {
                        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                            break at + 4;
                        }
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    while buffer.len() < head_end + length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                    }
                    let mut words = head.lines().next().unwrap_or("").split_whitespace();
                    let method = words.next().unwrap_or("").to_owned();
                    let path = words.next().unwrap_or("").to_owned();
                    seen.0.lock().unwrap().push(Seen {
                        line: format!("{method} {path}"),
                        body: String::from_utf8_lossy(&buffer[head_end..]).to_string(),
                    });
                    let (status, body) = reply(&method, &path);
                    let response = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), worker)
    }

    fn state(url: &str, audit: Lines, allow: &str) -> AppState {
        let config = Config::from_yaml(&format!(
            r#"
auth:
  issuer: https://kaas.example/dex
  client_id: kaas-ui
  redirect_url: https://kaas.example/auth/callback
environments:
  - id: dev
    kafka_clusters:
      - id: strimzi
        bootstrap: ["strimzi:9092"]
    connect_clusters:
      - id: connect
        url: {url}
        kafka_cluster: strimzi
        writes:
          allow: {allow}
"#
        ))
        .unwrap();
        let topic_view = Permission {
            resource: Resource::Topic,
            value: None,
            actions: vec![Action::View],
        };
        let policy = Policy::enforcing(vec![
            Role {
                name: "oncall".into(),
                subjects: vec!["alice".into()],
                clusters: vec!["strimzi".into()],
                cluster_labels: BTreeMap::new(),
                permissions: vec![
                    topic_view.clone(),
                    Permission {
                        resource: Resource::Connector,
                        value: None,
                        actions: vec![Action::All],
                    },
                    Permission {
                        resource: Resource::Connect,
                        value: None,
                        actions: vec![Action::All],
                    },
                ],
            },
            Role {
                name: "viewer".into(),
                subjects: vec!["bob".into()],
                clusters: vec!["strimzi".into()],
                cluster_labels: BTreeMap::new(),
                permissions: vec![topic_view],
            },
        ]);
        AppState::new(
            Arc::new(ArcSwap::from_pointee(
                Registry::from_config(&config).unwrap(),
            )),
            policy,
        )
        .with_audit(Arc::new(Audit::to_writer(Box::new(audit))))
    }

    const EVERYTHING: &str = "[connector.restart, connector.restart_task, connector.pause, \
        connector.resume, connector.stop, connector.reset_topics, connector.edit, \
        connector.create, connector.delete, connector.reset_offsets, connector.alter_offsets]";

    fn caller(state: &AppState, who: &str) -> Caller {
        let principal = Principal::new(who);
        let access = state.policy().access(&principal);
        Caller::new(principal, access)
    }

    async fn post(
        state: &AppState,
        who: &str,
        name: &str,
        op: &str,
        request: WriteRequest,
    ) -> ApiResult<Json<WriteResult>> {
        write(
            State(state.clone()),
            caller(state, who),
            Path(("dev".into(), "connect".into(), name.into(), op.into())),
            Some(Json(request)),
        )
        .await
    }

    fn restart_request() -> WriteRequest {
        WriteRequest {
            include_tasks: true,
            only_failed: true,
            ..WriteRequest::default()
        }
    }

    #[tokio::test]
    async fn a_granted_restart_is_audited_before_and_after_and_reaches_the_worker() {
        let (url, worker) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone(), EVERYTHING);

        let Json(result) = post(&state, "alice", "gtfs", "restart", restart_request())
            .await
            .unwrap();
        assert!(result.applied);
        assert_eq!(
            worker
                .writes()
                .iter()
                .map(|s| s.line.as_str())
                .collect::<Vec<_>>(),
            ["POST /connectors/gtfs/restart?includeTasks=true&onlyFailed=true"]
        );

        let lines = audit.read();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0]["phase"], "intent");
        assert_eq!(lines[1]["phase"], "outcome");
        assert_eq!(lines[1]["ok"], true);
        assert_eq!(lines[0]["id"], lines[1]["id"]);
        assert_eq!(
            Some(lines[0]["id"].as_str().unwrap()),
            result.audit_id.as_deref()
        );
        assert_eq!(lines[0]["roles"], serde_json::json!(["oncall"]));
    }

    #[tokio::test]
    async fn each_gate_refuses_in_its_own_words_and_nothing_reaches_the_worker() {
        let (url, worker) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone(), "[connector.restart, connector.pause]");

        let forbidden = post(&state, "bob", "gtfs", "restart", restart_request())
            .await
            .unwrap_err();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let not_enabled = post(&state, "alice", "gtfs", "stop", WriteRequest::default())
            .await
            .unwrap_err();
        assert_eq!(not_enabled.status(), StatusCode::FORBIDDEN);

        let anonymous = write(
            State(state.clone()),
            Caller::new(
                Principal::anonymous(),
                state.policy().access(&Principal::anonymous()),
            ),
            Path((
                "dev".into(),
                "connect".into(),
                "gtfs".into(),
                "restart".into(),
            )),
            None,
        )
        .await
        .unwrap_err();
        // Anonymous sees no Connect cluster in an enforcing deployment: 404,
        // before any gate is asked.
        assert_eq!(anonymous.status(), StatusCode::NOT_FOUND);

        assert!(worker.writes().is_empty(), "{:?}", worker.writes());
        assert!(audit.read().is_empty(), "a refused write leaves no intent");
    }

    #[tokio::test]
    async fn a_failed_intent_record_means_no_write() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (url, worker) = worker().await;
        let state = state(&url, Lines::default(), EVERYTHING)
            .with_audit(Arc::new(Audit::to_writer(Box::new(Broken))));
        let error = post(&state, "alice", "gtfs", "restart", restart_request())
            .await
            .unwrap_err();
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(worker.writes().is_empty());
    }

    #[tokio::test]
    async fn an_edit_keeps_the_secret_for_the_worker_and_audits_only_the_redaction() {
        let (url, worker) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone(), EVERYTHING);
        let edit = |dry_run| WriteRequest {
            config: Some(
                [
                    ("connector.class", "io.x.Mqtt"),
                    ("connect.mqtt.password", kaas_ui_connect::REDACTED),
                    ("connect.mqtt.hosts", "tcp://b"),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            ),
            dry_run,
            ..WriteRequest::default()
        };

        // A dry run validates, applies nothing and audits nothing.
        let Json(dry) = post(&state, "alice", "mqtt", "edit", edit(true))
            .await
            .unwrap();
        assert!(!dry.applied && dry.audit_id.is_none());
        assert_eq!(dry.validation.unwrap().error_count, 0);
        assert!(audit.read().is_empty());
        assert_eq!(
            worker
                .writes()
                .iter()
                .map(|s| s.line.as_str())
                .collect::<Vec<_>>(),
            ["PUT /connector-plugins/io.x.Mqtt/config/validate"]
        );

        // Applied: the worker gets its own secret back, and nothing else does.
        let Json(applied) = post(&state, "alice", "mqtt", "edit", edit(false))
            .await
            .unwrap();
        assert!(applied.applied);
        let put = worker.writes().last().cloned().unwrap();
        assert_eq!(put.line, "PUT /connectors/mqtt/config");
        assert!(
            put.body.contains(SECRET),
            "the worker must get the kept secret"
        );

        let response = serde_json::to_string(&applied).unwrap();
        assert!(!response.contains(SECRET), "{response}");
        assert!(!audit.text().contains(SECRET), "{}", audit.text());
        let intent = &audit.read()[0];
        assert_eq!(
            intent["parameters"]["config"]["connect.mqtt.password"],
            "******"
        );
        assert_eq!(
            intent["parameters"]["config"]["connect.mqtt.hosts"],
            "tcp://b"
        );
    }

    #[tokio::test]
    async fn a_create_needs_create_on_the_connect_cluster_and_reaches_the_worker() {
        let (url, worker) = worker().await;
        let state = state(&url, Lines::default(), EVERYTHING);
        let request = |name: &str| CreateRequest {
            name: name.into(),
            config: [("connector.class", "io.x.Mqtt"), ("tasks.max", "2")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            dry_run: false,
        };

        let viewer = create(
            State(state.clone()),
            caller(&state, "bob"),
            Path(("dev".into(), "connect".into())),
            Json(request("fresh")),
        )
        .await
        .unwrap_err();
        assert_eq!(viewer.status(), StatusCode::FORBIDDEN);
        assert!(worker.writes().is_empty());

        let Json(created) = create(
            State(state.clone()),
            caller(&state, "alice"),
            Path(("dev".into(), "connect".into())),
            Json(request("fresh")),
        )
        .await
        .unwrap();
        assert!(created.applied);
        let sent = worker.writes();
        assert_eq!(sent.last().unwrap().line, "POST /connectors");
        assert!(sent.last().unwrap().body.contains(r#""name":"fresh""#));
    }

    #[tokio::test]
    async fn a_destroy_needs_the_name_typed_and_a_worker_refusal_passes_through() {
        let (url, worker) = worker().await;
        let audit = Lines::default();
        let state = state(&url, audit.clone(), EVERYTHING);

        let unconfirmed = post(&state, "alice", "gtfs", "delete", WriteRequest::default())
            .await
            .unwrap_err();
        assert_eq!(unconfirmed.status(), StatusCode::BAD_REQUEST);
        let mistyped = post(
            &state,
            "alice",
            "gtfs",
            "delete",
            WriteRequest {
                confirm: Some("gtfs-other".into()),
                ..WriteRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(mistyped.status(), StatusCode::BAD_REQUEST);
        assert!(worker.writes().is_empty() && audit.read().is_empty());

        let confirm = |name: &str| WriteRequest {
            confirm: Some(name.into()),
            ..WriteRequest::default()
        };
        assert!(
            post(&state, "alice", "gtfs", "delete", confirm("gtfs"))
                .await
                .is_ok()
        );
        assert_eq!(
            worker.writes().last().unwrap().line,
            "DELETE /connectors/gtfs"
        );

        // The worker refuses to reset a running connector's offsets: its 400
        // and its words reach the caller, and the outcome is audited as failed.
        let refused = post(
            &state,
            "alice",
            "running",
            "reset-offsets",
            confirm("running"),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        let outcome = audit.read().last().cloned().unwrap();
        assert_eq!(outcome["phase"], "outcome");
        assert_eq!(outcome["ok"], false);
        assert!(outcome["error"].as_str().unwrap().contains("STOPPED"));

        // Altering offsets without saying which is refused before the audit.
        let before = audit.read().len();
        let missing = post(&state, "alice", "gtfs", "alter-offsets", confirm("gtfs"))
            .await
            .unwrap_err();
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        assert_eq!(audit.read().len(), before);
    }

    #[tokio::test]
    async fn a_task_restart_and_a_topic_reset_address_their_own_calls() {
        let (url, worker) = worker().await;
        let state = state(&url, Lines::default(), EVERYTHING);
        let Json(task) = write_task(
            State(state.clone()),
            caller(&state, "alice"),
            Path(("dev".into(), "connect".into(), "gtfs".into(), 3)),
        )
        .await
        .unwrap();
        assert_eq!(task.operation, Operation::ConnectorRestartTask);
        let _reset = post(
            &state,
            "alice",
            "gtfs",
            "reset-topics",
            WriteRequest::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            worker
                .writes()
                .iter()
                .map(|s| s.line.as_str())
                .collect::<Vec<_>>(),
            [
                "POST /connectors/gtfs/tasks/3/restart",
                "PUT /connectors/gtfs/topics/reset"
            ]
        );
        // And a dry run of something that is not an edit is a 400.
        let no_dry = post(
            &state,
            "alice",
            "gtfs",
            "restart",
            WriteRequest {
                dry_run: true,
                ..WriteRequest::default()
            },
        )
        .await
        .unwrap_err();
        assert_eq!(no_dry.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_pages_offer_what_the_ceiling_lists_and_say_what_the_caller_may_do() {
        let (url, _) = worker().await;
        let state = state(
            &url,
            Lines::default(),
            "[connector.restart, connector.pause, connector.create]",
        );
        let handle = state
            .connect_cluster("dev", "connect", &caller(&state, "alice"))
            .unwrap();

        let alice = write_options(&state, &handle, "gtfs", &caller(&state, "alice"));
        assert_eq!(alice.len(), 2, "restart and pause: create is the cluster's");
        assert!(alice.iter().all(|option| option.allowed));
        let bob = write_options(&state, &handle, "gtfs", &caller(&state, "bob"));
        assert!(
            bob.iter()
                .all(|option| option.refused_by == Some(Refusal::Forbidden))
        );
        // Only what the ceiling lists is offered; create is the cluster's.
        assert!(
            !alice
                .iter()
                .any(|o| o.operation == Operation::ConnectorStop)
        );
        assert!(
            !alice
                .iter()
                .any(|o| o.operation == Operation::ConnectorCreate)
        );

        let create = create_option(&state, &handle, &caller(&state, "alice")).unwrap();
        assert!(create.allowed);
        let bob = create_option(&state, &handle, &caller(&state, "bob")).unwrap();
        assert_eq!(bob.refused_by, Some(Refusal::Forbidden));
    }
}

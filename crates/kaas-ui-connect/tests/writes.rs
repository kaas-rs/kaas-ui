//! The writer against a stub worker that records every request, body and all.
//!
//! A grant is real here — minted by the gate from a policy, because there is
//! no other way to get one — and the assertions are about what a grant can
//! and cannot be used for: the operation it names, on the connector it names,
//! on the Connect cluster it names. And about the one thing an edit must get
//! exactly right: a value left at `******` goes back to the worker as the
//! worker's own secret, and that secret goes nowhere else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use kaas_ui_auth::{Action, Operation, Permission, Policy, Principal, Resource, Role, Target};
use kaas_ui_connect::{ConnectClient, ConnectFault, ConnectSettings, REDACTED, StoredOffset};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SECRET: &str = "hunter2-do-not-leak";

/// One request as the worker saw it.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    body: String,
}

struct Stub {
    addr: SocketAddr,
    log: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// What the stub answers: a canned body for the reads an edit needs, `202 {}`
/// for everything else.
fn answer(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("GET", "/") => (200, r#"{"version":"stub","kafka_cluster_id":"k"}"#.into()),
        ("GET", "/connectors/mqtt") => (
            200,
            format!(
                r#"{{"name":"mqtt","config":{{"connector.class":"io.x.Mqtt","tasks.max":"1",
                    "connect.mqtt.password":"{SECRET}","connect.mqtt.hosts":"tcp://a"}},
                    "tasks":[],"type":"source"}}"#
            ),
        ),
        ("GET", "/connector-plugins/io.x.Mqtt/config") => (
            200,
            r#"[{"name":"connect.mqtt.password","type":"PASSWORD"}]"#.into(),
        ),
        ("PUT", "/connector-plugins/io.x.Mqtt/config/validate") => (
            200,
            r#"{"name":"io.x.Mqtt","error_count":1,"groups":[],"configs":[
                {"definition":{"name":"connect.mqtt.hosts"},
                 "value":{"name":"connect.mqtt.hosts","value":"nope","errors":["Invalid host"]}},
                {"definition":{"name":"tasks.max"},
                 "value":{"name":"tasks.max","value":"1","errors":[]}}]}"#
                .into(),
        ),
        _ => (202, "{}".into()),
    }
}

impl Stub {
    async fn serving() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let log = Arc::clone(&log);
            async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let log = Arc::clone(&log);
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
                        let body = String::from_utf8_lossy(&buffer[head_end..]).to_string();
                        let mut words = head.lines().next().unwrap_or("").split_whitespace();
                        let method = words.next().unwrap_or("").to_owned();
                        let path = words.next().unwrap_or("").to_owned();
                        let (status, reply) = answer(&method, &path);
                        log.lock().unwrap().push(Seen {
                            line: format!("{method} {path}"),
                            body,
                        });
                        let response = format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{reply}",
                            reply.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.shutdown().await;
                    });
                }
            }
        });
        Self { addr, log, task }
    }

    fn client(&self, id: &str) -> ConnectClient {
        ConnectClient::new(ConnectSettings::new(id, format!("http://{}", self.addr))).unwrap()
    }

    fn lines(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|seen| seen.line.clone())
            .collect()
    }

    /// The writes only: what changed something.
    fn writes(&self) -> Vec<Seen> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| !seen.line.starts_with("GET "))
            .cloned()
            .collect()
    }
}

fn grant(operation: Operation, host: &str, connector: &str) -> kaas_ui_auth::Grant {
    let alice = Principal::new("alice");
    let policy = Policy::enforcing(vec![Role {
        name: "oncall".into(),
        subjects: vec!["alice".into()],
        clusters: vec!["*".into()],
        cluster_labels: BTreeMap::new(),
        permissions: vec![
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
    }]);
    policy
        .access(&alice)
        .authorize(
            &alice,
            operation,
            Target {
                environment: "dev".into(),
                cluster: "strimzi".into(),
                labels: BTreeMap::new(),
                host: host.into(),
                name: Some(connector.into()),
            },
            &Operation::ALL,
            None,
        )
        .unwrap()
}

fn config(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[tokio::test]
async fn each_operation_sends_exactly_its_own_call() {
    let stub = Stub::serving().await;
    let client = stub.client("connect");

    let restart = grant(Operation::ConnectorRestart, "connect", "gtfs a/b");
    client
        .writer(&restart)
        .unwrap()
        .restart(true, true)
        .await
        .unwrap();
    let task = grant(Operation::ConnectorRestartTask, "connect", "gtfs");
    client.writer(&task).unwrap().restart_task(2).await.unwrap();
    let pause = grant(Operation::ConnectorPause, "connect", "gtfs");
    client.writer(&pause).unwrap().pause().await.unwrap();
    let resume = grant(Operation::ConnectorResume, "connect", "gtfs");
    client.writer(&resume).unwrap().resume().await.unwrap();
    let stop = grant(Operation::ConnectorStop, "connect", "gtfs");
    client.writer(&stop).unwrap().stop().await.unwrap();
    let topics = grant(Operation::ConnectorResetTopics, "connect", "gtfs");
    client
        .writer(&topics)
        .unwrap()
        .reset_topics()
        .await
        .unwrap();
    let reset = grant(Operation::ConnectorResetOffsets, "connect", "gtfs");
    client
        .writer(&reset)
        .unwrap()
        .reset_offsets()
        .await
        .unwrap();
    let alter = grant(Operation::ConnectorAlterOffsets, "connect", "gtfs");
    let offsets: Vec<StoredOffset> = serde_json::from_str(
        r#"[{"partition":{"kafka_topic":"t","kafka_partition":0},"offset":{"kafka_offset":42}}]"#,
    )
    .unwrap();
    client
        .writer(&alter)
        .unwrap()
        .alter_offsets(offsets)
        .await
        .unwrap();
    let delete = grant(Operation::ConnectorDelete, "connect", "gtfs");
    client.writer(&delete).unwrap().delete().await.unwrap();

    let writes = stub.writes();
    let lines: Vec<&str> = writes.iter().map(|seen| seen.line.as_str()).collect();
    assert_eq!(
        lines,
        [
            "POST /connectors/gtfs%20a%2Fb/restart?includeTasks=true&onlyFailed=true",
            "POST /connectors/gtfs/tasks/2/restart",
            "PUT /connectors/gtfs/pause",
            "PUT /connectors/gtfs/resume",
            "PUT /connectors/gtfs/stop",
            "PUT /connectors/gtfs/topics/reset",
            "DELETE /connectors/gtfs/offsets",
            "PATCH /connectors/gtfs/offsets",
            "DELETE /connectors/gtfs",
        ]
    );
    let altered: serde_json::Value = serde_json::from_str(&writes[7].body).unwrap();
    assert_eq!(altered["offsets"][0]["offset"]["kafka_offset"], 42);
}

#[tokio::test]
async fn an_edit_keeps_a_withheld_secret_by_sending_the_workers_own_copy() {
    let stub = Stub::serving().await;
    let client = stub.client("connect");
    let edit = grant(Operation::ConnectorEdit, "connect", "mqtt");
    let writer = client.writer(&edit).unwrap();

    // The form shows what a read shows: the password withheld. The operator
    // changes the hosts and leaves the password at `******`.
    let prepared = writer
        .prepare_edit(config(&[
            ("connector.class", "io.x.Mqtt"),
            ("tasks.max", "1"),
            ("connect.mqtt.password", REDACTED),
            ("connect.mqtt.hosts", "tcp://b"),
        ]))
        .await
        .unwrap();

    // What may be shown or audited is redacted, by the plugin's declaration.
    let described = writer.describe(&prepared).await;
    let shown = serde_json::to_string(&described).unwrap();
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(format!("{prepared:?}").contains("connect.mqtt.password"));
    assert!(!format!("{prepared:?}").contains(SECRET), "Debug leaked it");

    writer.edit(prepared).await.unwrap();
    let writes = stub.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].line, "PUT /connectors/mqtt/config");
    let sent: serde_json::Value = serde_json::from_str(&writes[0].body).unwrap();
    assert_eq!(
        sent["connect.mqtt.password"], SECRET,
        "the secret was not kept"
    );
    assert_eq!(sent["connect.mqtt.hosts"], "tcp://b");
}

#[tokio::test]
async fn a_placeholder_with_nothing_behind_it_is_refused() {
    let stub = Stub::serving().await;
    let client = stub.client("connect");
    let edit = grant(Operation::ConnectorEdit, "connect", "mqtt");
    let refused = client
        .writer(&edit)
        .unwrap()
        .prepare_edit(config(&[("connect.mqtt.token", REDACTED)]))
        .await
        .unwrap_err();
    assert!(matches!(refused, ConnectFault::Invalid(_)));

    // A new connector has no secret to keep at all.
    let create = grant(Operation::ConnectorCreate, "connect", "new");
    let writer = client.writer(&create).unwrap();
    assert!(matches!(
        writer.prepare_create(config(&[
            ("connector.class", "io.x.Mqtt"),
            ("connect.mqtt.password", REDACTED)
        ])),
        Err(ConnectFault::Invalid(_))
    ));
    // Nor a name that disagrees with the one granted.
    assert!(matches!(
        writer.prepare_create(config(&[
            ("connector.class", "io.x.Mqtt"),
            ("name", "other")
        ])),
        Err(ConnectFault::Invalid(_))
    ));
    assert!(stub.writes().is_empty());
}

#[tokio::test]
async fn a_create_is_validated_first_and_sent_with_its_name() {
    let stub = Stub::serving().await;
    let client = stub.client("connect");
    let create = grant(Operation::ConnectorCreate, "connect", "new");
    let writer = client.writer(&create).unwrap();
    let prepared = writer
        .prepare_create(config(&[
            ("connector.class", "io.x.Mqtt"),
            ("connect.mqtt.hosts", "nope"),
        ]))
        .unwrap();

    let verdict = writer.validate(&prepared).await.unwrap();
    assert_eq!(verdict.error_count, 1);
    assert_eq!(verdict.errors.len(), 1);
    assert_eq!(verdict.errors[0].key, "connect.mqtt.hosts");
    assert_eq!(verdict.errors[0].messages, ["Invalid host"]);

    writer.create(prepared).await.unwrap();
    let writes = stub.writes();
    assert_eq!(
        writes.iter().map(|s| s.line.as_str()).collect::<Vec<_>>(),
        [
            "PUT /connector-plugins/io.x.Mqtt/config/validate",
            "POST /connectors"
        ]
    );
    let validated: serde_json::Value = serde_json::from_str(&writes[0].body).unwrap();
    assert_eq!(
        validated["name"], "new",
        "validation is asked about the named connector"
    );
    let created: serde_json::Value = serde_json::from_str(&writes[1].body).unwrap();
    assert_eq!(created["name"], "new");
    assert_eq!(created["config"]["connector.class"], "io.x.Mqtt");
}

#[tokio::test]
async fn a_grant_is_for_one_operation_on_one_connector_on_one_cluster() {
    let stub = Stub::serving().await;
    let client = stub.client("connect");

    // A restart grant cannot stop, delete, or reset.
    let restart = grant(Operation::ConnectorRestart, "connect", "gtfs");
    let writer = client.writer(&restart).unwrap();
    assert!(matches!(
        writer.stop().await,
        Err(ConnectFault::Rejected(_))
    ));
    assert!(matches!(
        writer.delete().await,
        Err(ConnectFault::Rejected(_))
    ));
    assert!(matches!(
        writer.reset_offsets().await,
        Err(ConnectFault::Rejected(_))
    ));
    // An edit grant cannot create — even with a config prepared for an edit.
    let edit = grant(Operation::ConnectorEdit, "connect", "mqtt");
    let prepared = client
        .writer(&edit)
        .unwrap()
        .prepare_edit(config(&[("connector.class", "io.x.Mqtt")]))
        .await
        .unwrap();
    assert!(matches!(
        client.writer(&edit).unwrap().create(prepared).await,
        Err(ConnectFault::Rejected(_))
    ));
    // A grant for another Connect cluster is refused before anything is sent.
    let elsewhere = grant(Operation::ConnectorRestart, "mm2", "gtfs");
    assert!(matches!(
        client.writer(&elsewhere),
        Err(ConnectFault::Rejected(_))
    ));
    assert!(
        stub.writes().is_empty(),
        "nothing may reach the worker: {:?}",
        stub.lines()
    );
}

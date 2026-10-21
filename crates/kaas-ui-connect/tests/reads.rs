//! The client against a stub worker that records every request.
//!
//! Two claims this crate makes can only be checked from the far side of the
//! socket: that nothing but `GET` is ever sent, and that a secret in a
//! connector's config never comes back out of the client. The stub answers
//! with bodies shaped like the Strimzi worker's (measured 2026-10-20) and
//! keeps a log of `METHOD path`.
//!
//! Loopback only, so this runs in `cargo xtask ci`. The live half is
//! `cargo xtask live`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use kaas_ui_connect::{
    ConnectAuth, ConnectClient, ConnectFault, ConnectSettings, ConnectorStatus, ConnectorType,
    REDACTED, Redaction,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SECRET: &str = "hunter2-do-not-leak";

/// A worker that answers what it was told to and writes down what it was
/// asked.
struct Stub {
    addr: SocketAddr,
    log: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Stub {
    async fn serving(routes: Vec<(&str, u16, String)>) -> Self {
        let routes: HashMap<String, (u16, String)> = routes
            .into_iter()
            .map(|(path, status, body)| (path.to_owned(), (status, body)))
            .collect();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let log = Arc::clone(&log);
            async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let routes = routes.clone();
                    let log = Arc::clone(&log);
                    tokio::spawn(async move { answer(socket, &routes, &log).await });
                }
            }
        });
        Self { addr, log, task }
    }

    fn client(&self) -> ConnectClient {
        ConnectClient::new(
            ConnectSettings::new("connect", format!("http://{}", self.addr)).with_auth(
                ConnectAuth::Basic {
                    username: "kaas-ui".into(),
                    password: "irrelevant".into(),
                },
            ),
        )
        .unwrap()
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

async fn answer(
    mut socket: tokio::net::TcpStream,
    routes: &HashMap<String, (u16, String)>,
    log: &Mutex<Vec<String>>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let Ok(read) = socket.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buffer);
    let mut words = head.lines().next().unwrap_or("").split_whitespace();
    let method = words.next().unwrap_or("").to_owned();
    let path = words.next().unwrap_or("").to_owned();
    log.lock().unwrap().push(format!("{method} {path}"));

    let (status, body) = routes.get(&path).cloned().unwrap_or((
        404,
        r#"{"error_code":404,"message":"HTTP 404 Not Found"}"#.to_owned(),
    ));
    let response = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

fn root() -> (&'static str, u16, String) {
    (
        "/",
        200,
        r#"{"version":"stub-version","commit":"a18251bae0b825c6","kafka_cluster_id":"RPquI6Y0R3uuCqz8FMrSvg"}"#
            .to_owned(),
    )
}

fn mqtt_config() -> String {
    format!(
        r#"{{"connector.class":"io.lenses.streamreactor.connect.mqtt.source.MqttSourceConnector",
            "connect.mqtt.password":"{SECRET}",
            "connect.mqtt.hosts":"tcp://192.168.1.10:1883",
            "key.converter":"org.apache.kafka.connect.storage.StringConverter",
            "tasks.max":"1","name":"mqtt-home-connector"}}"#
    )
}

fn worker() -> Vec<(&'static str, u16, String)> {
    vec![
        root(),
        (
            "/connectors?expand=status&expand=info",
            200,
            format!(
                r#"{{
                "mqtt-home-connector": {{
                  "status": {{"name":"mqtt-home-connector",
                    "connector":{{"state":"RUNNING","worker_id":"w0:8083","version":"12.1.2"}},
                    "tasks":[{{"id":0,"state":"RUNNING","worker_id":"w0:8083","version":"12.1.2"}}],
                    "type":"source"}},
                  "info": {{"name":"mqtt-home-connector","config":{config},
                    "tasks":[{{"connector":"mqtt-home-connector","task":0}}],"type":"source"}}
                }},
                "s3-sink": {{
                  "status": {{"name":"s3-sink",
                    "connector":{{"state":"RUNNING","worker_id":"w1:8083"}},
                    "tasks":[{{"id":0,"state":"RUNNING","worker_id":"w1:8083"}},
                             {{"id":1,"state":"FAILED","worker_id":"w1:8083","trace":"boom"}}],
                    "type":"sink"}},
                  "info": {{"name":"s3-sink","config":{{"connector.class":"S3Sink",
                    "consumer.override.group.id":"s3-archive"}},"tasks":[],"type":"sink"}}
                }}
              }}"#,
                config = mqtt_config()
            ),
        ),
        (
            "/connectors/mqtt-home-connector",
            200,
            format!(
                r#"{{"name":"mqtt-home-connector","config":{},"tasks":[],"type":"source"}}"#,
                mqtt_config()
            ),
        ),
        (
            "/connectors/mqtt-home-connector/status",
            200,
            r#"{"name":"mqtt-home-connector","connector":{"state":"PAUSED","worker_id":"w0:8083"},
                "tasks":[{"id":0,"state":"PAUSED","worker_id":"w0:8083"}],"type":"source"}"#
                .to_owned(),
        ),
        (
            "/connectors/mqtt-home-connector/tasks",
            200,
            format!(
                r#"[{{"id":{{"connector":"mqtt-home-connector","task":0}},"config":{}}}]"#,
                mqtt_config()
            ),
        ),
        (
            "/connector-plugins/io.lenses.streamreactor.connect.mqtt.source.MqttSourceConnector/config",
            200,
            r#"[{"name":"connect.mqtt.password","type":"PASSWORD","group":"Connection",
                 "documentation":"The MQTT password"},
                {"name":"connect.mqtt.hosts","type":"STRING","group":"Connection"}]"#
                .to_owned(),
        ),
        (
            "/connectors/mqtt-home-connector/topics",
            200,
            r#"{"mqtt-home-connector":{"topics":["mqtt-b","mqtt-a"]}}"#.to_owned(),
        ),
        (
            "/connectors/gtfs/offsets",
            200,
            r#"{"offsets":[
                {"partition":{"file":"routes.txt"},"offset":null},
                {"partition":{"url":"https://example/tripUpdates.pb"},
                 "offset":{"entity_id":"2026-10-21:x","timestamp":1792568252534}}]}"#
                .to_owned(),
        ),
        (
            "/connectors/nope",
            404,
            r#"{"error_code":404,"message":"Connector nope not found"}"#.to_owned(),
        ),
        (
            "/connector-plugins?connectorsOnly=true",
            200,
            r#"[{"class":"org.apache.kafka.connect.mirror.MirrorSourceConnector","type":"source","version":"stub-version"},
                {"class":"io.lenses.streamreactor.connect.mqtt.sink.MqttSinkConnector","type":"sink","version":"12.1.2"}]"#
                .to_owned(),
        ),
    ]
}

#[tokio::test]
async fn every_request_is_a_get_whatever_was_read() {
    let stub = Stub::serving(worker()).await;
    let client = stub.client();

    client.ready().await.unwrap();
    client.connectors().await.unwrap();
    client.connector("mqtt-home-connector").await.unwrap();
    client.topics("mqtt-home-connector").await.unwrap();
    let _ = client.offsets("mqtt-home-connector").await;
    client.plugins().await.unwrap();
    let _ = client.connector("nope").await;

    let log = stub.log();
    assert!(log.len() >= 8, "{log:?}");
    for line in &log {
        assert!(
            line.starts_with("GET "),
            "a non-GET reached the worker: {line}"
        );
    }
}

#[tokio::test]
async fn the_list_folds_state_and_names_a_sinks_group() {
    let stub = Stub::serving(worker()).await;
    let connectors = stub.client().connectors().await.unwrap();

    let mqtt = connectors
        .iter()
        .find(|c| c.name == "mqtt-home-connector")
        .unwrap();
    assert_eq!(mqtt.kind, ConnectorType::Source);
    assert_eq!(mqtt.status, ConnectorStatus::Running);
    assert_eq!(mqtt.consumer_group, None, "a source commits to no group");
    assert_eq!(mqtt.version.as_deref(), Some("12.1.2"));

    let sink = connectors.iter().find(|c| c.name == "s3-sink").unwrap();
    assert_eq!(sink.status, ConnectorStatus::Degraded);
    assert_eq!((sink.tasks_running, sink.tasks_failed), (1, 1));
    assert_eq!(sink.consumer_group.as_deref(), Some("s3-archive"));
    assert_eq!(sink.tasks[1].trace.as_deref(), Some("boom"));

    // Neither the list nor anything derived from it carries a config value.
    let json = serde_json::to_string(&*connectors).unwrap();
    assert!(!json.contains(SECRET));
}

#[tokio::test]
async fn a_secret_never_comes_back_out_of_the_client() {
    let stub = Stub::serving(worker()).await;
    let detail = stub
        .client()
        .connector("mqtt-home-connector")
        .await
        .unwrap();

    assert_eq!(detail.summary.status, ConnectorStatus::Paused);
    assert_eq!(detail.config.redaction, Redaction::Declared);
    let password = detail
        .config
        .entries
        .iter()
        .find(|e| e.key == "connect.mqtt.password")
        .unwrap();
    assert!(password.redacted);
    assert_eq!(password.value, REDACTED);
    assert_eq!(password.group.as_deref(), Some("Connection"));

    // The task configs carry the same keys and get the same treatment.
    assert_eq!(detail.task_configs.len(), 1);
    let json = serde_json::to_string(&detail).unwrap();
    assert!(!json.contains(SECRET), "the secret leaked: {json}");
}

#[tokio::test]
async fn a_missing_connector_and_a_missing_endpoint_are_different_answers() {
    let stub = Stub::serving(worker()).await;
    let client = stub.client();

    assert!(matches!(
        client.connector("nope").await,
        Err(ConnectFault::NotFound(_))
    ));
    // The stub has no /offsets route, so it answers the way an older worker
    // does: Jersey's own 404.
    assert!(matches!(
        client.offsets("mqtt-home-connector").await,
        Err(ConnectFault::Absent(_))
    ));
}

#[tokio::test]
async fn topics_and_plugins_come_back_sorted() {
    let stub = Stub::serving(worker()).await;
    let client = stub.client();
    assert_eq!(
        client.topics("mqtt-home-connector").await.unwrap(),
        ["mqtt-a", "mqtt-b"]
    );
    let plugins = client.plugins().await.unwrap();
    assert_eq!(plugins.len(), 2);
    assert_eq!(plugins[1].kind, ConnectorType::Source);
}

#[tokio::test]
async fn refused_credentials_are_a_configuration_fault_not_an_outage() {
    let stub = Stub::serving(vec![(
        "/",
        401,
        r#"{"error_code":401,"message":"Unauthorized"}"#.to_owned(),
    )])
    .await;
    let client = stub.client();
    assert!(matches!(
        client.ready().await,
        Err(ConnectFault::Refused(_))
    ));
    assert_eq!(
        client.health().status(),
        kaas_ui_connect::ConnectStatus::Misconfigured
    );
    // Remembered for the backoff rather than re-asked by every caller.
    let before = client.requests();
    let _ = client.ready().await;
    assert_eq!(client.requests(), before);
}

#[tokio::test]
async fn the_list_is_believed_for_its_ttl() {
    let stub = Stub::serving(worker()).await;
    let client = stub.client();
    client.connectors().await.unwrap();
    let after_first = client.requests();
    client.connectors().await.unwrap();
    assert_eq!(
        client.requests(),
        after_first,
        "a second read within the TTL dialled"
    );
}

#[tokio::test]
async fn a_cleared_offset_is_a_null_and_not_a_failure() {
    // Measured: the GTFS schedule source reports `"offset": null` for every
    // file it has finished, and the first version of this client failed the
    // whole offsets read on it.
    let stub = Stub::serving(worker()).await;
    let offsets = stub.client().offsets("gtfs").await.unwrap();
    assert_eq!(offsets.len(), 2);
    assert_eq!(offsets[0].offset, None);
    assert_eq!(
        offsets[1].offset.as_ref().unwrap()["timestamp"],
        serde_json::json!(1792568252534_u64)
    );
}

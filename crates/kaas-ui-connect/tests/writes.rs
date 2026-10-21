//! The writer against a stub worker that records every request.
//!
//! A grant is real here — minted by the gate from a policy, because there is
//! no other way to get one — and the assertions are about what a grant can
//! and cannot be used for: the operation it names, on the connector it names,
//! on the Connect cluster it names.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use kaas_ui_auth::{Action, Operation, Permission, Policy, Principal, Resource, Role, Target};
use kaas_ui_connect::{ConnectClient, ConnectFault, ConnectSettings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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
    /// Answers every request 202 with an empty JSON body, and writes it down.
    async fn accepting() -> Self {
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
                        let mut chunk = [0_u8; 1024];
                        while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                            match socket.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                            }
                        }
                        let head = String::from_utf8_lossy(&buffer);
                        let mut words = head.lines().next().unwrap_or("").split_whitespace();
                        let line = format!(
                            "{} {}",
                            words.next().unwrap_or(""),
                            words.next().unwrap_or("")
                        );
                        log.lock().unwrap().push(line);
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 202 Accepted\r\ncontent-type: application/json\r\n\
                                  content-length: 2\r\nconnection: close\r\n\r\n{}",
                            )
                            .await;
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

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn grant(operation: Operation, host: &str, connector: &str) -> kaas_ui_auth::Grant {
    let alice = Principal::new("alice");
    let policy = Policy::enforcing(vec![Role {
        name: "oncall".into(),
        subjects: vec!["alice".into()],
        clusters: vec!["*".into()],
        cluster_labels: BTreeMap::new(),
        permissions: vec![Permission {
            resource: Resource::Connector,
            value: None,
            actions: vec![Action::Operate],
        }],
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

#[tokio::test]
async fn each_operation_sends_exactly_its_own_call() {
    let stub = Stub::accepting().await;
    let client = stub.client("connect");

    let restart = grant(Operation::ConnectorRestart, "connect", "gtfs a/b");
    client
        .writer(&restart)
        .unwrap()
        .restart(true, true)
        .await
        .unwrap();
    let pause = grant(Operation::ConnectorPause, "connect", "gtfs");
    client.writer(&pause).unwrap().pause().await.unwrap();
    let resume = grant(Operation::ConnectorResume, "connect", "gtfs");
    client.writer(&resume).unwrap().resume().await.unwrap();
    let stop = grant(Operation::ConnectorStop, "connect", "gtfs");
    client.writer(&stop).unwrap().stop().await.unwrap();

    assert_eq!(
        stub.log(),
        [
            "POST /connectors/gtfs%20a%2Fb/restart?includeTasks=true&onlyFailed=true",
            "PUT /connectors/gtfs/pause",
            "PUT /connectors/gtfs/resume",
            "PUT /connectors/gtfs/stop",
        ]
    );
}

#[tokio::test]
async fn a_grant_is_for_one_operation_on_one_connector_on_one_cluster() {
    let stub = Stub::accepting().await;
    let client = stub.client("connect");

    // A restart grant cannot stop.
    let restart = grant(Operation::ConnectorRestart, "connect", "gtfs");
    assert!(matches!(
        client.writer(&restart).unwrap().stop().await,
        Err(ConnectFault::Rejected(_))
    ));
    // A grant for another Connect cluster is refused before anything is sent.
    let elsewhere = grant(Operation::ConnectorRestart, "mm2", "gtfs");
    assert!(matches!(
        client.writer(&elsewhere),
        Err(ConnectFault::Rejected(_))
    ));
    assert!(
        stub.log().is_empty(),
        "nothing may reach the worker: {:?}",
        stub.log()
    );
}

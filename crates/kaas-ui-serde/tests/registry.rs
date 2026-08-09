//! The registry paths, against a stub that counts what it was asked.
//!
//! Phase 6's acceptance turns on two numbers — how many requests reached the
//! registry, and how many times a decoder was told the same id — and neither
//! can be observed from outside the process. So the fixture is a registry:
//! forty lines of `TcpListener` that answer ccompat and count connections.
//! A mocking library would have given the responses and not the count.
//!
//! Everything here is cluster-free and network-free beyond loopback, so it
//! runs in `cargo xtask ci`. The live half — a real Apicurio, the canary's
//! topic on both clusters — is `cargo xtask live`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use kaas_ui_serde::{
    Codec, MAX_PAYLOAD_CHARS, NamingStrategy, NoteKind, RegistryHandle, RegistrySettings,
    SchemaFormat, SubjectNaming, decode,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A schema registry that answers what it was told to and counts the asking.
struct Stub {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    paths: Arc<std::sync::Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Stub {
    async fn serving(routes: &[(&str, u16, &str)]) -> Self {
        let routes: HashMap<String, (u16, String)> = routes
            .iter()
            .map(|(path, status, body)| ((*path).to_owned(), (*status, (*body).to_owned())))
            .collect();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(std::sync::Mutex::new(Vec::new()));

        let task = tokio::spawn({
            let requests = Arc::clone(&requests);
            let paths = Arc::clone(&paths);
            async move {
                while let Ok((socket, _)) = listener.accept().await {
                    let routes = routes.clone();
                    let requests = Arc::clone(&requests);
                    let paths = Arc::clone(&paths);
                    tokio::spawn(async move {
                        answer(socket, &routes, &requests, &paths).await;
                    });
                }
            }
        });

        Self {
            addr,
            requests,
            paths,
            task,
        }
    }

    fn url(&self) -> String {
        format!("http://{}/apis/ccompat/v7", self.addr)
    }

    fn handle(&self) -> Arc<RegistryHandle> {
        Arc::new(
            RegistryHandle::new(RegistrySettings::new("dev", self.url())).expect("a handle builds"),
        )
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

/// One request, one response, one closed connection — so a count of requests
/// and a count of connections are the same number.
async fn answer(
    mut socket: tokio::net::TcpStream,
    routes: &HashMap<String, (u16, String)>,
    requests: &AtomicUsize,
    paths: &std::sync::Mutex<Vec<String>>,
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

    let request = String::from_utf8_lossy(&buffer);
    let Some(line) = request.lines().next() else {
        return;
    };
    let Some(target) = line.split_whitespace().nth(1) else {
        return;
    };
    // The path without its query: the converter appends `?deleted=true`, and
    // a route table that had to know that would be testing the wrong thing.
    let path = target.split('?').next().unwrap_or(target);
    let path = path.strip_prefix("/apis/ccompat/v7").unwrap_or(path);

    requests.fetch_add(1, Ordering::Relaxed);
    paths.lock().unwrap().push(path.to_owned());

    let (status, body) = routes.get(path).cloned().unwrap_or((
        404,
        r#"{"error_code":40401,"message":"not found"}"#.to_owned(),
    ));

    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/vnd.schemaregistry.v1+json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        if status == 200 { "OK" } else { "Error" },
        body.len(),
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

/// The heartbeat schema, and a record of it. Both are `schema_registry_converter`'s
/// own fixture, so a failure here is ours rather than a fixture we mistyped.
const HEARTBEAT: &str = r#"{"schema":"{\"type\":\"record\",\"name\":\"Heartbeat\",\"namespace\":\"nl.openweb.data\",\"fields\":[{\"name\":\"beat\",\"type\":\"long\"}]}","subject":"hb-value","version":1,"id":1}"#;
const HEARTBEAT_RECORD: [u8; 6] = [0, 0, 0, 0, 1, 6];

fn avro_registry() -> Vec<(&'static str, u16, &'static str)> {
    vec![
        ("/subjects", 200, r#"["hb-value"]"#),
        ("/schemas/ids/1", 200, HEARTBEAT),
    ]
}

#[tokio::test]
async fn an_avro_record_decodes_with_the_id_resolved_from_the_registry() {
    let stub = Stub::serving(&avro_registry()).await;
    let registry = stub.handle();

    let decoded = decode(
        Some(&registry),
        &HEARTBEAT_RECORD,
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;

    assert_eq!(decoded.value, Some(serde_json::json!({ "beat": 3 })));
    assert_eq!(decoded.payload.codec, Codec::Avro);
    assert!(decoded.payload.note.is_none(), "{:?}", decoded.payload.note);

    // The id is shown, and so is which registry answered — a schema id means
    // nothing without the registry it is an id in.
    let schema = decoded.payload.schema.expect("a resolved schema");
    assert_eq!(schema.id, 1);
    assert_eq!(schema.format, SchemaFormat::Avro);
    assert_eq!(schema.registry, "dev");
    assert_eq!(schema.subject.as_deref(), Some("hb-value"));
    assert_eq!(schema.name.as_deref(), Some("nl.openweb.data.Heartbeat"));

    // And the bytes travel beside the value, which is what makes dropping to
    // hex free.
    assert_eq!(
        decoded.payload.raw.map(|raw| raw.hex),
        Some("000000000106".to_owned())
    );
}

/// The property the whole "shared, not owned" design exists for.
#[tokio::test]
async fn two_clusters_sharing_a_registry_resolve_an_id_once_between_them() {
    let stub = Stub::serving(&avro_registry()).await;

    // One handle, two references — which is exactly what two clusters naming
    // the same `schema_registry: dev` get.
    let registry = stub.handle();
    let first_cluster = Arc::clone(&registry);
    let second_cluster = Arc::clone(&registry);

    let first = decode(
        Some(&first_cluster),
        &HEARTBEAT_RECORD,
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;
    assert_eq!(first.value, Some(serde_json::json!({ "beat": 3 })));

    let after_first = stub.requests();
    let ours_after_first = registry.requests();
    assert!(after_first > 0, "the first decode has to ask somebody");

    let second = decode(
        Some(&second_cluster),
        &HEARTBEAT_RECORD,
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;
    assert_eq!(second.value, Some(serde_json::json!({ "beat": 3 })));

    assert_eq!(
        stub.requests(),
        after_first,
        "the second cluster reached the registry: it has a cache of its own, which means it has \
         a *second* cache, which is the mistake `RegistryHandle` exists to make unrepresentable. \
         Paths asked for: {:?}",
        stub.paths()
    );
    assert_eq!(registry.requests(), ours_after_first);
}

#[tokio::test]
async fn a_url_pointing_at_the_native_api_is_a_configuration_error() {
    // Apicurio's native API is a real server answering real requests. It just
    // has no `/subjects`, and every Avro topic silently rendering as hex is
    // the failure this catches.
    let stub = Stub::serving(&[("/apis/registry/v3/system/info", 200, "{}")]).await;
    let registry = stub.handle();

    let decoded = decode(
        Some(&registry),
        &HEARTBEAT_RECORD,
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;

    let note = decoded.payload.note.expect("a note");
    assert_eq!(note.kind, NoteKind::RegistryMisconfigured);
    assert!(
        note.message.contains("ccompat"),
        "the error has to name the endpoint that was expected: {}",
        note.message
    );
    assert!(note.message.contains("dev"), "{}", note.message);
    // Not silently hex: the bytes are hex *and* the reason is on the record.
    assert_eq!(decoded.payload.text, "000000000106");
}

#[tokio::test]
async fn an_unreachable_registry_degrades_to_hex_on_one_backoff_schedule() {
    // A port nobody is listening on. Binding and dropping is the reliable way
    // to get one that is closed rather than merely unused.
    let addr = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    };
    let registry = Arc::new(
        RegistryHandle::new(RegistrySettings::new(
            "dev",
            format!("http://{addr}/apis/ccompat/v7"),
        ))
        .unwrap(),
    );

    let first = decode(
        Some(&registry),
        &HEARTBEAT_RECORD,
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;
    let note = first.payload.note.expect("a note");
    assert_eq!(note.kind, NoteKind::RegistryUnavailable);
    assert!(
        note.message.contains("dev") && note.message.contains(&addr.to_string()),
        "the note has to name the registry: {}",
        note.message
    );
    // The records are still there.
    assert_eq!(first.payload.text, "000000000106");

    let after_first = registry.requests();

    // Nine more clusters' worth of records. One schedule, so none of them
    // dials: ten clusters sharing `dev` must not be ten retry storms.
    for _ in 0..9 {
        let again = decode(
            Some(&registry),
            &HEARTBEAT_RECORD,
            Codec::Auto,
            MAX_PAYLOAD_CHARS,
        )
        .await;
        assert_eq!(
            again.payload.note.map(|n| n.kind),
            Some(NoteKind::RegistryUnavailable)
        );
    }
    assert_eq!(
        registry.requests(),
        after_first,
        "the backoff is per registry, not per record"
    );
}

#[tokio::test]
async fn a_record_that_parses_and_violates_its_subject_is_shown_as_non_conforming() {
    const SCHEMA: &str = r#"{"schemaType":"JSON","subject":"orders-value","version":1,"id":2,"schema":"{\"$id\":\"https://kaas.test/orders.json\",\"type\":\"object\",\"properties\":{\"amount\":{\"type\":\"number\"}},\"required\":[\"amount\"]}"}"#;
    let stub = Stub::serving(&[
        ("/subjects", 200, r#"["orders-value"]"#),
        ("/schemas/ids/2", 200, SCHEMA),
    ])
    .await;
    let registry = stub.handle();

    let framed = |body: &str| {
        let mut bytes = vec![0, 0, 0, 0, 2];
        bytes.extend_from_slice(body.as_bytes());
        bytes
    };

    let good = decode(
        Some(&registry),
        &framed(r#"{"amount": 12}"#),
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;
    assert_eq!(good.value, Some(serde_json::json!({ "amount": 12 })));
    assert!(good.payload.note.is_none(), "{:?}", good.payload.note);

    // Parses as JSON, is not what the subject says. Two different questions,
    // and answering only the first would render this as valid.
    let bad = decode(
        Some(&registry),
        &framed(r#"{"amount": "twelve"}"#),
        Codec::Auto,
        MAX_PAYLOAD_CHARS,
    )
    .await;
    assert_eq!(
        bad.payload.note.as_ref().map(|n| n.kind),
        Some(NoteKind::NonConforming),
        "{:?}",
        bad.payload.note
    );
    // Still decoded: a non-conforming record is shown, not withheld.
    assert_eq!(bad.value, Some(serde_json::json!({ "amount": "twelve" })));
}

#[tokio::test]
async fn a_schema_carrying_a_reference_to_another_subject_decodes() {
    // A resolver that fetches only the id in the payload decodes the simple
    // topics and fails on the interesting ones — and fails at decode time
    // rather than at configuration time.
    const MAIN: &str = r#"{"id":5,"subject":"avro-test","version":1,"schema":"{\"type\":\"record\",\"name\":\"AvroTest\",\"namespace\":\"org.schema_registry_test_app.avro\",\"fields\":[{\"name\":\"id\",\"type\":{\"type\":\"fixed\",\"name\":\"Uuid\",\"size\":16}},{\"name\":\"by\",\"type\":{\"type\":\"enum\",\"name\":\"Language\",\"symbols\":[\"Java\",\"Rust\",\"Js\",\"Python\",\"Go\",\"C\"]}},{\"name\":\"counter\",\"type\":\"long\"},{\"name\":\"input\",\"type\":[\"null\",\"string\"],\"default\":null},{\"name\":\"results\",\"type\":{\"type\":\"array\",\"items\":\"Result\"}}]}","references":[{"name":"org.schema_registry_test_app.avro.Result","subject":"avro-result","version":1}]}"#;
    const REFERENCED: &str = r#"{"subject":"avro-result","version":1,"id":2,"schema":"{\"type\":\"record\",\"name\":\"Result\",\"namespace\":\"org.schema_registry_test_app.avro\",\"fields\":[{\"name\":\"up\",\"type\":\"string\"},{\"name\":\"down\",\"type\":\"string\"}]}"}"#;

    let stub = Stub::serving(&[
        ("/subjects", 200, r#"["avro-test","avro-result"]"#),
        ("/schemas/ids/5", 200, MAIN),
        ("/subjects/avro-result/versions/1", 200, REFERENCED),
    ])
    .await;
    let registry = stub.handle();

    let bytes = [
        0, 0, 0, 0, 5, 97, 19, 76, 118, 247, 191, 70, 148, 162, 9, 233, 76, 211, 29, 141, 180, 0,
        2, 2, 12, 83, 116, 114, 105, 110, 103, 2, 12, 83, 84, 82, 73, 78, 71, 12, 115, 116, 114,
        105, 110, 103, 0,
    ];
    let decoded = decode(Some(&registry), &bytes, Codec::Auto, MAX_PAYLOAD_CHARS).await;

    assert!(decoded.payload.note.is_none(), "{:?}", decoded.payload.note);
    let value = decoded.value.expect("a decoded value");
    assert_eq!(value["counter"], serde_json::json!(1));
    assert_eq!(value["by"], serde_json::json!("Java"));
    // The field whose type lives in the other subject: `Result` is defined in
    // `avro-result`, and without fetching it the array item has no type at all.
    assert_eq!(value["results"][0]["up"], serde_json::json!("STRING"));
    assert_eq!(value["results"][0]["down"], serde_json::json!("string"));
    assert!(
        stub.paths()
            .contains(&"/subjects/avro-result/versions/1".to_owned()),
        "the reference was never fetched: {:?}",
        stub.paths()
    );
}

#[tokio::test]
async fn a_payload_that_is_not_valid_avro_is_a_payload_error() {
    let stub = Stub::serving(&avro_registry()).await;
    let registry = stub.handle();

    // Framed, the id resolves, and the body is not what the schema says.
    let bytes = [0, 0, 0, 0, 1, 0xff, 0xff];
    let decoded = decode(Some(&registry), &bytes, Codec::Auto, MAX_PAYLOAD_CHARS).await;

    let note = decoded.payload.note.expect("a note");
    assert_eq!(note.kind, NoteKind::DecodeError);
    assert!(note.message.contains("avro"), "{}", note.message);
    // The schema is still named: knowing *which* schema it failed against is
    // the whole diagnosis.
    assert_eq!(decoded.payload.schema.map(|s| s.id), Some(1));
    assert_eq!(decoded.value, None);
}

#[tokio::test]
async fn the_registry_decides_what_a_schema_id_is() {
    let stub = Stub::serving(&avro_registry()).await;
    let registry = stub.handle();

    // Someone picked Protobuf from the chip. The registry says id 1 is Avro,
    // and the registry is the one that knows.
    let decoded = decode(
        Some(&registry),
        &HEARTBEAT_RECORD,
        Codec::Protobuf,
        MAX_PAYLOAD_CHARS,
    )
    .await;

    assert_eq!(decoded.payload.codec, Codec::Avro);
    let note = decoded.payload.note.expect("a note");
    assert_eq!(note.kind, NoteKind::OverrideRefused);
    assert!(note.message.contains("avro"), "{}", note.message);
}

/// The browser's half: what a registered version declares, over real HTTP.
///
/// One subject per format and per strategy, because the pairing is the whole
/// question — the name has to come out of Avro JSON, a `.proto` text and a JSON
/// Schema title alike, and a subject that reveals no topic under one strategy
/// reveals one under another. `naming.rs` unit-tests the reading; this is the
/// fetch, the parse and the reading together, which is where a `schemaType`
/// mapped to the wrong parser would show up and nowhere else.
#[tokio::test]
async fn a_registered_version_carries_the_name_its_subject_was_built_from() {
    const ORDER: &str = r#"{"subject":"orders-com.acme.Order","version":1,"id":7,"schema":"{\"type\":\"record\",\"name\":\"Order\",\"namespace\":\"com.acme\",\"fields\":[]}"}"#;
    const READING: &str = r#"{"subject":"com.acme.Reading","version":1,"id":8,"schemaType":"PROTOBUF","schema":"syntax = \"proto3\";\npackage com.acme;\nmessage Reading { double celsius = 1; }\n"}"#;
    const AUDIT: &str = r#"{"subject":"audit-value","version":1,"id":9,"schemaType":"JSON","schema":"{\"title\":\"com.acme.Audit\",\"type\":\"object\"}"}"#;

    let stub = Stub::serving(&[
        ("/subjects/orders-com.acme.Order/versions/1", 200, ORDER),
        ("/subjects/com.acme.Reading/versions/1", 200, READING),
        ("/subjects/audit-value/versions/1", 200, AUDIT),
    ])
    .await;
    let registry = stub.handle();

    // Avro, `TopicRecordNameStrategy`. The seam is where the declared name
    // starts, and the topic is everything before it.
    let order = registry
        .schema("orders-com.acme.Order", 1)
        .await
        .expect("the registry answers");
    assert_eq!(order.format, SchemaFormat::Avro);
    assert_eq!(order.record_name.as_deref(), Some("com.acme.Order"));
    let naming = SubjectNaming::of(&order.subject, order.record_name.as_deref());
    assert_eq!(naming.strategy, NamingStrategy::TopicRecordName);
    assert_eq!(naming.topic.as_deref(), Some("orders"));

    // Protobuf, `RecordNameStrategy`. There is no topic in it, and that is the
    // answer rather than a failure to find one.
    let reading = registry
        .schema("com.acme.Reading", 1)
        .await
        .expect("the registry answers");
    assert_eq!(reading.format, SchemaFormat::Protobuf);
    assert_eq!(reading.record_name.as_deref(), Some("com.acme.Reading"));
    let naming = SubjectNaming::of(&reading.subject, reading.record_name.as_deref());
    assert_eq!(naming.strategy, NamingStrategy::RecordName);
    assert_eq!(naming.topic, None);

    // JSON Schema, `TopicNameStrategy`. A declared name does not make every
    // subject a record subject: this one still reads by its suffix, and the
    // name rides along because it is true of the schema either way.
    let audit = registry
        .schema("audit-value", 1)
        .await
        .expect("the registry answers");
    assert_eq!(audit.format, SchemaFormat::Json);
    assert_eq!(audit.record_name.as_deref(), Some("com.acme.Audit"));
    let naming = SubjectNaming::of(&audit.subject, audit.record_name.as_deref());
    assert_eq!(naming.strategy, NamingStrategy::TopicName);
    assert_eq!(naming.topic.as_deref(), Some("audit"));
}

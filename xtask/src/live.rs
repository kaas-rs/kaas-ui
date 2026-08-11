//! Phase acceptance, against the two live clusters.
//!
//! There is no Docker here and no `testcontainers`. What there is instead is
//! two shared, long-lived, three-node clusters running software we did not
//! choose — a harder and more honest target than one container, and the reason
//! every degradation path has a live fixture *and* a live absence.
//!
//! The run starts the real binary with the real configuration and talks to it
//! over HTTP. Nothing is mocked.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Task, root};

pub(crate) const PORT: u16 = 18099;

pub(crate) struct Acceptance {
    pub(crate) passed: usize,
    pub(crate) failures: Vec<String>,
}

impl Acceptance {
    pub(crate) fn check(&mut self, name: &str, outcome: Result<String, String>) {
        match outcome {
            Ok(detail) => {
                self.passed += 1;
                println!("  ok    {name}{}", suffix(&detail));
            }
            Err(reason) => {
                println!("  FAIL  {name}: {reason}");
                self.failures.push(format!("{name}: {reason}"));
            }
        }
    }
}

fn suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!("  ({detail})")
    }
}

/// A server started for the duration of the run, and stopped after it.
pub(crate) struct Server(Child);

impl Server {
    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Whether the process has actually gone.
    ///
    /// `try_wait` and not a `/proc/<pid>` check: this is a *child* process, so
    /// between exiting and being reaped it is a zombie — and a zombie still
    /// has a `/proc` entry. Polling the filesystem here reports a clean
    /// shutdown as a hang, which is exactly how this assertion first failed
    /// against a server that was shutting down correctly.
    fn exited(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Build the server and start it on [`PORT`] with `config`.
///
/// The real binary and the real configuration, as the module header says —
/// shared with the login run so the two cannot diverge in how they launch it.
///
/// # Errors
///
/// If the config file is missing, the build fails, or the process will not
/// spawn.
pub(crate) fn start(config: &str) -> Result<Server, String> {
    let config_path = root().join(config);
    if !config_path.exists() {
        return Err(format!("{} does not exist", config_path.display()));
    }

    crate::run("cargo", &["build", "-p", "kaas-ui-server"])?;

    println!("\n$ kaas-ui --config {config} (port {PORT})");
    let child = Command::new(root().join("target").join("debug").join("kaas-ui"))
        .arg("--config")
        .arg(&config_path)
        .env("KAAS_UI_SERVER__LISTEN", format!("127.0.0.1:{PORT}"))
        .env("RUST_LOG", "warn")
        .current_dir(root())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("could not start kaas-ui: {error}"))?;
    Ok(Server(child))
}

/// Poll `/health` until the process answers, or give up after ten seconds.
pub(crate) async fn wait_ready(client: &reqwest::Client) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if client.get(url("/health")).send().await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

pub fn run(args: &[String]) -> Task {
    let mut config = "config.dev.yaml".to_owned();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            config = iter.next().cloned().ok_or("--config needs a path")?;
        }
    }

    let mut server = start(&config)?;

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let mut outcome = runtime.block_on(assertions());

    // Last, because it stops the server. Kept out of `assertions()` for that
    // reason: everything above needs it running.
    if let Ok(acceptance) = &mut outcome {
        runtime.block_on(drains_with_streams_open(acceptance, &mut server));
    }

    match outcome {
        Ok(acceptance) if acceptance.failures.is_empty() => {
            println!("\nlive: {} assertions, all green", acceptance.passed);
            Ok(())
        }
        Ok(acceptance) => Err(format!(
            "{} of {} assertions failed:\n  {}",
            acceptance.failures.len(),
            acceptance.passed + acceptance.failures.len(),
            acceptance.failures.join("\n  ")
        )),
        Err(error) => Err(error),
    }
}

/// A shutdown with message streams open must not wait for them.
///
/// The regression this guards is silent and expensive: an SSE response is an
/// unbounded body, so a draining server waits on one that never completes.
/// Before the shutdown latch this hung until SIGKILL — in Kubernetes, the full
/// `terminationGracePeriodSeconds` on every rollout, with every open stream
/// severed and no `phase: done` to tell the client a deploy happened rather
/// than a network fault.
async fn drains_with_streams_open(acceptance: &mut Acceptance, server: &mut Server) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            acceptance.check("a shutdown drains open streams", Err(error.to_string()));
            return;
        }
    };

    // Held open for the duration: the responses are alive, so the connections
    // are, which is exactly a browser sitting on the message view.
    let mut held = Vec::new();
    for partition in 0..3 {
        let opened = client
            .get(url(&format!(
                "/api/environments/{ENV}/clusters/kaas/topics/kperf-bench/messages/stream?mode=live&partitions={partition}"
            )))
            .send()
            .await;
        if let Ok(response) = opened {
            held.push(response);
        }
    }
    if held.is_empty() {
        acceptance.check(
            "a shutdown drains open streams",
            Err("no stream could be opened to test with".to_owned()),
        );
        return;
    }

    let pid = server.pid();
    let started = Instant::now();
    let signalled = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !signalled {
        acceptance.check(
            "a shutdown drains open streams",
            Err(format!("could not signal pid {pid}")),
        );
        return;
    }

    let exited = loop {
        if server.exited() {
            break true;
        }
        if started.elapsed() > Duration::from_secs(10) {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let took = started.elapsed();

    acceptance.check(
        &format!("a shutdown drains {} open streams", held.len()),
        if exited && took < Duration::from_secs(5) {
            Ok(format!("{took:?}"))
        } else if exited {
            Err(format!("took {took:?}"))
        } else {
            Err("never exited; SIGKILL would be required".to_owned())
        },
    );
}

/// The environment the live fixtures are declared in.
///
/// Everything below the fleet is addressed `(environment, id)` — a cluster id
/// alone addresses nothing, because two environments may each hold a `kafka` —
/// so every path here names one. `config.dev.yaml` puts `kaas`, `strimzi` and
/// `dead` in `dev`; the `staging` and `prod` sections beside them point at
/// names that do not resolve and exist to be counted, not read.
const ENV: &str = "dev";

pub(crate) fn url(path: &str) -> String {
    format!("http://127.0.0.1:{PORT}{path}")
}

/// Percent-encode a query parameter value.
///
/// A payload filter is arbitrary text, and some of it — `&`, `=`, `+`, spaces
/// — is structure to a query string. Hand-rolled rather than adding a
/// dependency to the build tool for four lines.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            other => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

async fn get(client: &reqwest::Client, path: &str) -> Result<Value, String> {
    let response = client
        .get(url(path))
        .send()
        .await
        .map_err(|error| format!("GET {path}: {error}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("GET {path}: body was not JSON: {error}"))?;
    if !status.is_success() {
        return Err(format!("GET {path}: {status}: {body}"));
    }
    Ok(body)
}

async fn assertions() -> Result<Acceptance, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let mut acceptance = Acceptance {
        passed: 0,
        failures: Vec::new(),
    };

    // --- the process serves before it has connected to anything -------------
    let started = Instant::now();
    if !wait_ready(&client).await {
        return Err("the server never started listening".into());
    }
    let boot = started.elapsed();
    acceptance.check(
        "serving within 2s of start",
        if boot < Duration::from_secs(2) {
            Ok(format!("{boot:?}"))
        } else {
            Err(format!("took {boot:?}"))
        },
    );

    // /health must not consult a cluster, including the dead one.
    let health_started = Instant::now();
    let health = get(&client, "/health").await;
    let health_took = health_started.elapsed();
    acceptance.check(
        "/health answers without touching a cluster",
        match health {
            Ok(body) if body["status"] == "ok" && health_took < Duration::from_millis(100) => {
                Ok(format!("{health_took:?}"))
            }
            Ok(_) => Err(format!("took {health_took:?}")),
            Err(error) => Err(error),
        },
    );

    // Give the two live clusters a moment to connect in the background. The
    // dead one will still be failing, which is the point.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // --- the fleet ----------------------------------------------------------
    //
    // `/api/environments` is the entry point and the only route that does not
    // name one: it is what tells a caller which environments there are. Every
    // cluster in the file is reached beneath one.
    let fleet_started = Instant::now();
    let fleet = get(&client, "/api/environments").await?;
    let fleet_took = fleet_started.elapsed();
    let sections = fleet["items"].as_array().cloned().unwrap_or_default();

    acceptance.check(
        "GET /api/environments returns one section per configured environment",
        if sections.len() >= 2 {
            Ok(sections
                .iter()
                .map(|section| {
                    format!(
                        "{}={}",
                        section["id"].as_str().unwrap_or("?"),
                        section["clusters"].as_array().map_or(0, Vec::len)
                    )
                })
                .collect::<Vec<_>>()
                .join(" "))
        } else {
            Err(format!("got {} sections", sections.len()))
        },
    );

    let cards = get(&client, &format!("/api/environments/{ENV}/clusters")).await?["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    acceptance.check(
        &format!("GET /api/environments/{ENV}/clusters returns the clusters declared in {ENV}"),
        if cards.len() >= 2 {
            Ok(format!("{} clusters", cards.len()))
        } else {
            Err(format!("got {}", cards.len()))
        },
    );

    // The listing under an environment is *that* environment's, which is the
    // whole reason the id sits in the path. A cluster id is unique only within
    // its environment, so a listing that reaches past it hands the client two
    // clusters with one name and no way to tell them apart — and every `find`
    // by id in the frontend then picks whichever sorted first.
    acceptance.check(
        "a cluster listing does not reach past the environment that names it",
        match cards
            .iter()
            .filter_map(|card| card["environment"].as_str())
            .find(|environment| *environment != ENV)
        {
            None => Ok(format!("all {} in {ENV}", cards.len())),
            Some(other) => Err(format!("a cluster from {other:?} is listed under {ENV}")),
        },
    );

    // The dead cluster costs a timeout on a background task, never on this
    // request. If lazy connection ever regresses, this is where it shows.
    acceptance.check(
        "an unreachable cluster does not slow the fleet request",
        if fleet_took < Duration::from_millis(250) {
            Ok(format!("{fleet_took:?}"))
        } else {
            Err(format!("took {fleet_took:?}"))
        },
    );

    let live: Vec<&Value> = cards
        .iter()
        .filter(|card| card["status"] == "ready")
        .collect();
    let dead: Vec<&Value> = cards
        .iter()
        .filter(|card| card["status"] == "unreachable")
        .collect();

    acceptance.check(
        "reachable clusters report brokers and topics",
        if live.len() >= 2
            && live
                .iter()
                .all(|card| card["brokerCount"].as_u64().unwrap_or(0) > 0)
        {
            Ok(live
                .iter()
                .map(|card| {
                    format!(
                        "{}={}b/{}t",
                        card["id"].as_str().unwrap_or("?"),
                        card["brokerCount"],
                        card["topicCount"]
                    )
                })
                .collect::<Vec<_>>()
                .join(" "))
        } else {
            Err(format!("{} ready clusters", live.len()))
        },
    );

    if let Some(card) = dead.first() {
        acceptance.check(
            "an unreachable cluster carries its transport error",
            if card["error"].as_str().is_some_and(|e| !e.is_empty()) {
                Ok(card["id"].as_str().unwrap_or("?").to_owned())
            } else {
                Err("no error attached".into())
            },
        );
    }

    let ids: Vec<String> = live
        .iter()
        .filter_map(|card| card["id"].as_str().map(str::to_owned))
        .collect();

    // --- capabilities: the conformance report -------------------------------
    let mut projections = Vec::new();
    for id in &ids {
        let capabilities = get(
            &client,
            &format!("/api/environments/{ENV}/clusters/{id}/capabilities"),
        )
        .await?;
        let available: Vec<String> = capabilities["features"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|feature| feature["state"] == "available")
            .filter_map(|feature| feature["feature"].as_str().map(str::to_owned))
            .collect();

        acceptance.check(
            &format!("{id}: the version table names the broker it came from"),
            if capabilities["source"]["kind"] == "broker" {
                Ok(format!("broker {}", capabilities["source"]["nodeId"]))
            } else {
                Err("no source".into())
            },
        );

        acceptance.check(
            &format!("{id}: acls are available"),
            if available.iter().any(|feature| feature == "acls") {
                Ok(String::new())
            } else {
                Err("DescribeAcls did not project".into())
            },
        );

        projections.push((id.clone(), available));
    }

    if projections.len() >= 2 {
        let (first, second) = (&projections[0], &projections[1]);
        acceptance.check(
            "the two clusters project different feature sets",
            if first.1 != second.1 {
                Ok(format!(
                    "{}={} features, {}={}",
                    first.0,
                    first.1.len(),
                    second.0,
                    second.1.len()
                ))
            } else {
                Err("identical: the conformance report says nothing".into())
            },
        );
    }

    // --- topics -------------------------------------------------------------
    for id in &ids {
        let topics = get(
            &client,
            &format!("/api/environments/{ENV}/clusters/{id}/topics"),
        )
        .await?;
        let names: Vec<String> = topics["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|topic| topic["name"].as_str().map(str::to_owned))
            .collect();

        acceptance.check(
            &format!("{id}: the topic list is not empty"),
            if names.is_empty() {
                Err("no topics".into())
            } else {
                Ok(format!("{} topics", names.len()))
            },
        );

        // Server-side filtering: the response changes, not just the render.
        if let Some(sample) = names.first() {
            let filtered = get(
                &client,
                &format!("/api/environments/{ENV}/clusters/{id}/topics?search={sample}"),
            )
            .await?;
            let filtered_count = filtered["items"].as_array().map_or(0, Vec::len);
            acceptance.check(
                &format!("{id}: filtering happens on the server"),
                if filtered_count < names.len() || names.len() == 1 {
                    Ok(format!("{filtered_count} of {}", names.len()))
                } else {
                    Err("the filter changed nothing".into())
                },
            );
        }

        // The headline test: describing topics of which two do not exist is a
        // result, not a failure.
        let mut asked = names.clone();
        asked.push("kaas-ui-does-not-exist-a".into());
        asked.push("kaas-ui-does-not-exist-b".into());
        let partial = get(
            &client,
            &format!(
                "/api/environments/{ENV}/clusters/{id}/topics?name={}",
                asked.join(",")
            ),
        )
        .await?;
        let items = partial["items"].as_array().map_or(0, Vec::len);
        let errors = partial["errors"].as_array().map_or(0, Vec::len);
        acceptance.check(
            &format!(
                "{id}: {} topics of which 2 do not exist is 200 with both",
                asked.len()
            ),
            if items == names.len() && errors == 2 {
                Ok(format!("{items} items, {errors} errors"))
            } else {
                Err(format!(
                    "{items} items and {errors} errors, expected {} and 2",
                    names.len()
                ))
            },
        );

        // Topic detail, and the tail, on the shared fixture.
        if names.iter().any(|name| name == "kperf-bench") {
            let detail = get(
                &client,
                &format!("/api/environments/{ENV}/clusters/{id}/topics/kperf-bench"),
            )
            .await?;
            let partitions = detail["items"][0]["partitions"]
                .as_array()
                .map_or(0, Vec::len);
            acceptance.check(
                &format!("{id}: kperf-bench describes its partitions"),
                if partitions > 0 {
                    Ok(format!("{partitions} partitions"))
                } else {
                    Err("no partitions".into())
                },
            );

            let offsets_present = detail["items"][0]["partitions"][0]["latestOffset"].is_i64();
            acceptance.check(
                &format!("{id}: partitions carry an offset range"),
                if offsets_present {
                    Ok(String::new())
                } else {
                    Err("no latestOffset".into())
                },
            );

            let tail = get(
                &client,
                &format!("/api/environments/{ENV}/clusters/{id}/topics/kperf-bench/messages/tail?limit=20"),
            )
            .await?;
            let records = tail["items"].as_array().map_or(0, Vec::len);
            // An emptied fixture is a precondition, not a failure — see the
            // seek-mode section below. But whether the fixture is empty is
            // decided by the offsets endpoint, never by the response under
            // test: a tail that comes back empty on a topic that *holds*
            // records is exactly the regression this check exists to catch,
            // and a skip keyed on the asserted response could never fail.
            let bounds = get(
                &client,
                &format!("/api/environments/{ENV}/clusters/{id}/topics/kperf-bench/offsets"),
            )
            .await?;
            let held: i64 = bounds["items"].as_array().map_or(0, |list| {
                list.iter()
                    .map(|p| {
                        p["latestOffset"].as_i64().unwrap_or(0)
                            - p["earliestOffset"].as_i64().unwrap_or(0)
                    })
                    .filter(|len| *len > 0)
                    .sum()
            });
            if held > 0 {
                acceptance.check(
                    &format!("{id}: the tail of a large topic comes back"),
                    if records > 0 {
                        Ok(format!("{records} records, {} fetched", tail["total"]))
                    } else {
                        Err(format!("0 records from a topic holding {held}"))
                    },
                );
            } else {
                println!("  skip  {id}: kperf-bench is empty; retention has moved past it");
            }
        }
    }

    // --- messages: the seven seek modes -------------------------------------
    //
    // These name concrete offsets — 1000000 and 1000037 — because the two
    // properties they check are off-by-one properties, and an off-by-one on a
    // "somewhere near the end" offset is invisible. The cost is that the
    // fixture ages: `kperf-bench` was written once and retention eventually
    // takes it. A topic that no longer holds those offsets is an **unmet
    // precondition**, not a regression, and reporting it as a failure would
    // train everyone to ignore this run.
    for id in &ids {
        let topics = get(
            &client,
            &format!("/api/environments/{ENV}/clusters/{id}/topics"),
        )
        .await?;
        let has_bench = topics["items"]
            .as_array()
            .is_some_and(|list| list.iter().any(|topic| topic["name"] == "kperf-bench"));
        if !has_bench {
            continue;
        }
        let base = format!("/api/environments/{ENV}/clusters/{id}/topics/kperf-bench");

        let bounds = get(&client, &format!("{base}/offsets")).await?;
        let partition_zero = bounds["items"]
            .as_array()
            .and_then(|list| list.iter().find(|p| p["partition"] == 0).cloned());
        let retains = partition_zero.as_ref().is_some_and(|p| {
            p["earliestOffset"].as_i64().unwrap_or(i64::MAX) <= 1_000_000
                && p["latestOffset"].as_i64().unwrap_or(0) > 1_000_037
        });
        if !retains {
            println!(
                "  skip  {id}: the seek-mode fixture has aged out of kperf-bench-0 \
                 (retains {}–{}); write it again to re-arm these",
                partition_zero
                    .as_ref()
                    .map_or(Value::Null, |p| p["earliestOffset"].clone()),
                partition_zero.map_or(Value::Null, |p| p["latestOffset"].clone()),
            );
            continue;
        }

        // `toOffset` is the assertion the kaas-lib anchor change exists for,
        // and the one an off-by-one hides in: a window that stops one short
        // still looks entirely plausible.
        let anchored = get(
            &client,
            &format!("{base}/messages?mode=toOffset&offset=1000000&limit=5&partitions=0"),
        )
        .await?;
        let offsets: Vec<i64> = anchored["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|row| row["offset"].as_i64())
            .collect();
        acceptance.check(
            &format!("{id}: toOffset includes the anchor and nothing above it"),
            if offsets.first() == Some(&1_000_000)
                && offsets.iter().all(|offset| *offset <= 1_000_000)
            {
                Ok(format!("{offsets:?}"))
            } else {
                Err(format!("got {offsets:?}"))
            },
        );

        // The other half of the same property: a forward read must begin at
        // the offset asked for, not at the base of the batch containing it.
        let forward = get(
            &client,
            &format!("{base}/messages?mode=fromOffset&offset=1000037&limit=5&partitions=0"),
        )
        .await?;
        let first = forward["items"][0]["offset"].as_i64();
        acceptance.check(
            &format!("{id}: fromOffset starts at the offset, not its batch"),
            if first == Some(1_000_037) {
                Ok(format!("{first:?}"))
            } else {
                Err(format!("started at {first:?}, expected 1000037"))
            },
        );

        // One record, fetched the way the detail panel fetches it.
        let one = get(&client, &format!("{base}/messages/0/1000037")).await?;
        acceptance.check(
            &format!("{id}: a single message is fetched by partition and offset"),
            if one["offset"].as_i64() == Some(1_000_037) && one["kind"] == "record" {
                Ok(format!("{} bytes", one["value"]["bytes"]))
            } else {
                Err(format!("got {one}"))
            },
        );

        let missing = client
            .get(url(&format!("{base}/messages/0/99999999999")))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            &format!("{id}: an offset past the end is 404, not the last record"),
            if missing.status() == reqwest::StatusCode::NOT_FOUND {
                Ok(String::new())
            } else {
                Err(format!("got {}", missing.status()))
            },
        );

        // A time seek is answered or it is not, and either way the answer is
        // reported rather than interpreted. `kaas` holds no timestamp index
        // and resolves to nothing; Strimzi resolves precisely. Both are
        // acceptable — silently showing the wrong window is not.
        let timestamp = forward["items"][0]["timestamp"].as_i64().unwrap_or(0);
        let seeked = get(
            &client,
            &format!("{base}/messages?mode=sinceTime&timestamp={timestamp}&limit=3&partitions=0"),
        )
        .await?;
        let unresolved = seeked["resolved"]["unresolved"].as_bool();
        let rows = seeked["items"].as_array().map_or(0, Vec::len);
        acceptance.check(
            &format!("{id}: a time seek reports what it resolved to"),
            match unresolved {
                Some(true) if rows == 0 => Ok("resolved to nothing, and says so".to_owned()),
                Some(false) if rows > 0 => Ok(format!("{rows} rows")),
                Some(true) => Err(format!("{rows} rows from an unresolved seek")),
                Some(false) => Err("resolved, but returned nothing".to_owned()),
                None => Err("no resolved block on a time mode".to_owned()),
            },
        );

        // The stream, over SSE. A backward window has no partial results, so
        // it must announce `seeking` before it announces anything else.
        let stream = client
            .get(url(&format!(
                "{base}/messages/stream?mode=newest&limit=4&partitions=0"
            )))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let content_type = stream
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let compressed = stream.headers().contains_key("content-encoding");
        let body = stream.text().await.map_err(|e| e.to_string())?;

        acceptance.check(
            &format!("{id}: the stream is an uncompressed event stream"),
            if content_type.starts_with("text/event-stream") && !compressed {
                Ok(content_type.clone())
            } else {
                // Compressing SSE holds every event in a buffer until it
                // fills, which reads as a stream that works but lags by
                // minutes.
                Err(format!("{content_type}, compressed={compressed}"))
            },
        );

        let phases: Vec<&str> = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: {\"phase\":\""))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        acceptance.check(
            &format!("{id}: a backward window seeks, streams, then ends"),
            if phases == ["seeking", "streaming", "done"] {
                Ok(phases.join(" → "))
            } else {
                Err(format!("{phases:?}"))
            },
        );

        acceptance.check(
            &format!("{id}: every messages event carries its last row's id"),
            if body
                .lines()
                .filter(|line| line.starts_with("id: "))
                .all(|line| line.trim_start_matches("id: ").contains('-'))
                && body.contains("id: 0-")
            {
                Ok(String::new())
            } else {
                Err("no {partition}-{offset} id on the batch".to_owned())
            },
        );
    }

    // --- analysis: the statistics tab's full-topic scan ----------------------
    //
    // `kaas-canary-v1` exists on both clusters and scans in seconds. The
    // assertions are internal-consistency properties rather than absolute
    // counts, because the canary producer is writing while this runs. The
    // histogram check is the acceptance criterion for `kaas` specifically:
    // that broker holds no timestamp index, but the histogram reads
    // timestamps off records rather than seeking by time, so the degradation
    // that breaks `sinceTime` there must not touch this view.
    for id in &ids {
        let base = format!("/api/environments/{ENV}/clusters/{id}/topics/kaas-canary-v1");
        let response = client
            .get(url(&format!("{base}/analysis")))
            .timeout(Duration::from_secs(180))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            acceptance.check(
                &format!("{id}: an analysis opens"),
                Err(format!("got {}", response.status())),
            );
            continue;
        }
        let body = response.text().await.map_err(|e| e.to_string())?;

        let mut result: Option<Value> = None;
        let mut fractions: Vec<f64> = Vec::new();
        let mut lines = body.lines().peekable();
        while let Some(line) = lines.next() {
            let Some(event) = line.strip_prefix("event: ") else {
                continue;
            };
            let Some(data) = lines.next().and_then(|next| next.strip_prefix("data: ")) else {
                continue;
            };
            match event {
                "result" => result = serde_json::from_str(data).ok(),
                "progress" => {
                    if let Ok(frame) = serde_json::from_str::<Value>(data)
                        && let Some(fraction) = frame["fraction"].as_f64()
                    {
                        fractions.push(fraction);
                    }
                }
                _ => {}
            }
        }

        let Some(result) = result else {
            acceptance.check(
                &format!("{id}: an analysis ends in a result"),
                Err("the stream closed with no result event".to_owned()),
            );
            continue;
        };

        // On a non-compacted topic every partition's count is exactly its
        // scanned offset span — the property `livetest probe` is the oracle
        // for, checked here through the analysis's own numbers.
        let consistent = result["partitionStats"]
            .as_array()
            .is_some_and(|partitions| {
                !partitions.is_empty()
                    && partitions.iter().all(|p| {
                        match (
                            p["totalMsgs"].as_i64(),
                            p["minOffset"].as_i64(),
                            p["maxOffset"].as_i64(),
                        ) {
                            (Some(msgs), Some(low), Some(high)) => msgs == high - low + 1,
                            _ => false,
                        }
                    })
            });
        acceptance.check(
            &format!("{id}: an analysis completes and its counts match its offsets"),
            if result["complete"].as_bool() == Some(true) && consistent {
                Ok(format!("{} records", result["totalStats"]["totalMsgs"]))
            } else {
                Err(format!(
                    "complete={}, counts-match-offsets={consistent}",
                    result["complete"]
                ))
            },
        );

        let hours = result["totalStats"]["hourlyMsgCounts"]
            .as_array()
            .map_or(0, Vec::len);
        acceptance.check(
            &format!("{id}: the hourly histogram is populated"),
            if hours > 0 {
                Ok(format!("{hours} hour(s)"))
            } else {
                Err("no hourly buckets — timestamps were not read off records".to_owned())
            },
        );

        acceptance.check(
            &format!("{id}: analysis progress is monotonic and capped at 1"),
            if fractions.windows(2).all(|pair| pair[0] <= pair[1])
                && fractions.iter().all(|fraction| *fraction <= 1.0)
            {
                Ok(format!("{} frame(s)", fractions.len()))
            } else {
                Err(format!("{fractions:?}"))
            },
        );

        // The record cap: exactly the sample asked for, and the result names
        // the cap — not an error, and not the topic's numbers.
        let capped = client
            .get(url(&format!("{base}/analysis?limit=100")))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let capped_result: Option<Value> = capped
            .lines()
            .zip(capped.lines().skip(1))
            .find(|(line, _)| *line == "event: result")
            .and_then(|(_, data)| data.strip_prefix("data: "))
            .and_then(|data| serde_json::from_str(data).ok());
        acceptance.check(
            &format!("{id}: a capped analysis is the sample it asked for"),
            match capped_result {
                Some(result)
                    if result["totalStats"]["totalMsgs"] == 100
                        && result["stoppedBy"] == "messageCap"
                        && result["complete"] == false
                        && result["errors"].as_array().is_some_and(Vec::is_empty) =>
                {
                    Ok("100 records, stoppedBy=messageCap, no error".to_owned())
                }
                Some(result) => Err(format!(
                    "totalMsgs={}, stoppedBy={}, complete={}",
                    result["totalStats"]["totalMsgs"], result["stoppedBy"], result["complete"]
                )),
                None => Err("no result event".to_owned()),
            },
        );
    }

    // --- one analysis per cluster --------------------------------------------
    //
    // The ceiling is about everyone else's latency, not memory: an analysis
    // fetches continuously on the shared per-broker connection, so a second
    // one would queue behind the first — see upstream ask 11. Holding an open
    // response on kperf-bench (146M records: it will not finish under this
    // test) and asking for a second must refuse with 429, and dropping the
    // first must free the slot, because dropping the response is the only
    // cancellation there is.
    {
        let target = ids.first().cloned().unwrap_or_default();
        let held = client
            .get(url(&format!(
                "/api/environments/{ENV}/clusters/{target}/topics/kperf-bench/analysis"
            )))
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let second = client
            .get(url(&format!(
                "/api/environments/{ENV}/clusters/{target}/topics/kaas-canary-v1/analysis"
            )))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            "a second analysis on a busy cluster is refused with 429",
            if second.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                Ok(String::new())
            } else {
                Err(format!("got {}", second.status()))
            },
        );
        drop(second);
        drop(held);

        // The slot frees when the reader goes away — the same release the
        // stream governor's permit has, asserted the same way: by using it.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let after = client
            .get(url(&format!(
                "/api/environments/{ENV}/clusters/{target}/topics/kaas-canary-v1/analysis"
            )))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            "dropping an analysis response frees the cluster's slot",
            if after.status().is_success() {
                Ok(String::new())
            } else {
                Err(format!("got {}", after.status()))
            },
        );
    }

    // --- streams are bounded, and the bound releases -------------------------
    {
        let target = ids.first().cloned().unwrap_or_default();
        let stream_url = url(&format!(
            "/api/environments/{ENV}/clusters/{target}/topics/kperf-bench/messages/stream?mode=live"
        ));

        // Five is the per-caller ceiling, and it applies only to a caller a
        // forwarded header actually names. This request arrives straight at
        // the binary, so without the header every stream here would share the
        // peer address — the case that must *not* be rationed, because behind
        // a proxy that key is the proxy rather than a person.
        let mut held = Vec::new();
        for _ in 0..5 {
            if let Ok(response) = client
                .get(&stream_url)
                .header("x-forwarded-for", "203.0.113.7")
                .timeout(Duration::from_secs(60))
                .send()
                .await
            {
                held.push(response);
            }
        }

        // A sixth now *evicts* rather than refuses. Refusing only frees a
        // slot when the server notices a reader has gone, and behind this
        // deployment's two proxies — a Cloudflare tunnel into code-server —
        // it never does: the upstream connection outlives the browser, so an
        // abandoned stream held its slot for the full lifetime cap and five
        // reloads locked a person out of their own tool.
        let sixth = client
            .get(&stream_url)
            .header("x-forwarded-for", "203.0.113.7")
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            "a sixth stream from one named caller is served, not refused",
            if sixth.status().is_success() {
                Ok(String::new())
            } else {
                Err(format!("got {}", sixth.status()))
            },
        );

        // And the caller is still bounded: the oldest was closed to make room.
        let oldest = held.remove(0);
        let ended = tokio::time::timeout(Duration::from_secs(5), oldest.text()).await;
        acceptance.check(
            "and their oldest is closed to make room",
            match ended {
                Ok(Ok(body)) if body.contains("\"phase\":\"done\"") => {
                    Ok("evicted with phase: done".to_owned())
                }
                Ok(Ok(_)) => Err("ended without telling the client why".to_owned()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("the evicted stream never ended".to_owned()),
            },
        );

        // A different caller is untouched, which is the point of counting per
        // caller rather than only in total.
        let other = client
            .get(&stream_url)
            .header("x-forwarded-for", "203.0.113.8")
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            "a different named caller is unaffected",
            if other.status().is_success() {
                Ok(String::new())
            } else {
                Err(format!("got {}", other.status()))
            },
        );

        // And a caller nothing identifies — every browser behind code-server's
        // proxy — must be neither capped nor evicted.
        let mut anonymous = Vec::new();
        for _ in 0..8 {
            if let Ok(response) = client
                .get(&stream_url)
                .timeout(Duration::from_secs(30))
                .send()
                .await
            {
                anonymous.push(response.status());
            }
        }
        acceptance.check(
            "callers a proxy makes indistinguishable are not capped at 5",
            if anonymous.len() == 8 && anonymous.iter().all(reqwest::StatusCode::is_success) {
                Ok(format!("{} opened", anonymous.len()))
            } else {
                Err(format!("statuses {anonymous:?}"))
            },
        );
        drop(anonymous);
        drop(other);
        drop(sixth);

        // Dropping the responses is exactly what a closed browser tab does.
        // The permit releases on drop and nowhere else, so this also proves
        // the scan behind each one was dropped with it.
        drop(held);
        tokio::time::sleep(Duration::from_secs(2)).await;

        let after = client
            .get(&stream_url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        acceptance.check(
            "abandoning a stream releases its slot within 5s",
            if after.status().is_success() {
                Ok(String::new())
            } else {
                Err(format!("still refused: {}", after.status()))
            },
        );
    }

    // --- groups -------------------------------------------------------------
    for id in &ids {
        let groups = get(
            &client,
            &format!("/api/environments/{ENV}/clusters/{id}/groups"),
        )
        .await?;
        let listed = groups["items"].as_array().cloned().unwrap_or_default();
        acceptance.check(
            &format!("{id}: groups list"),
            Ok(format!("{} groups", listed.len())),
        );

        if let Some(group) = listed.first().and_then(|g| g["groupId"].as_str()) {
            let offsets = get(
                &client,
                &format!("/api/environments/{ENV}/clusters/{id}/groups/{group}/offsets"),
            )
            .await?;
            let rows = offsets["items"].as_array().cloned().unwrap_or_default();
            let states: Vec<&str> = rows
                .iter()
                .filter_map(|row| row["lag"]["state"].as_str())
                .collect();
            acceptance.check(
                &format!("{id}: lag is classified, not subtracted"),
                if states.iter().all(|state| {
                    matches!(
                        *state,
                        "noCommit" | "emptyPartition" | "caughtUp" | "lagging" | "unknown"
                    )
                }) {
                    Ok(format!("{} rows", rows.len()))
                } else {
                    Err(format!("unexpected lag states: {states:?}"))
                },
            );
        }
    }

    // --- schemas and payload decoding ---------------------------------------
    //
    // One registry, referenced by both live clusters, with a canary producing
    // Confluent-framed Avro to `kaas-canary-v1` on each. That is the whole
    // fixture: the same schema id means the same schema on both sides, because
    // it is the same registry answering.
    const CANARY: &str = "kaas-canary-v1";

    let mut decoded_on = Vec::new();
    for cluster in ["strimzi", "kaas"] {
        let tail = get(
            &client,
            &format!(
                "/api/environments/{ENV}/clusters/{cluster}/topics/{CANARY}/messages/tail?limit=1"
            ),
        )
        .await;
        let value = tail
            .ok()
            .and_then(|body| body["items"].get(0).cloned())
            .map(|item| item["value"].clone());

        acceptance.check(
            &format!("an Avro topic on {cluster} decodes with its schema id resolved"),
            match &value {
                Some(value)
                    if value["codec"] == "avro"
                        && value["schema"]["id"].as_u64().is_some()
                        && value["note"].is_null() =>
                {
                    decoded_on.push((cluster, value.clone()));
                    Ok(format!(
                        "schema {} ({}) from registry {}",
                        value["schema"]["id"],
                        value["schema"]["subject"],
                        value["schema"]["registry"]
                    ))
                }
                Some(value) => Err(format!("{value}")),
                None => Err("no records on the canary topic".to_owned()),
            },
        );
    }

    // The same id, resolved by the one shared client. Two clusters answering
    // with different subjects for one id would mean two caches, which is the
    // mistake `RegistryHandle` exists to make unrepresentable.
    acceptance.check(
        "two clusters resolve the same schema id to the same schema",
        match decoded_on.as_slice() {
            [(_, first), (_, second)]
                if first["schema"]["id"] == second["schema"]["id"]
                    && first["schema"]["subject"] == second["schema"]["subject"]
                    && first["schema"]["registry"] == second["schema"]["registry"] =>
            {
                Ok(format!(
                    "id {} → {}",
                    first["schema"]["id"], first["schema"]["subject"]
                ))
            }
            [(_, first), (_, second)] => Err(format!("{first} vs {second}")),
            _ => Err("one of the two clusters did not decode".to_owned()),
        },
    );

    // Falling back is free and needs no schema: the raw bytes travelled beside
    // the decoded value, so this is the same record read differently rather
    // than a second read of it.
    let overridden = get(
        &client,
        &format!("/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages/tail?limit=1&valueCodec=hex"),
    )
    .await;
    acceptance.check(
        "the same topic renders as hex when the codec is overridden",
        match overridden.map(|body| body["items"][0]["value"].clone()) {
            Ok(value)
                if value["codec"] == "hex"
                    && value["note"].is_null()
                    && value["text"].as_str().is_some_and(|text| {
                        // Framed, so the rendering starts with the magic byte
                        // and the four-byte id it carried.
                        text.starts_with("00000000")
                    }) =>
            {
                Ok(String::new())
            }
            Ok(value) => Err(format!("{value}")),
            Err(error) => Err(error),
        },
    );

    // A header is an ordinary unframed payload on a registry-backed record.
    // Absence of framing is not a failure, and the two paths do not compete
    // for the same bytes even inside one record.
    let headers = get(
        &client,
        &format!("/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages/tail?limit=1"),
    )
    .await
    .map(|body| body["items"][0]["headers"].clone());
    acceptance.check(
        "an unframed payload renders without a decode-error",
        match headers {
            Ok(Value::Array(headers)) if !headers.is_empty() => {
                let clean = headers.iter().all(|header| {
                    header["value"]["note"].is_null() && header["value"]["codec"] == "auto"
                });
                if clean {
                    Ok(format!("{} header(s)", headers.len()))
                } else {
                    Err(format!("{headers:?}"))
                }
            }
            Ok(other) => Err(format!("no headers to check: {other}")),
            Err(error) => Err(error),
        },
    );

    // The browser hangs off the *registry*, not off a cluster: it used to be
    // `/api/clusters/{id}/schemas`, which made "the same subjects from either
    // cluster" a claim about two responses. It is now one response, and the
    // claim it replaces is the stronger one — both clusters name the same
    // registry id, so there is only one thing to ask.
    let referenced: Vec<(String, String)> = cards
        .iter()
        .filter_map(|card| {
            Some((
                card["id"].as_str()?.to_owned(),
                card["schemaRegistry"].as_str()?.to_owned(),
            ))
        })
        .collect();
    acceptance.check(
        "both live clusters reference one registry, so there is one cache to ask",
        match referenced.as_slice() {
            [] => Err("no cluster references a registry".to_owned()),
            [(_, first), rest @ ..] if rest.iter().all(|(_, id)| id == first) => Ok(referenced
                .iter()
                .map(|(cluster, id)| format!("{cluster}→{id}"))
                .collect::<Vec<_>>()
                .join(" ")),
            _ => Err(format!("{referenced:?}")),
        },
    );

    // A cluster that references none is a normal path, not a degraded one, and
    // the card is where that shows now that there is no per-cluster route.
    acceptance.check(
        "a cluster referencing no registry says so on its card",
        match cards
            .iter()
            .find(|card| card["schemaRegistry"].is_null())
            .and_then(|card| card["id"].as_str())
        {
            Some(id) => Ok(id.to_owned()),
            None => Err("every cluster references a registry; nothing tests absence".to_owned()),
        },
    );

    let registry_id = referenced
        .first()
        .map(|(_, id)| id.clone())
        .unwrap_or_default();
    let subjects = get(
        &client,
        &format!("/api/environments/{ENV}/schema-registries/{registry_id}/subjects?details=true"),
    )
    .await;
    let rows = subjects
        .as_ref()
        .ok()
        .and_then(|body| body["subjects"].as_array().cloned())
        .unwrap_or_default();
    acceptance.check(
        "the registry lists its subjects",
        match &subjects {
            Ok(body) if !rows.is_empty() && body["registry"]["status"] == "ready" => Ok(format!(
                "{} subject(s) from registry {}",
                rows.len(),
                body["registry"]["id"]
            )),
            Ok(body) => Err(format!("{body}")),
            Err(error) => Err(error.clone()),
        },
    );

    // The column the topic page reads. A subject that names a topic says which
    // topic, from the server, because no prefix match can: `orders-` would
    // claim `orders-eu-value`, and under `TopicRecordNameStrategy` the seam
    // between topic and record is in the schema rather than in the name.
    acceptance.check(
        "a subject naming a topic says which topic, and it is the whole name",
        match rows
            .iter()
            .find(|row| row["naming"]["topic"].as_str() == Some(CANARY))
        {
            Some(row) => Ok(format!(
                "{} → {} ({})",
                row["subject"], row["naming"]["topic"], row["naming"]["strategy"]
            )),
            None => Err(format!(
                "no subject names {CANARY}: {:?}",
                rows.iter()
                    .map(|row| row["subject"].clone())
                    .collect::<Vec<_>>()
            )),
        },
    );

    // What the fleet card asks for: the counts, and not one row of the thing
    // being counted. It is the whole reason `topics` is computed on the server
    // — a card that had to download every subject name to count them would put
    // the size of the biggest registry on the fleet page.
    let summary = get(
        &client,
        &format!("/api/environments/{ENV}/schema-registries/{registry_id}/subjects?limit=0"),
    )
    .await;
    // `topicName` only, which is the summary's own definition: it reads the
    // names and never a schema, so a `{topic}-{record}` subject carries no
    // topic there however the detailed listing beside it resolved the seam.
    let named: Vec<&str> = rows
        .iter()
        .filter(|row| row["naming"]["strategy"] == "topicName")
        .filter_map(|row| row["naming"]["topic"].as_str())
        .collect();
    acceptance.check(
        "the registry summarises itself without sending a subject",
        match &summary {
            Ok(body) => {
                let distinct = named.iter().collect::<std::collections::BTreeSet<_>>();
                let sent = body["subjects"].as_array().map_or(usize::MAX, Vec::len);
                let total = body["total"].as_u64() == Some(rows.len() as u64);
                let topics = body["topics"].as_u64() == Some(distinct.len() as u64);
                if sent == 0 && total && topics {
                    Ok(format!(
                        "{} subject(s) over {} topic(s), 0 rows sent",
                        body["total"], body["topics"]
                    ))
                } else {
                    Err(format!("{body} against {} listed subject(s)", rows.len()))
                }
            }
            Err(error) => Err(error.clone()),
        },
    );

    // A schema outlives the topic it described — deleting a topic does not
    // touch the registry — and the summary is where that shows. Checked
    // against the topic lists of the clusters that read this registry rather
    // than against a number written down here, because which topics exist is
    // the one thing about this fleet that changes between runs.
    let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut complete = true;
    for (cluster, _) in referenced.iter().filter(|(_, id)| *id == registry_id) {
        match get(
            &client,
            &format!("/api/environments/{ENV}/clusters/{cluster}/topics?internal=true"),
        )
        .await
        {
            Ok(body) => live.extend(
                body["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|topic| topic["name"].as_str())
                    .map(str::to_owned),
            ),
            // A cluster that would not answer is exactly the case the server
            // reports as `null`, so the expectation becomes "says it does not
            // know" rather than a count.
            Err(_) => complete = false,
        }
    }
    let expected = complete.then(|| named.iter().filter(|topic| !live.contains(**topic)).count());
    acceptance.check(
        "a subject whose topic is gone is counted, and a disconnected cluster is not a count",
        match &summary {
            Ok(body) => {
                let reported = body["dangling"].as_u64().map(|n| n as usize);
                let reported = if body["dangling"].is_null() {
                    None
                } else {
                    reported
                };
                if reported == expected {
                    Ok(match expected {
                        Some(0) => "0 dangling: every subject names a live topic".to_owned(),
                        Some(n) => format!(
                            "{n} of {} subject(s) name a topic on neither cluster",
                            rows.len()
                        ),
                        None => "a cluster is not answering, so the count is null".to_owned(),
                    })
                } else {
                    Err(format!("reported {reported:?}, expected {expected:?}"))
                }
            }
            Err(error) => Err(error.clone()),
        },
    );

    if let Some(subject) = rows.first().and_then(|row| row["subject"].as_str()) {
        let versions = get(
            &client,
            &format!(
                "/api/environments/{ENV}/schema-registries/{registry_id}/subjects/{subject}/versions"
            ),
        )
        .await;
        acceptance.check(
            "a subject's versions come back with their schema text",
            match versions {
                Ok(body) => {
                    let first = body["versions"][0].clone();
                    if first["schema"].as_str().is_some_and(|s| !s.is_empty())
                        && first["id"].as_u64().is_some()
                    {
                        Ok(format!(
                            "{} v{} (#{})",
                            subject, first["version"], first["id"]
                        ))
                    } else {
                        Err(format!("{body}"))
                    }
                }
                Err(error) => Err(error),
            },
        );
    }

    // A registry id nobody configured is absent, not forbidden — the same rule
    // a cluster id follows, and for the same reason: an id that answers 403 is
    // an id that can be enumerated.
    let unknown_registry = client
        .get(url(&format!(
            "/api/environments/{ENV}/schema-registries/not-a-registry/subjects"
        )))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    acceptance.check(
        "an unconfigured registry is 404, not 403",
        if unknown_registry.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(String::new())
        } else {
            Err(format!("got {}", unknown_registry.status()))
        },
    );

    // --- the payload filter ------------------------------------------------
    //
    // It matches a literal substring of the *decoded* value, which is a claim
    // the Avro canary can settle by itself: `sequence` is a field **name**, so
    // it exists in the schema and in the rendering and nowhere in the bytes.
    // A filter applied before the decode could not find it, and one applied
    // after cannot miss it.
    let by_field_name = get(
        &client,
        &format!(
            "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages\
             ?mode=newest&limit=20&filter={}",
            urlencode("sequence")
        ),
    )
    .await;
    acceptance.check(
        "the payload filter matches a field name the encoded bytes do not contain",
        match by_field_name {
            Ok(body) => {
                let rows = body["items"].as_array().cloned().unwrap_or_default();
                // "sequence" as ASCII. Avro binary carries no field names, so
                // finding this in the raw bytes would mean the fixture changed
                // and the check no longer proves anything.
                let in_the_bytes = rows.iter().any(|row| {
                    row["value"]["raw"]["hex"]
                        .as_str()
                        .is_some_and(|hex| hex.contains("73657175656e6365"))
                });
                let in_the_rendering = rows.iter().all(|row| {
                    row["value"]["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("sequence"))
                });
                if !rows.is_empty() && in_the_rendering && !in_the_bytes {
                    Ok(format!("{} rows, none of them by their bytes", rows.len()))
                } else {
                    Err(format!(
                        "{} rows, rendered {in_the_rendering}, in bytes {in_the_bytes}",
                        rows.len()
                    ))
                }
            }
            Err(error) => Err(error),
        },
    );

    // The needle is data. Every character in it means itself, so a regex that
    // would match everything matches nothing at all.
    let as_a_pattern = get(
        &client,
        &format!(
            "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages\
             ?mode=newest&limit=20&filter={}",
            urlencode(".*")
        ),
    )
    .await;
    acceptance.check(
        "a needle is matched literally rather than as a pattern",
        match as_a_pattern {
            Ok(body) => {
                let rows = body["items"].as_array().map_or(0, Vec::len);
                if rows == 0 {
                    Ok(String::new())
                } else {
                    Err(format!("{rows} rows matched `.*` as a pattern"))
                }
            }
            Err(error) => Err(error),
        },
    );

    // The property paging rests on now that the filter runs after the decode:
    // a window read in full that matched nothing is not the end of the topic,
    // and the page has to say where to look next anyway. Anchoring on the rows
    // would answer `null` here and stop paging with the topic barely touched.
    //
    // Asserted on both directions, because they reach it by different routes —
    // the forward one counts what the scan emitted, the backward one what
    // `tail` returned — and with different claims about `hasMore`. Only the
    // forward read can promise it: `tail` spreads its limit across partitions
    // with `div_ceil`, so on this canary, where one partition of three holds
    // the records, a window of 20 reads 7 and never fills its budget. That is
    // the library's shape rather than a filter effect, and demanding `hasMore`
    // of it would be asserting the fixture instead of the behaviour.
    for (mode, budget_fills) in [("oldest", true), ("newest", false)] {
        acceptance.check(
            &format!("a {mode} window that matched nothing still says where the next one starts"),
            match get(
                &client,
                &format!(
                    "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages\
                     ?mode={mode}&limit=20&filter={}",
                    urlencode("no-record-contains-this-needle")
                ),
            )
            .await
            {
                Ok(body)
                    if body["items"].as_array().is_some_and(Vec::is_empty)
                        && body["nextOffset"].is_i64()
                        && (!budget_fills || body["hasMore"] == true) =>
                {
                    Ok(format!(
                        "next at {}, hasMore {}",
                        body["nextOffset"], body["hasMore"]
                    ))
                }
                Ok(body) => Err(format!("{body}")),
                Err(error) => Err(error),
            },
        );
    }

    // Partition selection is in the scan spec, so the broker never sends the
    // others — the filter narrows what is rendered, not what is read.
    //
    // Which partition is asked for is discovered rather than assumed: the
    // canary keys its records, so a hardcoded partition is a fixture that
    // silently becomes empty the first time the key distribution changes.
    let busiest = get(
        &client,
        &format!(
            "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages?mode=newest&limit=1"
        ),
    )
    .await
    .ok()
    .and_then(|body| body["items"][0]["partition"].as_i64())
    .unwrap_or(0);
    let one_partition = get(
        &client,
        &format!(
            "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages\
             ?mode=newest&limit=50&partitions={busiest}&filter={}",
            urlencode("sequence")
        ),
    )
    .await;
    acceptance.check(
        "a filtered read still reads only the partitions it was given",
        match one_partition {
            Ok(body) => {
                let rows = body["items"].as_array().cloned().unwrap_or_default();
                let all_one = rows
                    .iter()
                    .all(|row| row["partition"].as_i64() == Some(busiest));
                if all_one && !rows.is_empty() {
                    Ok(format!("{} rows, all partition {busiest}", rows.len()))
                } else {
                    Err(format!("{} rows from partition {busiest}", rows.len()))
                }
            }
            Err(error) => Err(error),
        },
    );

    // A needle the server will not serve is refused once, by length, rather
    // than compared against every record in a window on the caller's say-so.
    let enormous = client
        .get(url(&format!(
            "/api/environments/{ENV}/clusters/strimzi/topics/{CANARY}/messages?mode=newest&filter={}",
            urlencode(&"x".repeat(kaas_ui_core::decode::MAX_FILTER_CHARS + 1))
        )))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let enormous_status = enormous.status();
    let enormous_body: Value = enormous.json().await.unwrap_or(Value::Null);
    acceptance.check(
        "a needle past the ceiling is a 400 naming the ceiling",
        if enormous_status == reqwest::StatusCode::BAD_REQUEST
            && enormous_body["message"]
                .as_str()
                .is_some_and(|message| message.contains("at most"))
        {
            Ok(String::new())
        } else {
            Err(format!("{enormous_status}: {enormous_body}"))
        },
    );

    // --- an unknown id is absent, not forbidden -----------------------------
    let status = client
        .get(url(&format!(
            "/api/environments/{ENV}/clusters/definitely-not-configured"
        )))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .status();
    acceptance.check(
        "an unconfigured cluster is 404, not 403",
        if status == reqwest::StatusCode::NOT_FOUND {
            Ok(String::new())
        } else {
            Err(format!("got {status}"))
        },
    );

    // And the segment above it. An environment nobody can see must be absent
    // too, or every id beneath it is probeable one level up.
    let unknown_environment = client
        .get(url(
            "/api/environments/definitely-not-an-environment/clusters",
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .status();
    acceptance.check(
        "an unconfigured environment is 404, not an empty list",
        if unknown_environment == reqwest::StatusCode::NOT_FOUND {
            Ok(String::new())
        } else {
            Err(format!("got {unknown_environment}"))
        },
    );

    Ok(acceptance)
}

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

const PORT: u16 = 18099;

struct Acceptance {
    passed: usize,
    failures: Vec<String>,
}

impl Acceptance {
    fn check(&mut self, name: &str, outcome: Result<String, String>) {
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
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn run(args: &[String]) -> Task {
    let mut config = "config.dev.yaml".to_owned();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--config" {
            config = iter.next().cloned().ok_or("--config needs a path")?;
        }
    }

    let config_path = root().join(&config);
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
    let _server = Server(child);

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let outcome = runtime.block_on(assertions());

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

fn url(path: &str) -> String {
    format!("http://127.0.0.1:{PORT}{path}")
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
    let mut ready = false;
    while started.elapsed() < Duration::from_secs(10) {
        if client.get(url("/health")).send().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !ready {
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
    let fleet_started = Instant::now();
    let fleet = get(&client, "/api/clusters").await?;
    let fleet_took = fleet_started.elapsed();
    let cards = fleet["items"].as_array().cloned().unwrap_or_default();

    acceptance.check(
        "GET /api/clusters returns every configured cluster",
        if cards.len() >= 2 {
            Ok(format!("{} clusters", cards.len()))
        } else {
            Err(format!("got {}", cards.len()))
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
        let capabilities = get(&client, &format!("/api/clusters/{id}/capabilities")).await?;
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
        let topics = get(&client, &format!("/api/clusters/{id}/topics")).await?;
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
                &format!("/api/clusters/{id}/topics?search={sample}"),
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
            &format!("/api/clusters/{id}/topics?name={}", asked.join(",")),
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
            let detail = get(&client, &format!("/api/clusters/{id}/topics/kperf-bench")).await?;
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
                &format!("/api/clusters/{id}/topics/kperf-bench/messages/tail?limit=20"),
            )
            .await?;
            let records = tail["items"].as_array().map_or(0, Vec::len);
            acceptance.check(
                &format!("{id}: the tail of a large topic comes back"),
                if records > 0 {
                    Ok(format!("{records} records, {} fetched", tail["total"]))
                } else {
                    Err("no records".into())
                },
            );
        }
    }

    // --- groups -------------------------------------------------------------
    for id in &ids {
        let groups = get(&client, &format!("/api/clusters/{id}/groups")).await?;
        let listed = groups["items"].as_array().cloned().unwrap_or_default();
        acceptance.check(
            &format!("{id}: groups list"),
            Ok(format!("{} groups", listed.len())),
        );

        if let Some(group) = listed.first().and_then(|g| g["groupId"].as_str()) {
            let offsets = get(
                &client,
                &format!("/api/clusters/{id}/groups/{group}/offsets"),
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

    // --- an unknown cluster is absent, not forbidden ------------------------
    let status = client
        .get(url("/api/clusters/definitely-not-configured"))
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

    Ok(acceptance)
}

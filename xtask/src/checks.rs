//! The invariant checks.
//!
//! Each one corresponds to a claim the plan makes that is only true if it is
//! mechanically checked. Three of them are also asserted by unit tests, on
//! purpose: a grep is defeated by a rename and a test is defeated by a call
//! site nobody tested, so it is both or neither.

use std::path::{Path, PathBuf};

use crate::{Task, root};

pub fn all() -> Task {
    one_construction_site()?;
    no_kafka_version_literal()?;
    no_committed_link_fence()?;
    no_mutating_route()?;
    Ok(())
}

/// Every `.rs` file under `crates/`.
///
/// `xtask/` is excluded on purpose: it is this file, and it necessarily names
/// every pattern it looks for.
fn sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&root().join("crates"), &mut files);
    files
}

fn collect(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("*") || trimmed.starts_with("/*")
}

/// **1. One construction site.**
///
/// Read-only is the architecture, not a setting. Exactly one
/// `Admin::connect_read_only(`, no `Admin::connect(` at all.
fn one_construction_site() -> Task {
    let needle = "Admin::connect";
    let mut read_only = Vec::new();
    let mut mutating = Vec::new();

    for path in sources() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in source.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            for (index, _) in line.match_indices(needle) {
                let rest = &line[index + needle.len()..];
                let where_ = format!("{}:{}", path.display(), number + 1);
                if rest.starts_with("_read_only(") {
                    read_only.push(where_);
                } else if rest.starts_with('(') {
                    mutating.push(where_);
                }
            }
        }
    }

    if !mutating.is_empty() {
        return Err(format!(
            "a mutating Admin::connect exists:\n  {}",
            mutating.join("\n  ")
        ));
    }
    if read_only.len() != 1 {
        return Err(format!(
            "expected exactly one Admin::connect_read_only, found {}:\n  {}",
            read_only.len(),
            read_only.join("\n  ")
        ));
    }

    println!("  ok  one construction site: {}", read_only[0]);
    Ok(())
}

/// **2. No Kafka version number anywhere.**
///
/// kaas-lib owns version and implementation compatibility completely. If
/// kaas-ui ever *needs* to know that a Kafka release added something, the
/// knowledge belongs downstairs — file it in docs/reference/upstream-asks.md.
///
/// The pattern is built from characters rather than written as a literal so
/// this file does not fail its own check.
fn no_kafka_version_literal() -> Task {
    let majors: [char; 3] = ['2', '3', '4'];
    let mut hits = Vec::new();

    for path in sources() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in source.lines().enumerate() {
            let bytes: Vec<char> = line.chars().collect();
            for index in 0..bytes.len() {
                if !majors.contains(&bytes[index]) {
                    continue;
                }
                // Not part of a longer number or identifier. `-` is in the
                // exclusion list because of hyphenated names that carry a
                // number for unrelated reasons — `Apache-2.0`, `SHA-256` —
                // and the line-level rule below is what catches a genuine
                // `kafka-4.x` anyway.
                if index > 0
                    && (bytes[index - 1].is_alphanumeric()
                        || bytes[index - 1] == '.'
                        || bytes[index - 1] == '_'
                        || bytes[index - 1] == '-')
                {
                    continue;
                }
                if bytes.get(index + 1) != Some(&'.') {
                    continue;
                }
                if !bytes.get(index + 2).is_some_and(|c| c.is_ascii_digit()) {
                    continue;
                }
                hits.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }

        // A line that names Kafka *and* carries a dotted number is suspect
        // wherever the number sits, which is what makes the exclusions above
        // safe: `Apache-2.0` passes, `kafka-4.2` does not.
        for (number, line) in source.lines().enumerate() {
            let mentions_kafka = line.to_lowercase().contains("kafka");
            let dotted = line
                .chars()
                .collect::<Vec<char>>()
                .windows(3)
                .any(|w| majors.contains(&w[0]) && w[1] == '.' && w[2].is_ascii_digit());
            if (mentions_kafka && dotted)
                || line.contains("kafka_version")
                || line.contains("broker_version")
            {
                hits.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }

    if !hits.is_empty() {
        return Err(format!(
            "a Kafka version number appears in the workspace:\n  {}\n\n\
             kaas-lib owns version compatibility. Push the knowledge down rather than \
             branching here.",
            hits.join("\n  ")
        ));
    }

    println!("  ok  no Kafka version literal");
    Ok(())
}

/// **3. No local override committed.**
fn no_committed_link_fence() -> Task {
    let manifest = root().join("Cargo.toml");
    let source = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    // A real table header, not the sentence in the comment above the
    // dependency table that explains what `link` does.
    let patched = source.lines().any(|line| {
        line.contains("BEGIN kaas-lib local override")
            || (!line.trim_start().starts_with('#') && line.trim() == "[patch.crates-io]")
    });
    if patched {
        return Err(
            "Cargo.toml carries a kaas-lib path override. Run `cargo xtask unlink` before \
             committing: a linked tree builds against a checkout nobody else has."
                .into(),
        );
    }
    println!("  ok  no local kaas-lib override");
    Ok(())
}

/// **4. No non-GET data route.**
///
/// No mutating endpoint exists — not disabled, not 403, absent from the
/// router. The auth flow (a later phase) is the only thing that may add one,
/// and it will be a named exception rather than a loosened rule.
fn no_mutating_route() -> Task {
    let mut hits = Vec::new();
    for path in sources() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in source.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            for verb in [".post(", ".put(", ".patch(", ".delete("] {
                if line.contains(verb) {
                    hits.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    if !hits.is_empty() {
        return Err(format!(
            "a non-GET route exists:\n  {}\n\n\
             Every data endpoint is a GET. There is no write path to add one to.",
            hits.join("\n  ")
        ));
    }

    println!("  ok  every route is a GET");
    Ok(())
}

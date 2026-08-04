//! `cargo xtask <task>`.
//!
//! ```text
//! ci      fmt + clippy + unit tests + the invariant checks. No cluster.
//! live    phase acceptance, against the two live clusters
//! login   Phase 4 acceptance: a real login against dex-test in the cluster
//! docs    write docs/openapi.json
//! link    point the workspace at ../kaas-lib
//! unlink  undo it
//! ```
//!
//! There is deliberately no `integration`: Docker is not available in the
//! environment kaas-ui is developed in, and two real three-node clusters are a
//! better target than one container anyway.

mod checks;
mod live;
mod login;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let rest: Vec<String> = std::env::args().skip(2).collect();

    let result = match task.as_str() {
        "ci" => ci(),
        "live" => live::run(&rest),
        "login" => login::run(),
        "docs" => docs(),
        "link" => link(),
        "unlink" => unlink(),
        other => {
            eprintln!("unknown task {other:?}\n");
            eprintln!("usage: cargo xtask <ci|live|login|docs|link|unlink>");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("\nxtask {task}: {error}");
            ExitCode::FAILURE
        }
    }
}

type Task = Result<(), String>;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn run(program: &str, args: &[&str]) -> Task {
    println!("\n$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed", args.join(" ")))
    }
}

/// fmt, clippy, unit tests, and the checks that make the plan's claims true.
fn ci() -> Task {
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--workspace"])?;

    println!("\n--- invariants ---");
    checks::all()?;
    println!("\nci: green");
    Ok(())
}

/// Write the OpenAPI document.
fn docs() -> Task {
    let spec = kaas_ui_api::openapi::spec_json()
        .map_err(|error| format!("could not build the OpenAPI document: {error}"))?;
    let path = root().join("docs").join("openapi.json");
    std::fs::write(&path, format!("{spec}\n"))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    println!("wrote {}", path.display());
    println!(
        "\nThe TypeScript client in web/src/api/types.ts is hand-written against this \n\
         document. Regenerating it with Orval is deferred until the schema stops moving \n\
         — see docs/README.md."
    );
    Ok(())
}

const FENCE_BEGIN: &str = "# --- BEGIN kaas-lib local override (cargo xtask link) ---";
const FENCE_END: &str = "# --- END kaas-lib local override ---";

/// Point the workspace at a sibling `../kaas-lib` checkout.
fn link() -> Task {
    let manifest = root().join("Cargo.toml");
    let current = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    if current.contains(FENCE_BEGIN) {
        println!("already linked");
        return Ok(());
    }

    let sibling = root().parent().unwrap().join("kaas-lib");
    if !sibling.join("crates").join("kafka-conn").exists() {
        return Err(format!(
            "{} does not look like a kaas-lib checkout",
            sibling.display()
        ));
    }

    let block = format!(
        "\n{FENCE_BEGIN}\n\
         [patch.crates-io]\n\
         kafka-conn  = {{ path = \"../kaas-lib/crates/kafka-conn\" }}\n\
         kafka-meta  = {{ path = \"../kaas-lib/crates/kafka-meta\" }}\n\
         kafka-admin = {{ path = \"../kaas-lib/crates/kafka-admin\" }}\n\
         kafka-read  = {{ path = \"../kaas-lib/crates/kafka-read\" }}\n\
         {FENCE_END}\n"
    );

    std::fs::write(&manifest, format!("{current}{block}")).map_err(|e| e.to_string())?;
    println!("linked to ../kaas-lib — `cargo xtask ci` will refuse to pass until unlinked");
    Ok(())
}

/// Remove the local override.
fn unlink() -> Task {
    let manifest = root().join("Cargo.toml");
    let current = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
    let Some(start) = current.find(FENCE_BEGIN) else {
        println!("not linked");
        return Ok(());
    };
    let Some(end) = current.find(FENCE_END) else {
        return Err("the link fence is open but never closed; fix Cargo.toml by hand".into());
    };

    let mut cleaned = String::with_capacity(current.len());
    cleaned.push_str(current[..start].trim_end());
    cleaned.push('\n');
    cleaned.push_str(&current[end + FENCE_END.len()..]);
    std::fs::write(&manifest, cleaned).map_err(|e| e.to_string())?;
    println!("unlinked");
    Ok(())
}

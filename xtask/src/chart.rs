//! The Helm chart at `deploy/helm/kaas-ui`.
//!
//! `helm lint` and `helm template` do not parse what they render, and kustomize
//! does, which is what ArgoCD runs: kaas-broker's chart once passed both with a
//! stray `-}}` that ran two documents together, and it stopped a deploy. Neither
//! tool knows what kaas-ui accepts either, so the rendered `config.yaml` goes
//! through the binary's own parser. The binary refuses unknown keys at startup,
//! which makes a stale chart a pod in CrashLoopBackOff rather than a warning.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kaas_ui_core::Config;
use serde::Deserialize as _;
use serde_yaml::Value;

use crate::{Task, root, run};

const CHART: &str = "deploy/helm/kaas-ui";
const CASES: &str = "deploy/helm/ci";

pub fn check() -> Task {
    tracks_the_workspace_version()?;
    // With a fleet, because the defaults have none and are refused on purpose.
    let full = format!("{CASES}/full.yaml");
    run("helm", &["lint", "--strict", CHART, "--values", &full])?;

    for case in cases(false)? {
        let args = values_args(&case);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        renders_for_the_binary(&name(&case), &args)?;
    }
    for case in cases(true)? {
        is_refused(&case)?;
    }
    only_startup_blocks_roll_the_pod()?;

    println!("  chart: ok");
    Ok(())
}

/// One tag publishes one image and one chart, and the chart's `appVersion` is
/// the image it installs. A `Chart.yaml` left behind would make a local render
/// install the previous release while saying nothing.
fn tracks_the_workspace_version() -> Task {
    let path = root().join(CHART).join("Chart.yaml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let chart: Value =
        serde_yaml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let workspace = env!("CARGO_PKG_VERSION");
    for field in ["version", "appVersion"] {
        let found = chart.get(field).and_then(Value::as_str);
        if found != Some(workspace) {
            return Err(format!(
                "{}: `{field}` is {found:?} and the workspace is {workspace}; a release bumps both",
                path.display()
            ));
        }
    }
    Ok(())
}

/// The files of `deploy/helm/ci` that should render, or should be refused.
fn cases(invalid: bool) -> Result<Vec<PathBuf>, String> {
    let directory = root().join(CASES);
    let entries = fs::read_dir(&directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .filter(|path| path.to_string_lossy().ends_with(".invalid.yaml") == invalid)
        .collect();
    files.sort();
    Ok(files)
}

fn name(case: &Path) -> String {
    case.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn values_args(case: &Path) -> Vec<String> {
    vec!["--values".to_string(), case.display().to_string()]
}

/// `helm template`, and either what it wrote or what it said.
fn render(args: &[&str]) -> Result<Result<String, String>, String> {
    let output = Command::new("helm")
        .args(["template", "kaas-ui", CHART])
        .args(args)
        .current_dir(root())
        .output()
        .map_err(|error| format!("could not run helm: {error}"))?;
    if output.status.success() {
        Ok(Ok(String::from_utf8_lossy(&output.stdout).into_owned()))
    } else {
        Ok(Err(String::from_utf8_lossy(&output.stderr).into_owned()))
    }
}

/// Parse every document, refusing duplicate keys, as kustomize does.
fn documents(label: &str, rendered: &str) -> Result<Vec<Value>, String> {
    let mut parsed = Vec::new();
    for document in serde_yaml::Deserializer::from_str(rendered) {
        let value = Value::deserialize(document)
            .map_err(|error| format!("{label}: rendered invalid YAML: {error}"))?;
        if !value.is_null() {
            parsed.push(value);
        }
    }
    Ok(parsed)
}

fn of_kind<'a>(documents: &'a [Value], kind: &str) -> Option<&'a Value> {
    documents
        .iter()
        .find(|document| document.get("kind").and_then(Value::as_str) == Some(kind))
}

fn renders_for_the_binary(label: &str, args: &[&str]) -> Task {
    let rendered =
        render(args)?.map_err(|error| format!("{label}: helm template failed: {error}"))?;
    let documents = documents(label, &rendered)?;
    if of_kind(&documents, "Deployment").is_none() {
        return Err(format!("{label}: rendered no Deployment"));
    }
    if let Some(map) = of_kind(&documents, "ConfigMap") {
        let file = map
            .get("data")
            .and_then(|data| data.get("config.yaml"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{label}: the ConfigMap has no config.yaml"))?;
        Config::from_yaml(file)
            .map_err(|error| format!("{label}: kaas-ui would refuse this config: {error}"))?;
    }
    println!("  chart: {label} renders");
    Ok(())
}

/// A case must fail the render, and fail it for the reason on its first line:
/// `# expect: <text>`. Failing for some other reason would prove nothing.
fn is_refused(case: &Path) -> Task {
    let label = name(case);
    let text = fs::read_to_string(case).map_err(|e| format!("{label}: {e}"))?;
    let expected = text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("# expect: "))
        .ok_or_else(|| format!("{label}: the first line must be `# expect: <text>`"))?;
    let args = values_args(case);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match render(&args)? {
        Ok(_) => Err(format!("{label}: rendered, and should have been refused")),
        Err(error) if error.contains(expected) => {
            println!("  chart: {label} is refused");
            Ok(())
        }
        Err(error) => Err(format!(
            "{label}: refused without saying {expected:?}: {error}"
        )),
    }
}

/// kaas-ui swaps `environments` live, so editing the fleet must not roll the pod
/// and sign everyone out, while every other block is read once at startup and an
/// edit to one means nothing until it does.
fn only_startup_blocks_roll_the_pod() -> Task {
    let checksum = |extra: &[&str]| -> Result<String, String> {
        let base = root().join(CASES).join("full.yaml");
        let mut args = vec!["--values", base.to_str().unwrap_or_default()];
        args.extend_from_slice(extra);
        let rendered = render(&args)?.map_err(|error| format!("helm template failed: {error}"))?;
        rendered
            .lines()
            .find_map(|line| line.trim().strip_prefix("checksum/startup-config: "))
            .map(str::to_owned)
            .ok_or_else(|| "the pod has no checksum/startup-config annotation".to_string())
    };

    let base = checksum(&[])?;
    if checksum(&["--set", "config.environments[0].name=Renamed"])? != base {
        return Err("editing `environments` would roll the pod".to_string());
    }
    if checksum(&["--set", "config.roles[0].name=renamed"])? == base {
        return Err("editing `roles` would not roll the pod".to_string());
    }
    Ok(())
}

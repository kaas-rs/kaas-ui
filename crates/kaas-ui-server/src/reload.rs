//! Configuration reload.
//!
//! Adding a cluster must not disturb the connections of the ones that did not
//! change, so a reload **swaps the registry** — building a new map that reuses
//! the existing `Arc<ClusterHandle>` for every unchanged entry — rather than
//! mutating one in place.
//!
//! The change is detected by comparing the file's bytes rather than by
//! watching it with inotify. A Kubernetes ConfigMap is mounted as a symlink to
//! a `..data` directory that is swapped atomically, so a watch on the file's
//! inode never fires; watching the directory works, and so does this, with one
//! fewer dependency and no platform-specific behaviour to get wrong.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use kaas_ui_core::{Config, Registry};

/// How often the configuration file is re-read.
const INTERVAL: Duration = Duration::from_secs(5);

/// Watch the configuration file and swap the registry when it changes.
///
/// The ownership file a config names is watched with it: a generated file
/// that changes because somebody merged a `KafkaConnector` is a change to
/// what the UI may edit, and has to land without anyone touching the config.
pub fn watch(path: PathBuf, registry: Arc<ArcSwap<Registry>>) {
    tokio::spawn(async move {
        let mut ownership = Config::load(&path)
            .ok()
            .and_then(|config| config.ownership_file);
        let mut last = fingerprint(&path, ownership.as_deref()).ok();

        loop {
            tokio::time::sleep(INTERVAL).await;

            let current = match fingerprint(&path, ownership.as_deref()) {
                Ok(bytes) => bytes,
                Err(error) => {
                    // A ConfigMap update is not atomic from the reader's point
                    // of view for a moment. Keep the registry we have.
                    tracing::debug!(path = %path.display(), %error, "config unreadable");
                    continue;
                }
            };

            if last.as_deref() == Some(current.as_slice()) {
                continue;
            }
            last = Some(current);

            let loaded = Config::load(&path);
            if let Ok(config) = &loaded {
                ownership.clone_from(&config.ownership_file);
            }
            match loaded.and_then(|config| registry.load().reloaded(&config)) {
                Ok(reloaded) => {
                    let count = reloaded.len();
                    registry.store(Arc::new(reloaded));
                    tracing::info!(clusters = count, "configuration reloaded");
                }
                // A bad edit must not take the process down: it is already
                // serving a dozen clusters that have nothing to do with it.
                Err(error) => {
                    tracing::error!(path = %path.display(), %error, "invalid configuration; keeping the previous one");
                }
            }
        }
    });
}

/// The bytes a change is detected by: the config, then the ownership file it
/// names, so editing either one reloads.
fn fingerprint(
    config: &std::path::Path,
    ownership: Option<&std::path::Path>,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = std::fs::read(config)?;
    if let Some(ownership) = ownership {
        bytes.push(0);
        bytes.extend(std::fs::read(ownership)?);
    }
    Ok(bytes)
}

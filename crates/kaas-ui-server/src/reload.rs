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
pub fn watch(path: PathBuf, registry: Arc<ArcSwap<Registry>>) {
    tokio::spawn(async move {
        let mut last = std::fs::read(&path).ok();

        loop {
            tokio::time::sleep(INTERVAL).await;

            let current = match std::fs::read(&path) {
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

            match Config::load(&path).and_then(|config| registry.load().reloaded(&config)) {
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

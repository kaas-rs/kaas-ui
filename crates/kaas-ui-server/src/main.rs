//! The kaas-ui binary.
//!
//! Loads configuration, builds the registry, starts one connector task per
//! cluster, and serves. **It connects to nothing before it starts serving** —
//! a fleet of twelve clusters in which one is down must still boot, and boot
//! at the same speed as one in which none are.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod assets;
mod reload;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::Router;
use clap::Parser;
use kaas_ui_api::AppState;
use kaas_ui_auth::Policy;
use kaas_ui_core::{Config, Registry};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

/// A read-only, multi-cluster Kafka UI.
#[derive(Debug, Parser)]
#[command(name = "kaas-ui", version, about)]
struct Args {
    /// Path to the YAML configuration.
    #[arg(short, long, default_value = "config.yaml", env = "KAAS_UI_CONFIG")]
    config: PathBuf,

    /// Print the OpenAPI document and exit.
    #[arg(long)]
    openapi: bool,

    /// Load the configuration, report what it says, and exit.
    #[arg(long)]
    check: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    if args.openapi {
        return match kaas_ui_api::openapi::spec_json() {
            Ok(spec) => {
                println!("{spec}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("could not build the OpenAPI document: {error}");
                ExitCode::FAILURE
            }
        };
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,kaas_ui=debug")),
        )
        .init();

    let config = match Config::load(&args.config) {
        Ok(config) => config,
        Err(error) => {
            // A configuration error is the one failure worth being loud and
            // specific about: everything after this point is recoverable.
            eprintln!("{}: {error}", args.config.display());
            return ExitCode::FAILURE;
        }
    };

    if args.check {
        for cluster in &config.clusters {
            println!(
                "{}\t{}\t{}",
                cluster.id,
                cluster.display_name(),
                cluster.bootstrap.join(",")
            );
        }
        return ExitCode::SUCCESS;
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("could not start the tokio runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(serve(args.config, config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "kaas-ui stopped");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config_path: PathBuf, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let listen = config.server.listen;

    let registry = Arc::new(ArcSwap::from_pointee(Registry::from_config(&config)));
    // Connectors start here, on background tasks. Nothing below waits for one.
    registry.load().spawn_connectors();
    tracing::info!(
        clusters = registry.load().len(),
        "registry built; connecting in the background"
    );

    reload::watch(config_path, Arc::clone(&registry));

    // Empty unless a proxy mounts kaas-ui under a path. Resolved once here so
    // the routes below stay rooted at `/` — the prefix is already gone by the
    // time a request arrives, and only `index.html` needs to know about it.
    let base = config.server.base_prefix();
    if !base.is_empty() {
        tracing::info!(%base, "serving under a path prefix");
    }

    // Who may see what. No roles configured is the open deployment — one
    // anonymous caller who sees every cluster, which is what kaas-ui did
    // before any of this existed. Said out loud at startup either way: an
    // operator who believes a cluster is restricted when it is not is the
    // failure this line exists to prevent.
    let policy = if config.roles.is_empty() {
        tracing::info!(
            "authentication is not configured: every request is anonymous and sees every cluster"
        );
        Policy::open()
    } else {
        Policy::enforcing(config.roles.clone())
    };
    if let Some(warning) = config.role_warning() {
        tracing::warn!("{warning}");
    }

    let state = AppState::new(Arc::clone(&registry), policy);

    let app = Router::new()
        .merge(kaas_ui_api::router(state.clone()))
        // Everything that is not an API route is the frontend, including the
        // client-side routes that have no file behind them.
        .fallback(move |uri| assets::serve(uri, base.clone()))
        // Safe over the message stream: `DefaultPredicate` already declines
        // `text/event-stream`, so events are not held back waiting for a
        // compression buffer to fill. Replacing this predicate with a
        // hand-written one would silently reintroduce that — a live view whose
        // records arrive in bursts of a hundred, seconds late.
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "serving");

    // `ConnectInfo` so the message stream can charge its per-caller ceiling to
    // somebody. Behind the cluster's tunnel every request arrives from the
    // same socket, which is why the peer address is the *fallback* there and
    // the forwarded hop is preferred — see `kaas_ui_api::streaming::Principal`.
    // Signalled the moment shutdown begins, so the deadline below measures the
    // drain rather than the whole uptime.
    let (draining, drained) = tokio::sync::oneshot::channel();

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown().await;
        // Before axum starts draining, not after. A message stream is an
        // unbounded response body: it completes when the stream does, and a
        // live tail's stream completes when the client leaves or its lifetime
        // expires. A shutdown is neither, so without this the drain waits on a
        // response that will never finish and the process has to be killed —
        // severing every open stream with no `phase: done` to explain it.
        state.stop_streams();
        let _ = draining.send(());
    });

    tokio::select! {
        result = server => result?,
        () = async move {
            let _ = drained.await;
            tokio::time::sleep(DRAIN_DEADLINE).await;
        } => {
            // The backstop. Streams end by themselves now, so reaching this
            // means something else is holding a connection open, and taking
            // longer than the orchestrator will wait only converts a tidy exit
            // into a SIGKILL.
            tracing::warn!(
                deadline = ?DRAIN_DEADLINE,
                "connections did not drain in time; exiting anyway"
            );
        }
    }

    tracing::info!("shut down cleanly");
    Ok(())
}

/// How long to wait for connections to drain before exiting regardless.
///
/// Comfortably inside Kubernetes' default `terminationGracePeriodSeconds` of
/// 30, because the choice is not "wait longer or lose data" — it is "exit
/// tidily now, or be SIGKILLed at the deadline having waited anyway".
const DRAIN_DEADLINE: Duration = Duration::from_secs(10);

/// Wait for the signals Kubernetes and a terminal actually send.
async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            // Without SIGTERM the pod still stops, just less politely. Not a
            // reason to refuse to run.
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => tracing::info!("interrupted"),
        () = terminate => tracing::info!("terminated"),
    }
}

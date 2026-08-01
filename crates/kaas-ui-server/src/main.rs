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

use arc_swap::ArcSwap;
use axum::Router;
use clap::Parser;
use kaas_ui_api::AppState;
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

    let app = Router::new()
        .merge(kaas_ui_api::router(AppState::new(Arc::clone(&registry))))
        // Everything that is not an API route is the frontend, including the
        // client-side routes that have no file behind them.
        .fallback(assets::serve)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "serving");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;

    tracing::info!("shut down cleanly");
    Ok(())
}

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

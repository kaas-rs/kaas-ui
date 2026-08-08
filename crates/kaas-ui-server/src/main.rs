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
mod dex;
mod reload;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::Router;
use clap::Parser;
use kaas_ui_api::AppState;
use kaas_ui_auth::{Policy, Provider};
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
        // Environment first, because it is now half of what identifies a
        // cluster: two lines with the same id in different environments are
        // two clusters, and a report that dropped the first column would make
        // that look like a duplicate.
        for (environment, cluster) in config.clusters() {
            println!(
                "{}\t{}\t{}\t{}",
                environment,
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

/// Assemble the router.
///
/// Split out of [`serve`] so that **authentication being optional is a test
/// rather than a claim**. A development instance behind code-server runs with
/// no identity provider anywhere: no `dex` block, no `roles`, one anonymous
/// caller who sees everything. That is not a fallback for a misconfiguration,
/// it is the default, and the tests below hold it in place.
fn build_router(
    state: AppState,
    dex: Option<&kaas_ui_core::config::DexConfig>,
    base: String,
) -> Result<Router, String> {
    let mut app = Router::new().merge(kaas_ui_api::router(state));

    // The login provider, if this deployment has one. Merged before the asset
    // fallback, which would otherwise answer `/dex/...` with index.html — a
    // 200 carrying the wrong document, which is a worse failure than a 404.
    if let Some(dex) = dex {
        let proxy = dex::DexProxy::new(&dex.upstream)?;
        tracing::info!(upstream = %dex.upstream, "serving the login provider at /dex");
        app = app.merge(dex::router(proxy));
    }

    Ok(app
        // Everything that is not an API route is the frontend, including the
        // client-side routes that have no file behind them.
        .fallback(move |uri| assets::serve(uri, base.clone()))
        // Safe over the message stream: `DefaultPredicate` already declines
        // `text/event-stream`, so events are not held back waiting for a
        // compression buffer to fill. Replacing this predicate with a
        // hand-written one would silently reintroduce that — a live view whose
        // records arrive in bursts of a hundred, seconds late.
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http()))
}

async fn serve(config_path: PathBuf, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let listen = config.server.listen;

    let registry = Arc::new(ArcSwap::from_pointee(Registry::from_config(&config)?));
    // Connectors start here, on background tasks. Nothing below waits for one
    // — and neither does a schema registry, which is dialled on first use for
    // the same reason.
    registry.load().spawn_connectors();
    tracing::info!(
        clusters = registry.load().len(),
        schema_registries = registry.load().schema_registries().count(),
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
            "no roles are configured: every request is the anonymous caller, and that caller is \
             an administrator"
        );
        Policy::open()
    } else {
        Policy::enforcing(config.roles.clone())
    };
    if let Some(warning) = config.role_warning() {
        tracing::warn!("{warning}");
    }

    let mut state = AppState::new(Arc::clone(&registry), policy);

    // Discovery happens here, at startup, so a wrong issuer is a failure to
    // boot rather than a failure at somebody's first login. Sessions are
    // encrypted with a key generated per process: restarting signs everyone
    // out, which is the trade for having no session store and no key to keep.
    //
    // Failing fast is only safe because `auth.internal_url` keeps this hop
    // inside the cluster, and it is defaulted from `dex.upstream` rather than
    // remembered — see `OidcConfig::default_internal_url_from` for what
    // discovering over the public issuer costs.
    if let Some(auth) = config.auth.clone() {
        let provider = Provider::discover(auth)
            .await
            .map_err(std::io::Error::other)?;
        tracing::info!(
            session_ttl = ?provider.session_ttl(),
            "logins are enabled; a restart ends every session"
        );
        state = state.with_auth(Arc::new(provider));
    }

    let app =
        build_router(state.clone(), config.dex.as_ref(), base).map_err(std::io::Error::other)?;

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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use kaas_ui_auth::Policy;
    use kaas_ui_core::config::DexConfig;
    use tower::ServiceExt;

    use super::*;

    fn state() -> AppState {
        let config = Config::from_yaml(
            r#"
environments:
  - id: dev
    kafka_clusters:
      - id: kaas
        bootstrap: ["kaas.kaas.svc.cluster.local:9092"]
"#,
        )
        .expect("the fixture config parses");
        AppState::new(
            Arc::new(ArcSwap::from_pointee(
                Registry::from_config(&config).unwrap(),
            )),
            Policy::open(),
        )
    }

    async fn get(app: Router, path: &str) -> StatusCode {
        app.oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("the request builds"),
        )
        .await
        .expect("the router answers")
        .status()
    }

    /// The development instance: kaas-ui behind code-server, no identity
    /// provider within reach, and none wanted. `/dex` is not a route at all —
    /// it falls through to the frontend like any other unknown path.
    #[tokio::test]
    async fn without_a_dex_block_there_is_no_login_provider_to_reach() {
        let app = build_router(state(), None, String::new()).expect("the router builds");

        // The SPA fallback answers, which is what an unrouted path does here.
        assert_eq!(get(app.clone(), "/dex/auth").await, StatusCode::OK);
        // And the application is entirely usable without one.
        assert_eq!(get(app.clone(), "/health").await, StatusCode::OK);
        assert_eq!(get(app, "/api/me").await, StatusCode::OK);
    }

    /// And with one configured the same path is the proxy — reaching for an
    /// upstream that is not there, rather than serving the frontend. The
    /// difference between these two tests is the whole of "optional".
    #[tokio::test]
    async fn with_a_dex_block_the_same_path_is_proxied() {
        let dex = DexConfig {
            // Nothing listens here. A 502 proves the request left kaas-ui.
            upstream: "http://127.0.0.1:1".to_owned(),
        };
        let app = build_router(state(), Some(&dex), String::new()).expect("the router builds");

        assert_eq!(get(app.clone(), "/dex/auth").await, StatusCode::BAD_GATEWAY);
        assert_eq!(get(app, "/health").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn an_unusable_dex_address_stops_the_process_rather_than_the_login() {
        let dex = DexConfig {
            upstream: "dex.dex.svc.cluster.local:5556".to_owned(),
        };
        assert!(build_router(state(), Some(&dex), String::new()).is_err());
    }
}

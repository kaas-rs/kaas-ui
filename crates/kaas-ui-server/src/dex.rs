//! The login provider, served under this application's own hostname.
//!
//! Dex is a separate process in a separate namespace, and every browser hop of
//! an OIDC login has to reach it: the redirect to `/dex/auth`, and the one
//! GitHub sends back to `/dex/callback`. Reaching it means being on the public
//! internet under a name the browser can resolve.
//!
//! Rather than give it a hostname of its own — a DNS record, a second public
//! surface, a second thing to remember — this forwards `/dex/*` to it from
//! inside kaas-ui. ArgoCD does exactly this for its own Dex at `/api/dex`, and
//! it is why there is no `dex.argocd.example.com` anywhere in a cluster
//! running it.
//!
//! # Two things this leans on
//!
//! **Dex serves everything under its issuer's path**, so with
//! `issuer: https://kaas.smeding.cloud/dex` it expects to receive `/dex/auth`,
//! not `/auth`. Nothing is stripped here, and nothing should be: rewriting the
//! path would break the discovery document, which advertises absolute URLs
//! built from that same issuer.
//!
//! **This forwards whatever method it is given.** Dex's token endpoint is a
//! `POST`, its login form for a password connector is a `POST`, and a proxy
//! that quietly answered `405` to those would be a proxy that works right up
//! until someone adds a connector. kaas-ui's read-only guarantee does not come
//! from the verbs on its routes — it comes from the single
//! `Admin::connect_read_only` construction site, and nothing reachable through
//! here has an admin client at all.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

/// How long the upstream has to answer before the request gives up.
///
/// A login redirect is milliseconds of work. This exists so a Dex that accepts
/// a connection and then says nothing cannot pin a handler open — the same
/// reasoning as the cluster call ceiling, at a tenth of the value because
/// there is no broker at the other end.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(10);

/// Headers that describe one hop and must not be forwarded to the next.
///
/// Copying `connection` or `transfer-encoding` onward is how a proxy produces
/// a response the client cannot frame. `host` is dropped so the client sets it
/// from the upstream authority; Dex does not read it, because every URL it
/// emits is built from the configured issuer.
const HOP_BY_HOP: [HeaderName; 9] = [
    axum::http::header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    axum::http::header::PROXY_AUTHENTICATE,
    axum::http::header::PROXY_AUTHORIZATION,
    axum::http::header::TE,
    axum::http::header::TRAILER,
    axum::http::header::TRANSFER_ENCODING,
    axum::http::header::UPGRADE,
    axum::http::header::HOST,
];

/// Where Dex is, and the client that talks to it.
#[derive(Debug, Clone)]
pub struct DexProxy {
    /// `http://dex.dex.svc.cluster.local:5556`, from the config file.
    upstream: Arc<str>,
    client: Client<HttpConnector, Body>,
}

impl DexProxy {
    /// Build a proxy to an in-cluster Dex.
    ///
    /// # Errors
    ///
    /// If the address is not a URI with a scheme and an authority — caught
    /// here rather than on the first login attempt, when whoever typed it is
    /// no longer looking.
    pub fn new(upstream: &str) -> Result<Self, String> {
        let parsed: Uri = upstream
            .parse()
            .map_err(|error| format!("dex.upstream {upstream:?} is not a URI: {error}"))?;
        if parsed.scheme().is_none() || parsed.authority().is_none() {
            return Err(format!(
                "dex.upstream {upstream:?} needs a scheme and a host, as in \
                 http://dex.dex.svc.cluster.local:5556"
            ));
        }

        // Plain HTTP only. The hop is inside the cluster, the public leg is
        // terminated by the tunnel, and an HTTPS connector here would add a
        // TLS stack to reach a service one network away.
        let mut connector = HttpConnector::new();
        connector.set_nodelay(true);

        Ok(Self {
            upstream: Arc::from(upstream.trim_end_matches('/')),
            client: Client::builder(TokioExecutor::new()).build(connector),
        })
    }
}

/// `/dex` and everything under it.
pub fn router(proxy: DexProxy) -> Router {
    Router::new()
        .route("/dex", any(forward))
        .route("/dex/{*rest}", any(forward))
        .with_state(proxy)
}

/// Forward one request and return what came back.
async fn forward(State(proxy): State<DexProxy>, request: Request) -> Response {
    let (parts, body) = request.into_parts();

    // The path is passed through unchanged — see the module docs. Dex is
    // configured to expect `/dex/...` and builds its own URLs from the issuer.
    let target = format!(
        "{}{}",
        proxy.upstream,
        parts
            .uri
            .path_and_query()
            .map_or(parts.uri.path(), |pq| pq.as_str())
    );

    let mut upstream = Request::builder().method(parts.method.clone()).uri(&target);
    if let Some(headers) = upstream.headers_mut() {
        *headers = strip_hop_by_hop(&parts.headers);
    }

    let Ok(upstream) = upstream.body(body) else {
        return bad_gateway("could not build the upstream request");
    };

    match tokio::time::timeout(UPSTREAM_TIMEOUT, proxy.client.request(upstream)).await {
        Ok(Ok(response)) => {
            let (parts, body) = response.into_parts();
            let mut out = Response::new(Body::new(body));
            *out.status_mut() = parts.status;
            *out.headers_mut() = strip_hop_by_hop(&parts.headers);
            out
        }
        Ok(Err(error)) => {
            tracing::warn!(%target, %error, "dex proxy: upstream failed");
            bad_gateway(&format!("the login provider did not answer: {error}"))
        }
        Err(_) => {
            tracing::warn!(%target, "dex proxy: upstream timed out");
            bad_gateway("the login provider did not answer in time")
        }
    }
}

fn strip_hop_by_hop(headers: &HeaderMap) -> HeaderMap {
    let mut out = headers.clone();
    for name in HOP_BY_HOP {
        out.remove(name);
    }
    out
}

/// 502, in words rather than a blank page.
///
/// Somebody sees this in a browser mid-login, so it says which component is
/// unwell — the alternative is a white page and an assumption that kaas-ui
/// itself is broken.
fn bad_gateway(detail: &str) -> Response {
    (StatusCode::BAD_GATEWAY, format!("dex proxy: {detail}\n")).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_without_a_scheme_is_refused_at_startup() {
        assert!(DexProxy::new("dex.dex.svc.cluster.local:5556").is_err());
        assert!(DexProxy::new("not a uri").is_err());
        assert!(DexProxy::new("http://dex.dex.svc.cluster.local:5556").is_ok());
    }

    #[test]
    fn a_trailing_slash_does_not_become_a_double_one() {
        // `//dex/auth` is a different path to `/dex/auth`, and Dex would answer
        // the first with a 404 that looks like a routing bug here.
        let proxy = DexProxy::new("http://dex:5556/").expect("a valid address");
        assert_eq!(&*proxy.upstream, "http://dex:5556");
    }

    #[test]
    fn hop_by_hop_headers_do_not_travel() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            "kaas.smeding.cloud".parse().unwrap(),
        );
        headers.insert(
            axum::http::header::CONNECTION,
            "keep-alive".parse().unwrap(),
        );
        headers.insert(axum::http::header::COOKIE, "session=abc".parse().unwrap());

        let out = strip_hop_by_hop(&headers);

        assert!(!out.contains_key(axum::http::header::HOST));
        assert!(!out.contains_key(axum::http::header::CONNECTION));
        // Cookies must survive: Dex keeps its own session across the connector
        // round trip, and dropping them turns a login into a redirect loop.
        assert_eq!(out.get(axum::http::header::COOKIE).unwrap(), "session=abc");
    }
}

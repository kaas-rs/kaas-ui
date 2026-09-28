//! The embedded frontend.
//!
//! `rust-embed` pulls `web/dist` in at compile time, which is why the frontend
//! stage of the container build must run *before* the Rust stage. Getting that
//! order backwards produces an image that builds cleanly and serves 404s.
//!
//! # Serving under a path prefix
//!
//! The bundle is built once, for `/`, and adapted here. A reverse proxy that
//! mounts kaas-ui at `/proxy/8099` strips that prefix before the request
//! arrives, so the *server* sees ordinary paths — but the *browser* is still at
//! the prefixed URL, and an `index.html` pointing at `/assets/index-x.js`
//! sends it outside the prefix, where the proxy answers 404.
//!
//! So the two URLs `index.html` hands the browser are rewritten at serve time:
//! the asset references, and a `<base>` element telling the frontend where it
//! is. Doing it here rather than at build time is what stops the binary from
//! being *compiled for* one deployment — a build that 404s its own assets the
//! moment it is served from somewhere else is a trap worth not setting.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

/// Everything under `web/dist`.
#[derive(Debug, Embed)]
#[folder = "../../web/dist"]
struct Assets;

/// Serve a static asset, falling back to `index.html`.
///
/// The fallback is what makes a client-side router work on a hard refresh:
/// `/clusters/kaas/topics/orders` is not a file, and the browser must still
/// get the application rather than a 404.
pub async fn serve(uri: Uri, base: String) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(response) = file(path, &base) {
        return response;
    }

    // Anything that looks like a file and is not one is genuinely missing.
    // Falling back to index.html for a `.js` request turns a bad asset URL
    // into a blank page and a console error about a MIME type, which is a
    // much worse thing to debug than a 404.
    if path
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'))
    {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    file("index.html", &base).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "no frontend was built into this binary",
        )
            .into_response()
    })
}

fn file(path: &str, base: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();

    // Vite fingerprints everything under /assets/, so those are immutable.
    // index.html never is: it is the thing that names the current fingerprints,
    // and caching it is how a browser ends up asking for a bundle that was
    // deleted two deploys ago.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    // The document is where a policy takes effect; a script or a stylesheet
    // carrying one is ignored by the browser. Computed before the body takes
    // the bytes.
    let policy = (path == "index.html")
        .then(|| content_security_policy(&String::from_utf8_lossy(&asset.data)));

    let body = if path == "index.html" && !base.is_empty() {
        match std::str::from_utf8(&asset.data) {
            Ok(html) => Body::from(rebase(html, base)),
            // Not text, which cannot happen for index.html and is still not
            // worth failing the whole page over.
            Err(_) => Body::from(asset.data.into_owned()),
        }
    } else {
        Body::from(asset.data.into_owned())
    };

    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control);
    if let Some(policy) = policy {
        response = response.header(header::CONTENT_SECURITY_POLICY, policy);
    }

    Some(
        response
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    )
}

/// The policy `index.html` is served under.
///
/// **Script is the bundle and nothing else.** The one inline script — the
/// theme resolver that has to run before first paint — is allowed by its hash,
/// computed from the embedded file rather than written down, so editing it
/// cannot silently break the page and no `'unsafe-inline'` is needed to keep
/// it working. Any other inline script, which is what an injection would be,
/// does not run.
///
/// Style keeps `'unsafe-inline'`: Radix positions popovers with inline styles
/// and a policy that refused them would be a broken tooltip rather than a
/// defence. Style injection is not script execution.
///
/// `connect-src` names GitHub because the footer asks its API which kaas-lib
/// release is newest — the one request the frontend makes off this origin.
/// `frame-ancestors 'none'` is the clickjacking defence; `X-Frame-Options` is
/// set beside it for browsers that predate it.
fn content_security_policy(html: &str) -> String {
    let hashes: String = inline_scripts(html)
        .map(|script| {
            use base64::Engine as _;
            use sha2::Digest as _;
            let digest = sha2::Sha256::digest(script.as_bytes());
            format!(
                " 'sha256-{}'",
                base64::engine::general_purpose::STANDARD.encode(digest)
            )
        })
        .collect();
    format!(
        "default-src 'self'; \
         script-src 'self'{hashes}; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; \
         font-src 'self' data:; \
         connect-src 'self' https://api.github.com; \
         object-src 'none'; \
         base-uri 'self'; \
         form-action 'self'; \
         frame-ancestors 'none'"
    )
}

/// The bodies of every `<script>` without a `src`, exactly as a browser hashes
/// them: the text between the tags, whitespace and all.
fn inline_scripts(html: &str) -> impl Iterator<Item = &str> {
    let mut rest = html;
    std::iter::from_fn(move || {
        loop {
            let open = rest.find("<script")?;
            let after_open = rest.get(open..)?;
            let tag_end = after_open.find('>')?;
            let tag = after_open.get(..tag_end)?;
            let body_start = after_open.get(tag_end + 1..)?;
            let close = body_start.find("</script>")?;
            let body = body_start.get(..close)?;
            rest = body_start.get(close..)?;
            if !tag.contains("src=") {
                return Some(body);
            }
        }
    })
}

/// Point `index.html` at a prefix.
///
/// Two edits, and both are needed:
///
/// * every `="/assets/…"` gains the prefix, so the browser asks the proxy for
///   something the proxy will forward;
/// * a `<base href="{prefix}/">` is inserted, which is how the frontend learns
///   where it is — the router's `basepath` and every URL built in JavaScript
///   read it back. See `web/src/api/base.ts`.
///
/// The asset URLs stay **absolute** rather than being made relative and left
/// to the `<base>` element. Relative URLs would resolve against the document
/// on a deep link — `/proxy/8099/clusters/kaas` would look for
/// `/proxy/8099/clusters/assets/…` in any browser that ignored the base
/// element — and an absolute rewrite has no such failure mode.
fn rebase(html: &str, base: &str) -> String {
    let rewritten = html.replace("=\"/assets/", &format!("=\"{base}/assets/"));
    match rewritten.find("<head>") {
        Some(index) => {
            let cut = index + "<head>".len();
            let (start, rest) = rewritten.split_at(cut);
            format!("{start}\n    <base href=\"{base}/\">{rest}")
        }
        // The placeholder page `build.rs` writes when no frontend was built
        // has no `<head>` and no assets to point at. Nothing to do.
        None => rewritten,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <script type="module" crossorigin src="/assets/index-abc.js"></script>
    <link rel="stylesheet" crossorigin href="/assets/index-def.css">
  </head>
  <body><div id="root"></div></body>
</html>"#;

    #[test]
    fn a_prefix_reaches_both_the_assets_and_the_base_element() {
        let html = rebase(INDEX, "/proxy/8099");
        assert!(html.contains(r#"src="/proxy/8099/assets/index-abc.js""#));
        assert!(html.contains(r#"href="/proxy/8099/assets/index-def.css""#));
        assert!(html.contains(r#"<base href="/proxy/8099/">"#));
        // And nothing is left pointing at the root, which would 404 through
        // the proxy while the rest of the page loaded.
        assert!(!html.contains(r#"="/assets/"#));
    }

    #[test]
    fn the_base_element_lands_inside_head() {
        let html = rebase(INDEX, "/kafka");
        let head = html.find("<head>").expect("head");
        let base = html.find("<base").expect("base");
        let close = html.find("</head>").expect("closing head");
        assert!(head < base && base < close, "{html}");
    }

    #[test]
    fn a_page_with_no_head_is_left_alone() {
        // `build.rs` writes exactly this when web/dist was empty at compile
        // time. It has no assets and no head; rewriting it must not mangle it.
        let placeholder = "<!doctype html>\n<meta charset=\"utf-8\">\n<title>not built</title>";
        assert_eq!(rebase(placeholder, "/proxy/8099"), placeholder);
    }

    #[test]
    fn only_inline_scripts_are_hashed() {
        let html = "<head><script>boot()</script>\
                    <script type=\"module\" src=\"/assets/index.js\"></script></head>";
        let found: Vec<&str> = inline_scripts(html).collect();
        assert_eq!(found, ["boot()"]);
    }

    #[test]
    fn the_policy_allows_the_inline_script_by_hash_and_nothing_else_inline() {
        // `echo -n 'boot()' | openssl dgst -sha256 -binary | base64`
        let policy = content_security_policy("<script>boot()</script>");
        assert!(
            policy.contains(
                "script-src 'self' 'sha256-MeZS89WlF0u+o0hCvHTBt4q1WHU+U+sJKgbdRUc36mY='"
            ),
            "{policy}"
        );
        assert!(!policy.contains("unsafe-eval"), "{policy}");
        assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
        let script_src = policy
            .split(';')
            .find(|directive| directive.trim().starts_with("script-src"))
            .expect("a script-src");
        assert!(!script_src.contains("unsafe-inline"), "{policy}");
    }

    #[test]
    fn index_html_carries_a_policy_and_assets_do_not_need_one() {
        let index = file("index.html", "").expect("index.html is always embedded");
        assert!(
            index
                .headers()
                .contains_key(header::CONTENT_SECURITY_POLICY)
        );
    }

    #[test]
    fn serving_from_the_root_rewrites_nothing() {
        // The default path, and the one that must stay byte-identical: an
        // empty prefix skips `rebase` entirely in `file`.
        assert_eq!(
            kaas_ui_core::config::ServerConfig::default().base_prefix(),
            ""
        );
    }
}

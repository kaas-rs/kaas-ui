//! Phase 4 acceptance: a login, performed by a program.
//!
//! Everything from the redirect to the session cookie was covered by unit
//! tests over recorded documents and by people using it. Nothing exercised the
//! whole flow unattended, and the phase file named that as the reason a
//! key-rotation bug reached a user.
//!
//! The provider is `dex-test` in this cluster — ClusterIP, two static-password
//! users, no public surface. The phase file asked for a container in CI, on
//! the reasoning that the alternative was depending on GitHub from a test.
//! That was a false choice: `cargo xtask live` already runs inside the cluster,
//! and there is no Docker on the development box, so a container would have
//! made this the one acceptance check that could never be run by hand.
//!
//! # Why the cookies are handled by hand
//!
//! kaas-ui's cookies are `Secure`, correctly — the browser that carries them
//! is on https. This run is not a browser and talks to a loopback port over
//! plain HTTP, so any RFC-6265 cookie jar drops both of them on the floor.
//!
//! That matters more than it sounds. Without the pending cookie the callback
//! takes its "arrived here without having started a login" branch, which
//! redirects to `/` — *the same place a successful login lands*. A jar-based
//! version of this file passed its first run for exactly that reason, having
//! verified nothing at all. So every hop here is explicit: no redirect
//! following, cookies kept per host in a map that does not know what `Secure`
//! means, and the session asserted by what it can *see* rather than by where
//! it came to rest.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::{Method, Url};
use serde_json::Value;

use crate::live::{Acceptance, PORT, start, url, wait_ready};
use crate::{Task, root};

/// The configuration whose provider is `dex-test`, and whose two roles differ
/// in exactly one permission.
const CONFIG: &str = "config.live-auth.yaml";

/// The provider that configuration points at, character for character.
///
/// Written out here as well as in the config so this check can run *before*
/// the server does — and [`fixture_is_reachable`] refuses to run at all if the
/// two ever stop agreeing, which is the only way a duplicated address is safe.
const PROVIDER: &str = "http://dex-test.dex-test.svc.cluster.local:5556/dex";

/// Fixture credentials. They authenticate to a Dex that vouches for nothing
/// but a kaas-ui on a loopback port — see `apps/dex-test/` in the cluster repo.
const ADMIN: (&str, &str) = ("admin@kaas-ui.test", "kaas-ui-acceptance-admin");
const VIEWER: (&str, &str) = ("viewer@kaas-ui.test", "kaas-ui-acceptance-viewer");

/// A redirect chain longer than this is a loop, not a login.
const MAX_HOPS: usize = 10;

/// Fail on the fixture rather than on its symptom.
///
/// `auth.issuer` in [`CONFIG`] *is* `dex-test`, so a missing fixture makes
/// `Provider::discover` fail at startup and kaas-ui exit before it binds. What
/// that looks like from here is `the server never started listening` — true,
/// three layers from the cause, and identical to what a genuine crash would
/// print.
///
/// A `GET` rather than a socket connect, so a Dex that is listening but not
/// serving its discovery document fails here too, with its status.
///
/// # Errors
///
/// If the config has stopped naming [`PROVIDER`], or the provider does not
/// answer.
async fn fixture_is_reachable() -> Result<(), String> {
    let config = std::fs::read_to_string(root().join(CONFIG))
        .map_err(|error| format!("{CONFIG}: {error}"))?;
    if !config.contains(PROVIDER) {
        return Err(format!(
            "{CONFIG} no longer points at {PROVIDER}. This check duplicates that address to \
             run before the server does, and the two have drifted — reconcile them rather \
             than deleting this."
        ));
    }

    let url = format!("{PROVIDER}/.well-known/openid-configuration");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?;

    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(format!(
            "the login fixture answered {} at {url}. Dex is listening but not serving its \
             discovery document — check the dex-test pod's logs.",
            response.status()
        )),
        Err(error) => Err(format!(
            "the login fixture is not reachable at {url}: {error}\n  \
             `cargo xtask login` needs the dex-test app synced in this cluster, and runs from \
             inside it. See apps/dex-test/ in Woestebanaan/k3s-cluster."
        )),
    }
}

pub fn run() -> Task {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;

    // Before the build and before the server: this is the failure most likely
    // to greet somebody running the command for the first time.
    runtime.block_on(fixture_is_reachable())?;

    // Held to the end of the function: dropping it kills the process, and
    // every assertion below needs it listening.
    let _server = start(CONFIG)?;

    match runtime.block_on(assertions()) {
        Ok(acceptance) if acceptance.failures.is_empty() => {
            println!("\nlogin: {} assertions, all green", acceptance.passed);
            Ok(())
        }
        Ok(acceptance) => Err(format!(
            "{} of {} assertions failed:\n  {}",
            acceptance.failures.len(),
            acceptance.passed + acceptance.failures.len(),
            acceptance.failures.join("\n  ")
        )),
        Err(error) => Err(error),
    }
}

/// Cookies, per host, with no opinion about `Secure`.
///
/// Ignoring attributes is what lets this run at all over a loopback port — and
/// it is also a blind spot, because a browser enforces every one of them.
/// [`attributes`] keeps the raw `Set-Cookie` line beside the value so the run
/// can assert on what a browser would act on.
#[derive(Default)]
struct Jar {
    values: HashMap<String, HashMap<String, String>>,
    /// The full `Set-Cookie` line each cookie last arrived on, by name.
    raw: HashMap<String, String>,
}

impl Jar {
    /// Take whatever the response set.
    ///
    /// A cookie set to the empty string is a deletion — that is how
    /// `session::clear` signs somebody out — and keeping it would send
    /// `kaas-ui-session=` on every later request.
    fn absorb(&mut self, at: &Url, response: &reqwest::Response) {
        let host = at.host_str().unwrap_or_default().to_owned();
        let jar = self.values.entry(host).or_default();
        for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
            let Ok(text) = value.to_str() else { continue };
            let Some((name, value)) = text.split(';').next().and_then(|pair| pair.split_once('='))
            else {
                continue;
            };
            let name = name.trim().to_owned();
            if value.is_empty() {
                // A deletion, and its `Set-Cookie` carries only what a
                // deletion needs — recording it here would have the attribute
                // check assert against the wrong line. The pending cookie is
                // always deleted at the callback, so this is not an edge case.
                jar.remove(&name);
            } else {
                self.raw.insert(name.clone(), text.to_owned());
                jar.insert(name, value.to_owned());
            }
        }
    }

    fn header(&self, at: &Url) -> Option<String> {
        let jar = self.values.get(at.host_str()?)?;
        if jar.is_empty() {
            return None;
        }
        Some(
            jar.iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    fn holds(&self, at: &Url, name: &str) -> bool {
        self.values
            .get(at.host_str().unwrap_or_default())
            .is_some_and(|jar| jar.contains_key(name))
    }

    /// The `Set-Cookie` line `name` last arrived on, verbatim.
    fn attributes(&self, name: &str) -> Option<&str> {
        self.raw.get(name).map(String::as_str)
    }
}

/// The attributes a browser enforces, checked on the wire.
///
/// `session.rs` unit-tests the builder; this checks what a real response
/// carried, which is the same claim one layer further out — a middleware that
/// rewrote `Set-Cookie` would pass the unit test and fail here.
///
/// Each has a distinct browser-only failure. Without `HttpOnly` script can
/// read a session. Without `Secure` it travels in clear. `SameSite=Strict`
/// strips the cookie from the provider's top-level GET back to
/// `/auth/callback`, so the login fails and reads as a broken provider. A
/// `Path` narrower than `/` hides it from the callback.
fn browser_attributes(line: &str) -> Result<String, String> {
    let lower = line.to_ascii_lowercase();
    let mut missing = Vec::new();
    if !lower.contains("httponly") {
        missing.push("HttpOnly");
    }
    if !lower.contains("secure") {
        missing.push("Secure");
    }
    if !lower.contains("samesite=lax") {
        missing.push("SameSite=Lax");
    }
    if !lower.contains("path=/") {
        missing.push("Path=/");
    }
    if missing.is_empty() {
        Ok("HttpOnly, Secure, SameSite=Lax, Path=/".to_owned())
    } else {
        Err(format!("missing {}", missing.join(", ")))
    }
}

/// One request, cookies attached, redirects **not** followed.
async fn hop(
    client: &reqwest::Client,
    jar: &mut Jar,
    method: Method,
    at: Url,
    form: Option<&[(&str, &str)]>,
) -> Result<reqwest::Response, String> {
    let mut request = client.request(method.clone(), at.clone());
    if let Some(cookies) = jar.header(&at) {
        request = request.header(reqwest::header::COOKIE, cookies);
    }
    if let Some(fields) = form {
        request = request.form(fields);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("{method} {at}: {error}"))?;
    jar.absorb(&at, &response);
    Ok(response)
}

/// Where a response says to go next, resolved against where it came from.
fn next_hop(at: &Url, response: &reqwest::Response) -> Result<Option<Url>, String> {
    if !response.status().is_redirection() {
        return Ok(None);
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .ok_or_else(|| format!("{at} answered {} with no Location", response.status()))?
        .to_str()
        .map_err(|error| format!("{at}: unreadable Location: {error}"))?;
    at.join(location)
        .map(Some)
        .map_err(|error| format!("{at}: Location {location} is not a URL: {error}"))
}

/// Follow redirects from `at` until something is not one.
async fn follow(
    client: &reqwest::Client,
    jar: &mut Jar,
    mut at: Url,
    mut response: reqwest::Response,
) -> Result<(Url, reqwest::Response), String> {
    for _ in 0..MAX_HOPS {
        match next_hop(&at, &response)? {
            None => return Ok((at, response)),
            Some(next) => {
                at = next;
                response = hop(client, jar, Method::GET, at.clone(), None).await?;
            }
        }
    }
    Err(format!("more than {MAX_HOPS} redirects, ending at {at}"))
}

/// The `href` of one connector's link on Dex's chooser page.
///
/// **Dex only shows a chooser when it has more than one connector.** With one
/// it redirects straight into it, which is what this file assumed for as long
/// as `dex-test` had only static passwords. The moment a second connector
/// exists — a `mockCallback` here, Entra alongside GitHub in the deployed one
/// — that assumption stops being true, and a harness that did not notice would
/// land on the chooser and report "expected Dex's login form".
///
/// Scanned for rather than constructed: the `req` id is Dex's, minted per
/// flow, and guessing the URL shape would couple this to a template.
fn connector_link(body: &str, connector: &str) -> Option<String> {
    let needle = format!("/auth/{connector}?req=");
    let at = body.find(&needle)?;
    let opening = body.get(..at)?.rfind(['"', '\''])?;
    let rest = body.get(opening + 1..)?;
    let closing = rest.find(['"', '\''])?;
    Some(rest.get(..closing)?.to_owned())
}

/// Start a login and get as far as the named connector.
///
/// Ends either on that connector's own page — Dex's login form, for a password
/// connector — or wherever it sent the browser next, which for a connector
/// that needs no interaction is already back at kaas-ui.
async fn reach_connector(
    client: &reqwest::Client,
    jar: &mut Jar,
    connector: &str,
) -> Result<(Url, reqwest::Response), String> {
    let start_at = Url::parse(&url("/auth/login")).map_err(|error| error.to_string())?;
    let response = hop(client, jar, Method::GET, start_at.clone(), None).await?;

    // Before anything else: kaas-ui must have stashed a pending login. Without
    // it the callback below cannot verify `state`, `nonce` or the PKCE
    // verifier, and — crucially — it does not *fail*, it redirects to `/`.
    if !jar.holds(&start_at, "kaas-ui-login") {
        return Err(format!(
            "GET /auth/login set no pending cookie (got {:?})",
            jar.header(&start_at)
        ));
    }

    let (at, response) = follow(client, jar, start_at, response).await?;
    if !response.status().is_success() {
        return Err(format!(
            "the redirect chain ended at {at} with {}",
            response.status()
        ));
    }

    // Already inside a connector: Dex had only one, or it redirected past the
    // chooser. Nothing to select.
    if at.path().contains(&format!("/auth/{connector}")) {
        return Ok((at, response));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("{at}: unreadable body: {error}"))?;
    let link = connector_link(&body, connector).ok_or_else(|| {
        format!("landed on {at} with no link to the {connector:?} connector — is it configured?")
    })?;
    let chosen = at
        .join(&link)
        .map_err(|error| format!("{at}: {link} is not a URL: {error}"))?;

    let response = hop(client, jar, Method::GET, chosen.clone(), None).await?;
    follow(client, jar, chosen, response).await
}

/// Sign in with a password, and end holding a session cookie.
///
/// # Errors
///
/// If any hop answers something other than what the flow requires — named by
/// hop, because "login failed" is the least useful sentence a test can print.
async fn sign_in(
    client: &reqwest::Client,
    jar: &mut Jar,
    (user, password): (&str, &str),
) -> Result<String, String> {
    let (form_at, response) = reach_connector(client, jar, "local").await?;
    if !response.status().is_success() {
        return Err(format!(
            "the redirect chain ended at {form_at} with {}",
            response.status()
        ));
    }
    if !form_at.path().contains("/dex/auth/local/login") {
        return Err(format!("expected Dex's login form, landed on {form_at}"));
    }

    // Dex's request id is already in the query of the URL we are sitting on,
    // which is what the form's `action` would have posted to.
    let posted = hop(
        client,
        jar,
        Method::POST,
        form_at.clone(),
        Some(&[("login", user), ("password", password)]),
    )
    .await?;

    let (landed, response) = follow(client, jar, form_at, posted).await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "the login ended at {landed} with {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    if landed.port() != Some(PORT) || landed.path() != "/" {
        return Err(format!(
            "a completed login should end at /, ended at {landed}"
        ));
    }

    // The assertion that a jar-based version of this got wrong: landing on `/`
    // proves nothing, because the "you never started a login" branch lands
    // there too. A session cookie is what a verified login leaves behind.
    if !jar.holds(&landed, "kaas-ui-session") {
        return Err(
            "the login ended at / without a session cookie — the callback took its \
             no-pending-login branch and verified nothing"
                .to_owned(),
        );
    }
    Ok(landed.to_string())
}

/// Sign in through a connector that asserts a fixed identity, groups included.
///
/// **The only way this run can prove group matching.** Dex's static-password
/// connector has no groups field at all — `staticPasswords` is email, hash,
/// username, userID and nothing else — so the two users above can never
/// exercise a role whose `subjects` names a group. `mockCallback` asserts
/// `groups: ["authors"]` and needs no interaction, so selecting it *is* the
/// whole login.
///
/// Which makes this the acceptance for the defect Part 1 fixed: before the
/// `groups` claim was read, this login succeeded and matched nothing.
async fn sign_in_as_author(client: &reqwest::Client, jar: &mut Jar) -> Result<String, String> {
    let (landed, response) = reach_connector(client, jar, "mock").await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "the mock login ended at {landed} with {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    if landed.port() != Some(PORT) || landed.path() != "/" {
        return Err(format!(
            "a completed login should end at /, ended at {landed}"
        ));
    }
    if !jar.holds(&landed, "kaas-ui-session") {
        return Err(
            "the mock login ended at / without a session cookie — the callback took its \
             no-pending-login branch and verified nothing"
                .to_owned(),
        );
    }
    Ok(landed.to_string())
}

async fn api(client: &reqwest::Client, jar: &mut Jar, path: &str) -> Result<(u16, Value), String> {
    let at = Url::parse(&url(path)).map_err(|error| error.to_string())?;
    let response = hop(client, jar, Method::GET, at, None).await?;
    let status = response.status().as_u16();
    let body = response.json().await.unwrap_or(Value::Null);
    Ok((status, body))
}

/// How many clusters this caller can see.
///
/// `items`, not `data.items`: the fleet route answers the bare list. Getting
/// this wrong reads as "nobody can see anything", which is indistinguishable
/// from the access rules working — the first version of this file asserted
/// the anonymous case green on a parse error.
fn fleet_size(body: &Value) -> usize {
    body.get("items")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

async fn assertions() -> Result<Acceptance, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        // Every redirect is inspected. See the module header.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;

    let mut acceptance = Acceptance {
        passed: 0,
        failures: Vec::new(),
    };

    let ready = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    if !wait_ready(&ready).await {
        return Err("the server never started listening".into());
    }

    // --- the anonymous caller, for contrast ---------------------------------
    //
    // Roles are configured, so an unauthenticated caller matches none of them
    // and sees nothing. Asserted first because every claim below is only
    // interesting against it: a fleet that was always visible would prove
    // nothing about a login.
    let mut nobody = Jar::default();
    let (status, body) = api(&client, &mut nobody, "/api/clusters").await?;
    acceptance.check(
        "an anonymous caller sees an empty fleet, not an error",
        if status == 200 && fleet_size(&body) == 0 {
            Ok("200, 0 clusters".to_owned())
        } else {
            Err(format!("{status}, {} clusters", fleet_size(&body)))
        },
    );

    // --- an actual login ----------------------------------------------------
    let mut admin = Jar::default();
    let signed_in = sign_in(&client, &mut admin, ADMIN).await;
    acceptance.check(
        "a login completes end to end against a real provider",
        signed_in
            .as_ref()
            .map(|_| "pending → Dex → callback → session".to_owned())
            .map_err(Clone::clone),
    );
    if signed_in.is_err() {
        // Everything below needs a session. Failing them all individually
        // would bury the one failure that matters under six identical ones.
        return Ok(acceptance);
    }

    // Both cookies, as they arrived. This run's jar deliberately ignores
    // attributes — that is the only way it works over a loopback port — so
    // without these two checks a `Secure` or `SameSite` regression would leave
    // every assertion here green and break login in every browser.
    for name in ["kaas-ui-login", "kaas-ui-session"] {
        acceptance.check(
            &format!("{name} carries the attributes a browser enforces"),
            admin.attributes(name).map_or_else(
                || Err("never seen on the wire".to_owned()),
                browser_attributes,
            ),
        );
    }

    let (status, body) = api(&client, &mut admin, "/api/clusters").await?;
    let admin_fleet = fleet_size(&body);
    acceptance.check(
        "the session that login issued is accepted",
        if status == 200 && admin_fleet > 0 {
            Ok(format!("{admin_fleet} clusters"))
        } else {
            Err(format!("{status}, {admin_fleet} clusters"))
        },
    );

    let (status, body) = api(&client, &mut admin, "/api/me").await?;
    let authenticated = body
        .get("authenticated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let named = body
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let roles: Vec<String> = body
        .get("roles")
        .and_then(Value::as_array)
        .map(|roles| {
            roles
                .iter()
                .filter_map(|role| role.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    acceptance.check(
        "the session names the person who signed in, and their roles",
        if status == 200
            && authenticated
            && named == "acceptance-admin"
            && roles == ["acceptance-admin"]
        {
            Ok(format!("{named} as {roles:?}"))
        } else {
            Err(format!("{status}: {body}"))
        },
    );

    // --- the grant boundary, with two real sessions -------------------------
    //
    // The thing Phase 4 could not show. Both halves were wired and unit-tested
    // against a constructed `Access`; what was missing was a caller who holds
    // one permission and not the other. `acceptance-viewer` differs from
    // `acceptance-admin` in exactly one line of config.
    let mut viewer = Jar::default();
    let signed_in = sign_in(&client, &mut viewer, VIEWER).await;
    acceptance.check(
        "a second user signs in with different grants",
        signed_in
            .as_ref()
            .map(|_| "viewer".to_owned())
            .map_err(Clone::clone),
    );
    if signed_in.is_err() {
        return Ok(acceptance);
    }

    let (status, body) = api(&client, &mut viewer, "/api/clusters").await?;
    let viewer_fleet = fleet_size(&body);
    acceptance.check(
        "a view-only caller still sees the fleet",
        if status == 200 && viewer_fleet == admin_fleet && viewer_fleet > 0 {
            Ok(format!("{viewer_fleet} clusters, same as admin"))
        } else {
            Err(format!(
                "{status}, {viewer_fleet} clusters against admin's {admin_fleet}"
            ))
        },
    );

    // A topic to ask payloads from. Any one will do; the boundary is about the
    // caller, not the topic.
    let (_, topics) = api(&client, &mut admin, "/api/clusters/strimzi/topics?limit=1").await?;
    let topic = topics
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let Some(topic) = topic else {
        acceptance.check(
            "the grant boundary refuses payloads to a view-only caller",
            Err("no topic on strimzi to ask payloads from".to_owned()),
        );
        return Ok(acceptance);
    };

    // `tail`, because the bare `/messages` route wants a seek mode and a
    // `400` would prove nothing about permissions either way.
    let messages = format!("/api/clusters/strimzi/topics/{topic}/messages/tail?limit=1");

    let (admin_status, _) = api(&client, &mut admin, &messages).await?;
    acceptance.check(
        "messages_read admits the caller that holds it",
        if admin_status == 200 {
            Ok(format!("200 on {topic}"))
        } else {
            Err(format!("{admin_status} on {topic}"))
        },
    );

    let (viewer_status, _) = api(&client, &mut viewer, &messages).await?;
    acceptance.check(
        "messages_read refuses the caller that does not",
        if viewer_status == 403 {
            Ok(format!("403 on {topic}"))
        } else {
            Err(format!(
                "{viewer_status} on {topic}, expected 403 — the grant boundary is open"
            ))
        },
    );

    // --- a group, rather than a person, resolving a role --------------------
    //
    // Every assertion above names its caller by email. That path worked even
    // while the `groups` claim was being parsed and thrown away, which is how
    // the defect survived a phase: a role naming a group matched nobody, the
    // config validated, the login succeeded, and the fleet came back empty.
    // `acceptance-authors` names no person — only the group `mockCallback`
    // asserts — so it can only be reached by reading the claim.
    let mut author = Jar::default();
    let signed_in = sign_in_as_author(&client, &mut author).await;
    acceptance.check(
        "a connector that asserts groups completes a login",
        signed_in
            .as_ref()
            .map(|_| "mock".to_owned())
            .map_err(Clone::clone),
    );

    if signed_in.is_ok() {
        let (status, body) = api(&client, &mut author, "/api/me").await?;
        let roles: Vec<String> = body
            .get("roles")
            .and_then(Value::as_array)
            .map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| role.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        acceptance.check(
            "a group in the claim resolves a role that names no person",
            if status == 200 && roles == ["acceptance-authors"] {
                Ok(format!("authors -> {roles:?}"))
            } else {
                Err(format!(
                    "{status}, roles {roles:?} — a role naming only a group matched nothing, \
                     so the groups claim is not reaching the policy"
                ))
            },
        );

        let (status, body) = api(&client, &mut author, "/api/clusters").await?;
        let seen = fleet_size(&body);
        acceptance.check(
            "and that role grants what it says",
            if status == 200 && seen == admin_fleet && seen > 0 {
                Ok(format!("{seen} clusters"))
            } else {
                Err(format!(
                    "{status}, {seen} clusters against admin's {admin_fleet}"
                ))
            },
        );
    }

    // --- and signing out ends it --------------------------------------------
    let logout_at = Url::parse(&url("/auth/logout")).map_err(|error| error.to_string())?;
    let logout = hop(&client, &mut admin, Method::POST, logout_at.clone(), None).await?;
    let logout_status = logout.status().as_u16();
    let cleared = !admin.holds(&logout_at, "kaas-ui-session");
    let (after, body) = api(&client, &mut admin, "/api/clusters").await?;
    acceptance.check(
        "signing out ends this session",
        if logout_status < 400 && cleared && after == 200 && fleet_size(&body) == 0 {
            Ok("the cookie is gone and the fleet is empty again".to_owned())
        } else {
            Err(format!(
                "logout {logout_status}, cookie cleared {cleared}, then {after} with {} clusters",
                fleet_size(&body)
            ))
        },
    );

    Ok(acceptance)
}

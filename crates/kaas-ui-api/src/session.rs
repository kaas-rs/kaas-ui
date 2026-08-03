//! The session, and the login it came from, as encrypted cookies.
//!
//! There is no session store. What a session *is* here is small enough to
//! carry: a subject, a name to render, the role names a login resolved to, and
//! when it expires. A store would be a second thing to run and a second thing
//! to lose.
//!
//! **Encrypted rather than signed.** A signed cookie is readable by whoever
//! holds it; the pending-login cookie carries a PKCE verifier, and a verifier
//! anyone can read is not a verifier. Both cookies go through the same private
//! jar for that reason.
//!
//! The key is generated at startup, so **restarting the process ends every
//! session**. For a single-replica read-only browser tool that is a fair
//! trade against another secret to store, mount and rotate — and it is said
//! out loud in the startup log rather than left to be discovered as "it logs
//! me out sometimes".

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, SameSite};
use kaas_ui_auth::Pending;
use serde::{Deserialize, Serialize};

/// The signed-in session.
pub const SESSION_COOKIE: &str = "kaas-ui-session";

/// A login in flight, between the redirect and the callback.
pub const PENDING_COOKIE: &str = "kaas-ui-login";

/// How long a browser has to come back from the provider.
///
/// Ten minutes is a slow GitHub login with a password manager and a second
/// factor. Beyond that the flow is abandoned, and a pending cookie that
/// outlives its flow is a replay window.
const PENDING_TTL: Duration = Duration::from_secs(600);

/// What the session cookie holds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    /// The `sub` claim.
    pub subject: String,
    /// What to render.
    pub name: String,
    /// The roles this login resolved to — names, not the groups claim they
    /// came from. See `Policy::access_for_roles`.
    pub roles: Vec<String>,
    /// Unix seconds. Checked here rather than trusted from the cookie's own
    /// expiry, which a client controls.
    pub expires_at: u64,
}

impl Session {
    /// Whether this session is still valid.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.expires_at > now()
    }
}

/// Seconds since the epoch, saturating rather than panicking on a clock that
/// predates it.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The session in this jar, if it is still live.
///
/// An expired session reads as no session rather than as an error: the browser
/// is simply signed out, which is what expiry means.
#[must_use]
pub fn read(jar: &PrivateCookieJar) -> Option<Session> {
    let raw = jar.get(SESSION_COOKIE)?;
    let session: Session = serde_json::from_str(raw.value()).ok()?;
    session.is_live().then_some(session)
}

/// Put a session in the jar.
pub fn issue(
    jar: PrivateCookieJar,
    subject: String,
    name: String,
    roles: Vec<String>,
    ttl: Duration,
) -> PrivateCookieJar {
    let session = Session {
        subject,
        name,
        roles,
        expires_at: now().saturating_add(ttl.as_secs()),
    };
    match serde_json::to_string(&session) {
        Ok(value) => jar.add(cookie(SESSION_COOKIE, value, ttl)),
        // Unreachable with a struct of owned strings, and a session that
        // cannot be written is better than one that is written empty.
        Err(_) => jar,
    }
}

/// Remember a login in flight.
pub fn stash(jar: PrivateCookieJar, pending: &Pending) -> PrivateCookieJar {
    match serde_json::to_string(pending) {
        Ok(value) => jar.add(cookie(PENDING_COOKIE, value, PENDING_TTL)),
        Err(_) => jar,
    }
}

/// Read the login in flight and remove it.
///
/// Removed whether or not the callback succeeds: a pending login is good for
/// exactly one attempt, and leaving it in place would let a captured callback
/// URL be replayed.
pub fn take(jar: PrivateCookieJar) -> (Option<Pending>, PrivateCookieJar) {
    let pending = jar
        .get(PENDING_COOKIE)
        .and_then(|raw| serde_json::from_str(raw.value()).ok());
    (pending, clear(jar, PENDING_COOKIE))
}

/// Remove a cookie, telling the browser to forget it too.
pub fn clear(jar: PrivateCookieJar, name: &'static str) -> PrivateCookieJar {
    jar.remove(Cookie::from(name))
}

/// The attributes every cookie here carries.
///
/// `HttpOnly` so script cannot read a session; `Secure` because an OIDC
/// redirect URL is https anyway; `Lax` because the provider returns the
/// browser by a top-level navigation, which `Strict` would strip the cookie
/// from — and that failure looks exactly like a broken login.
fn cookie(name: &'static str, value: String, ttl: Duration) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        // The browser-side lifetime is a hint; `Session::expires_at` inside
        // the encrypted payload is what actually decides, because a client
        // controls its own cookie jar and does not control this.
        .max_age(time::Duration::seconds(
            ttl.as_secs().try_into().unwrap_or(i64::MAX),
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::Key;

    use super::*;

    fn jar() -> PrivateCookieJar {
        PrivateCookieJar::new(Key::generate())
    }

    #[test]
    fn a_session_survives_the_round_trip() {
        let jar = issue(
            jar(),
            "sub-1".to_owned(),
            "Woestebanaan".to_owned(),
            vec!["everyone".to_owned()],
            Duration::from_secs(3600),
        );
        let session = read(&jar).expect("the session was just written");
        assert_eq!(session.subject, "sub-1");
        assert_eq!(session.roles, ["everyone"]);
        assert!(session.is_live());
    }

    #[test]
    fn an_expired_session_reads_as_no_session() {
        let jar = issue(
            jar(),
            "sub-1".to_owned(),
            "Woestebanaan".to_owned(),
            Vec::new(),
            Duration::from_secs(0),
        );
        assert!(read(&jar).is_none());
    }

    #[test]
    fn a_pending_login_is_good_for_one_attempt() {
        let pending = Pending {
            state: "state".to_owned(),
            nonce: "nonce".to_owned(),
            verifier: "verifier".to_owned(),
        };
        let jar = stash(jar(), &pending);

        let (found, jar) = take(jar);
        assert_eq!(found.expect("stashed").verifier, "verifier");

        // Gone on the second look, so a captured callback cannot be replayed.
        let (again, _) = take(jar);
        assert!(again.is_none());
    }

    #[test]
    fn a_session_is_opaque_to_a_process_with_a_different_key() {
        // Through real headers, because that is the only place the value is
        // actually encrypted: `jar.get` hands back what `jar.add` was given.
        // This is the property the module doc claims — a restart rotates the
        // key, and every session in the wild becomes unreadable.
        use axum::http::HeaderMap;
        use axum::http::header::{COOKIE, SET_COOKIE};
        use axum::response::IntoResponse;

        let key = Key::generate();
        let jar = issue(
            PrivateCookieJar::new(key.clone()),
            "sub-1".to_owned(),
            "Woestebanaan".to_owned(),
            vec!["everyone".to_owned()],
            Duration::from_secs(3600),
        );

        let response = (jar, ()).into_response();
        let set_cookie = response
            .headers()
            .get(SET_COOKIE)
            .expect("a session was issued")
            .to_str()
            .expect("ascii");
        let ciphertext = set_cookie
            .split(';')
            .next()
            .expect("a name=value pair")
            .to_owned();
        assert!(
            !ciphertext.contains("Woestebanaan"),
            "the name travelled in clear text: {ciphertext}"
        );

        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, ciphertext.parse().expect("a cookie header"));

        // The process that wrote it reads it back.
        assert!(read(&PrivateCookieJar::from_headers(&headers, key)).is_some());
        // Anybody else — including this process after a restart — does not.
        assert!(read(&PrivateCookieJar::from_headers(&headers, Key::generate())).is_none());
    }
}

//! The handlers.
//!
//! Every one of them is a `GET`. Adding a `.post(` here is a CI failure, and
//! that is deliberate: read-only is the architecture, not a setting, and the
//! way an architecture stays true is that breaking it does not compile — or,
//! failing that, does not merge.

pub mod auth;
pub mod capabilities;
pub mod clusters;
pub mod configs;
pub mod groups;
pub mod health;
pub mod me;
pub mod messages;
pub mod schemas;
pub mod spec;
pub mod topics;

/// Split a repeated query parameter given as `a,b,c`.
///
/// Comma-separated rather than repeated keys because `serde_urlencoded`
/// cannot collect repeated keys into a `Vec`, and reaching for another
/// extractor crate to spell `?name=a&name=b` is not worth the dependency.
pub(crate) fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

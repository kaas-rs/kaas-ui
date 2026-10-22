//! The handlers.
//!
//! Every read is a `GET`. The writes are listed — the openapi test holds the
//! exact set of non-GET paths, so a new one is a reviewed change to that list
//! — and each mints a `Grant` through the gate before its client will send
//! anything. A target with no `writes:` ceiling has nothing a grant could be
//! minted against.

pub mod admin;
pub mod analysis;
pub mod auth;
pub mod capabilities;
pub mod clusters;
pub mod configs;
pub mod connect;
pub mod connect_write;
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

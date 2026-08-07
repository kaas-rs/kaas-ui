//! Who is asking, and what they are allowed to see.
//!
//! Two halves, and this crate is currently the second one only:
//!
//! * **identity** — [`Principal`], the answer to "who is this". Today it is
//!   only ever [`Principal::anonymous`]; the OIDC exchange that produces a real
//!   one is the next slice of Phase 4.
//! * **authorization** — [`Policy`], a list of [`Role`]s, resolved against a
//!   principal into an [`Access`]: which clusters exist as far as this caller
//!   is concerned, and what they may do with them.
//!
//! # Read-only makes this small
//!
//! With no writes, permissions collapse to two axes: *which clusters* and
//! *metadata versus payloads*. That is a matrix with two columns, and it is
//! hand-rolled here on purpose — a policy engine is the part of an auth system
//! that most looks like it wants a framework and least needs one. See
//! `docs/11-built.md`, under Phase 4.
//!
//! # Nothing here knows about a provider
//!
//! No GitHub, no Google, no Entra. A [`Principal`] carries a subject and a set
//! of group strings, and Dex is what turns any of those providers into that
//! shape. Keeping this crate provider-blind is what makes a second identity
//! source a Dex config change rather than a kaas-ui release.
//!
//! [`Connector`] is the near miss, and stays on the right side of the line: it
//! is a label and an opaque id, read from *this* deployment's configuration and
//! forwarded to Dex unexamined. No code here branches on which connector it is,
//! and adding one is still a config change rather than a release — it is now
//! two config files that have to agree rather than one, which is the price of
//! not making people click through Dex's chooser page.
//!
//! # This crate has no dependency on the rest of the workspace
//!
//! Deliberately. `kaas-ui-core` depends on *it* — the registry lookup takes an
//! [`Access`] — so the arrow points this way and cannot point back. Anything
//! here that starts wanting a cluster type belongs in core instead.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod audit;
pub mod identity;
pub mod oidc;
pub mod policy;

pub use audit::{Audit, AuditError, Kind, Read};
pub use identity::Principal;
pub use oidc::{Connector, OidcConfig, OidcError, Pending, Provider};
pub use policy::Action;
pub use policy::{Access, Permission, Policy, Resource, Role};

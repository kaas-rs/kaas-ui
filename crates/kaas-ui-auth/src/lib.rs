//! Who is asking, what they may see, and what they may change.
//!
//! Three parts:
//!
//! * **identity** — [`Principal`], the answer to "who is this": the anonymous
//!   caller, or one the OIDC exchange in [`oidc`] vouched for.
//! * **authorization** — [`Policy`], a list of [`Role`]s, resolved against a
//!   principal into an [`Access`]: which clusters exist as far as this caller
//!   is concerned, and what they may do with them.
//! * **the write gate** — [`Access::authorize`], the only place a [`Grant`] is
//!   minted, and [`Audit`], which records a disclosure or a write before it
//!   happens.
//!
//! # Hand-rolled on purpose
//!
//! Permissions are a short list of resources, each with only the actions some
//! code path performs, selected by cluster id and labels. That is small enough
//! to write out, and a policy engine is the part of an auth system that most
//! looks like it wants a framework and least needs one. `docs/RBAC.md` is the
//! whole model.
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
pub mod gate;
pub mod identity;
pub mod oidc;
pub mod policy;

pub use audit::{Audit, AuditError, Change, Kind, Phase, Read};
pub use gate::{Denied, Grant, Operation, Target, Tier};
pub use identity::Principal;
pub use oidc::{Connector, OidcConfig, OidcError, Pending, Provider};
pub use policy::Action;
pub use policy::{Access, Permission, Policy, Resource, Role};

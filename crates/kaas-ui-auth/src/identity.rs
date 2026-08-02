//! Who is asking.

use std::collections::BTreeSet;

/// An authenticated caller, or the anonymous one.
///
/// The fields are exactly what a role can match on and what the UI needs to
/// render a "signed in as" line: a stable subject, a display name, and the
/// group strings the provider asserted. Claims beyond those are deliberately
/// not carried — an encrypted session cookie has a 4 KB budget, and a raw
/// `groups` claim from a large organisation will spend it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    subject: String,
    name: Option<String>,
    groups: BTreeSet<String>,
    authenticated: bool,
}

impl Principal {
    /// A caller who has not signed in.
    ///
    /// Not an error state. With no `auth` block configured every request is
    /// this, and the policy is open — which is what keeps a deployment that
    /// has never seen an identity provider working exactly as it did before.
    #[must_use]
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_owned(),
            name: None,
            groups: BTreeSet::new(),
            authenticated: false,
        }
    }

    /// A caller an identity provider vouched for.
    #[must_use]
    pub fn new(
        subject: impl Into<String>,
        name: Option<String>,
        groups: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            name,
            groups: groups.into_iter().collect(),
            authenticated: true,
        }
    }

    /// The stable id — the `sub` claim, not the email.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// What to render. Falls back to the subject rather than to nothing.
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.subject)
    }

    /// The provider's group strings — `org` and `org:team` from Dex's GitHub
    /// connector, whatever the equivalent is elsewhere.
    pub fn groups(&self) -> impl Iterator<Item = &str> {
        self.groups.iter().map(String::as_str)
    }

    /// Whether an identity provider vouched for this caller.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Everything a role's `subjects` entry may name this caller by.
    ///
    /// The subject first, then the groups. An `org:team` string is a group as
    /// far as this crate is concerned; that it looks structured is Dex's
    /// business and not something to parse here.
    pub(crate) fn identifiers(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.subject.as_str()).chain(self.groups())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_is_not_authenticated_and_has_no_groups() {
        let who = Principal::anonymous();
        assert!(!who.is_authenticated());
        assert_eq!(who.groups().count(), 0);
        // It still renders as something rather than as an empty string.
        assert_eq!(who.display_name(), "anonymous");
    }

    #[test]
    fn a_display_name_falls_back_to_the_subject() {
        let who = Principal::new("CgVhZG1pbhIFbG9jYWw", None, ["kaas-rs".to_owned()]);
        assert_eq!(who.display_name(), "CgVhZG1pbhIFbG9jYWw");

        let named = Principal::new("sub", Some("Ada".to_owned()), []);
        assert_eq!(named.display_name(), "Ada");
    }

    #[test]
    fn identifiers_are_the_subject_and_the_groups() {
        let who = Principal::new(
            "sub-1",
            None,
            ["kaas-rs".to_owned(), "kaas-rs:platform".to_owned()],
        );
        let seen: Vec<&str> = who.identifiers().collect();
        assert_eq!(seen, ["sub-1", "kaas-rs", "kaas-rs:platform"]);
    }
}

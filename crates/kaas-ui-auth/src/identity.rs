//! Who is asking.

use std::collections::BTreeSet;

/// An authenticated caller, or the anonymous one.
///
/// The fields are exactly what a role can match on and what the UI needs to
/// render a "signed in as" line: a stable subject, a display name, the other
/// names the provider knows this person by, and the group strings it asserted.
/// Claims beyond those are deliberately not carried — an encrypted session
/// cookie has a 4 KB budget, and a raw `groups` claim from a large
/// organisation will spend it.
///
/// # Aliases and groups are separate on purpose
///
/// Both end up in [`identifiers`](Self::identifiers) and a role's `subjects`
/// matches against either, so nothing downstream needs to tell them apart.
/// They are still distinct fields because *this* type is where the difference
/// is knowable: an alias is another name for this one person, a group is a set
/// they belong to. Collapsing them is how the two were confused in the first
/// place — `preferred_username` and `email` were passed positionally into a
/// parameter named `groups`, and the real `groups` claim went unread for a
/// whole phase without anything failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    subject: String,
    name: Option<String>,
    aliases: BTreeSet<String>,
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
            aliases: BTreeSet::new(),
            groups: BTreeSet::new(),
            authenticated: false,
        }
    }

    /// A caller an identity provider vouched for.
    ///
    /// The subject is the only thing required to have one: it is the `sub`
    /// claim, and every other field is something a provider may or may not
    /// assert. The rest arrive through [`with_name`](Self::with_name),
    /// [`with_aliases`](Self::with_aliases) and
    /// [`with_groups`](Self::with_groups).
    #[must_use]
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            name: None,
            aliases: BTreeSet::new(),
            groups: BTreeSet::new(),
            authenticated: true,
        }
    }

    /// What to render for this caller, when the provider said.
    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// The other names this one person answers to — `preferred_username` and
    /// `email`.
    ///
    /// A role naming a person writes one of these, because `sub` is opaque:
    /// Dex mints it from a `(connector, user id)` pair and nobody can read it
    /// off a screen.
    #[must_use]
    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = String>) -> Self {
        self.aliases = aliases.into_iter().collect();
        self
    }

    /// The sets this caller belongs to, from the `groups` claim.
    #[must_use]
    pub fn with_groups(mut self, groups: impl IntoIterator<Item = String>) -> Self {
        self.groups = groups.into_iter().collect();
        self
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

    /// The other names this person answers to — `preferred_username`, `email`.
    pub fn aliases(&self) -> impl Iterator<Item = &str> {
        self.aliases.iter().map(String::as_str)
    }

    /// The provider's group strings — an Entra group name through Dex's
    /// `microsoft` connector, `org` and `org:team` through its GitHub one,
    /// whatever the equivalent is elsewhere.
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
    /// The subject first, then the aliases, then the groups — one flat set,
    /// because a role says *who it applies to* and does not care which kind of
    /// name it was given. An `org:team` string is a group as far as this crate
    /// is concerned; that it looks structured is Dex's business and not
    /// something to parse here.
    pub(crate) fn identifiers(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.subject.as_str())
            .chain(self.aliases())
            .chain(self.groups())
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
        let who = Principal::new("CgVhZG1pbhIFbG9jYWw").with_groups(["kaas-rs".to_owned()]);
        assert_eq!(who.display_name(), "CgVhZG1pbhIFbG9jYWw");

        let named = Principal::new("sub").with_name(Some("Ada".to_owned()));
        assert_eq!(named.display_name(), "Ada");
    }

    #[test]
    fn identifiers_are_the_subject_then_the_aliases_then_the_groups() {
        let who = Principal::new("sub-1")
            .with_aliases(["ada".to_owned(), "ada@example.test".to_owned()])
            .with_groups(["kaas-rs".to_owned(), "kaas-rs:platform".to_owned()]);

        let seen: Vec<&str> = who.identifiers().collect();
        assert_eq!(
            seen,
            [
                "sub-1",
                "ada",
                "ada@example.test",
                "kaas-rs",
                "kaas-rs:platform"
            ]
        );
    }

    /// The confusion this type is shaped to prevent: an alias is not a group,
    /// and asking for one must not answer with the other.
    #[test]
    fn an_alias_is_not_a_group() {
        let who = Principal::new("sub-1")
            .with_aliases(["ada@example.test".to_owned()])
            .with_groups(["platform".to_owned()]);

        assert_eq!(who.aliases().collect::<Vec<_>>(), ["ada@example.test"]);
        assert_eq!(who.groups().collect::<Vec<_>>(), ["platform"]);
    }
}

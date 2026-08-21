//! The subject set — one identity expansion.
//!
//! **Every service builds its subject here and nowhere else**, through `SubjectSet::of`, and every
//! service that has to turn subject strings back into user ids goes through
//! `Authz::expand_to_user_ids`. A second expansion written at a call site is a second chance to get
//! the rule wrong — to drop a subject prefix it does not recognise, or to expand a secured API key
//! into its owner — and the copy that is wrong is the one nobody reads.

use infra::session::UserSession;

/// The identities one request is judged as.
///
/// Two bindings, not one, because the two branches of the rules need different shapes:
///
/// * `keys` — canonical subject keys, matched against `relation_tuples.subject_key`, which is a
///   generated column precisely so this is one indexable predicate rather than three column
///   comparisons.
/// * `user_ids` — integer user ids, for the branches that compare against an integer user column
///   (`platform#flagged` over `users.is_admin`, `board_post#author` over `board_posts.user_id`).
///   A cast-free integer comparison is what keeps those branches index-usable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubjectSet {
  keys:     Vec<String>,
  user_ids: Vec<i32>,
}

impl SubjectSet {
  /// The identity of one session.
  ///
  /// A **secured** API key is the whole identity: it does not expand to the owner's user, role or
  /// organization, so it never reaches `users.is_admin`. That is the structural fix for "a secured
  /// key bound to an admin keeps full administrative power" — `is_admin` stops being a boolean
  /// read off the session anywhere on an authorization path, and becomes the `platform#flagged`
  /// branch, whose `user_ids` binding is empty here. The branch is then statically false rather
  /// than accidentally true.
  ///
  /// An **unsecured** key expands to the owner's full identity, which is what the user guard sets
  /// up: `session.profile` arrives already filled in with the owner and `profile.api_key` unset, so
  /// this function cannot tell such a request from the owner's own — and must not, because that
  /// equivalence is what "unsecured" means.
  pub fn of(session: &UserSession) -> Self {
    if let Some(hash) = &session.profile.api_key {
      return Self {
        keys:     vec![format!("apikey:{hash}")],
        user_ids: Vec::new(),
      };
    }

    // Membership is not inherited upward: the caller's own organization and nothing else. The
    // organization hierarchy lives on the *resource* side, via the `^` closure, so a grant on an
    // ancestor reaches a descendant and never the reverse.
    Self {
      keys:     vec![
        format!("user:{}", session.profile.id),
        format!("role:{}#member", session.profile.role.id),
        format!("organization:{}#member", session.profile.organization.id),
      ],
      user_ids: vec![session.profile.id],
    }
  }

  /// An explicit subject set, for tests and for the seeding paths that judge as somebody who has no
  /// session.
  pub fn from_parts(keys: Vec<String>, user_ids: Vec<i32>) -> Self {
    Self { keys, user_ids }
  }

  /// The empty subject — matches nothing. An anonymous caller.
  pub fn anonymous() -> Self {
    Self::default()
  }

  pub fn keys(&self) -> &[String] {
    &self.keys
  }

  pub fn user_ids(&self) -> &[i32] {
    &self.user_ids
  }

  pub fn is_empty(&self) -> bool {
    self.keys.is_empty() && self.user_ids.is_empty()
  }
}

/// The public wildcard subject. Appended by the compiler to a relation that declares `user:*`, and
/// deliberately **not** carried in `keys`: a relation that never declared a wildcard must not match
/// one even if something wrote it.
pub const PUBLIC_WILDCARD: &str = "user:*";

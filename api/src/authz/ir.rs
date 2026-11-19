//! The rule IR.
//!
//! This is what a `.sz` rule file parses into and what `compile` walks. It is deliberately dumb: it
//! carries no resolution, no defaults and no inference. Everything a rule means is either written
//! in the schema text or rejected by `validate` — because the failure mode of an inferring IR is a
//! rule that means something nobody wrote, which in this subsystem is a permission nobody granted.
//!
//! Two shape decisions worth stating:
//!
//! * Owned `String`/`Vec` rather than `&'static str`/`&'static [_]`. The registry is built by
//!   parsing `include_str!`-ed schema text once, behind a `OnceLock`, rather than by a build-script
//!   codegen step. The text is still baked into the binary and a rule edit is still a deploy; what
//!   is avoided is a code generator whose output nobody reads.
//! * `Rewrite::Walk` carries a whole `Rewrite` on its right-hand side rather than a permission
//!   name, so a chain (`board->group->blocked`) is one node type instead of a special case.

use std::fmt;

use serde::Serialize;

/// Who a stored relation will accept as a subject.
///
/// Declared per relation so a write of `service:<namespace>#read@board:7` is refused at the API
/// instead of being stored and silently never matching. Without this vocabulary a reversed row —
/// an object written where a subject belongs — is accepted and then matches nothing for as long as
/// it sits in the table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum SubjectSpec {
  /// `user`, `apikey` — a terminal subject type.
  Direct(String),
  /// `role#member`, `organization#member` — a userset. Restricted to relations that are
  /// O(1)-derivable from the session, which is what keeps the subject side a string match.
  Userset { ty: String, relation: String },
  /// `user:*` — the public wildcard.
  Wildcard(String),
}

impl fmt::Display for SubjectSpec {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      SubjectSpec::Direct(ty) => f.write_str(ty),
      SubjectSpec::Userset { ty, relation } => write!(f, "{ty}#{relation}"),
      SubjectSpec::Wildcard(ty) => write!(f, "{ty}:*"),
    }
  }
}

/// One join in a multi-hop derivation: join `table` on its `key_col`, then continue with
/// `next_col`.
///
/// `via structures.site_id` is `Hop { table: "structures", key_col: "id", next_col: "site_id" }` —
/// the key column is always the hop table's `id`, which is the only shape the grammar admits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hop {
  pub table:    String,
  pub key_col:  String,
  pub next_col: String,
}

/// What the far end of a derived relation is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum EdgeTarget {
  /// The column already holds a canonical subject key (`tasks.owner_key`), so the predicate is a
  /// bare array match with no join and no cast.
  SubjectKey,
  /// The column holds an integer user id (`board_posts.user_id`).
  User,
  /// The column identifies an object of another definition — an edge, usable only under `->`.
  Object(String),
}

/// How the derivation's table relates to the object being judged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Correlation {
  /// The table *is* the definition's backing table, so the row under test is the correlation.
  Own,
  /// `of <table>.<col>` — the table is scanned for rows whose `<col>` is this object's id. This is
  /// the userset direction (`organization#member`, `role#member`).
  Of(String),
  /// The definition has no backing table, so the derivation is uncorrelated. Only `platform`
  /// uses it, and being uncorrelated is the point: Postgres hoists it into an InitPlan.
  Free,
}

/// A derived tupleset — a relation backed by a domain column instead of a tuple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Derivation {
  pub table:       String,
  pub column:      String,
  pub target:      EdgeTarget,
  pub correlation: Correlation,
  pub hops:        Vec<Hop>,
  /// Raw SQL boolean, ANDed into the derivation. Written column-qualified in the schema text so it
  /// cannot bind to the wrong table when the derivation is emitted inside a subquery.
  pub filter:      Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum RelationSource {
  Stored {
    subjects:        Vec<SubjectSpec>,
    /// Whether a `user:*` tuple may be written *and* matched. The compiler appends the wildcard
    /// branch only here, so a wildcard tuple on a relation that never declared one cannot take
    /// effect even if something wrote it behind the FK's back.
    allows_wildcard: bool,
  },
  Derived(Derivation),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Relation {
  pub name:   String,
  pub source: RelationSource,
}

/// A userset rewrite.
// The tree comes back to itself through several of its branches, so the schema is broken at
// this type rather than at each of them.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(no_recursion))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Rewrite {
  /// `_this` — tuples on (this object, this relation), or the derived relation's predicate.
  This(String),
  /// A permission on the same object, inlined at compile time.
  Computed(String),
  /// `rel^` — a `rel` tuple on this object *or any ancestor of it*, over the definition's
  /// declared closure table.
  Closure(String),
  /// `rel->…` — tuple_to_userset. The right-hand side is a whole rewrite so a chain such as
  /// `board->group->blocked` is one node.
  Walk {
    edge: String,
    next: Box<Rewrite>,
  },
  /// `type:id->…` — an absolute object reference, for the singletons (`platform:root`,
  /// `service:<namespace>`, `delegation_scope:<scope>`).
  Absolute {
    ty:   String,
    id:   String,
    next: Box<Rewrite>,
  },
  Union(Vec<Rewrite>),
  Intersection(Vec<Rewrite>),
  /// `a - b`. `b` may itself be a union, which is what lets a post subtract its own block, its
  /// board's and its group's in one exclusion binding outside the whole grant union.
  Exclusion {
    base:     Box<Rewrite>,
    subtract: Box<Rewrite>,
  },
}

/// Where a definition's objects live, when they live anywhere.
///
/// `service`, `platform` and `delegation_scope` have no table: their objects are named by string
/// and exist only as tuple subjects/resources. Everything else is backed by a row.
/// The SQL type of a definition's object id.
///
/// Three kinds rather than a boolean because two different questions depend on it. `Int` is the one
/// `^` can use, since the closure tables are integer-keyed. `Int` **and** `Uuid` both need an
/// explicit cast to reach `relation_tuples.resource_id`, which is `text`; Postgres has no implicit
/// `uuid = text`, so a missing cast here is not a slow query, it is a statement that will not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum IdKind {
  Int,
  Uuid,
  Text,
}

impl IdKind {
  pub fn is_int(self) -> bool {
    self == IdKind::Int
  }

  pub fn needs_text_cast(self) -> bool {
    self != IdKind::Text
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResourceTable {
  pub table:  String,
  pub id_col: String,
  pub kind:   IdKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClosureDecl {
  /// The parent relation the closure materialises. Recorded for documentation and for the
  /// projection; the compiler only needs the table.
  pub relation: String,
  pub table:    String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Definition {
  pub name:        String,
  pub table:       Option<ResourceTable>,
  pub closure:     Option<ClosureDecl>,
  pub relations:   Vec<Relation>,
  pub permissions: Vec<(String, Rewrite)>,
}

impl Definition {
  pub fn relation(&self, name: &str) -> Option<&Relation> {
    self.relations.iter().find(|r| r.name == name)
  }

  pub fn permission(&self, name: &str) -> Option<&Rewrite> {
    self.permissions.iter().find(|(n, _)| n == name).map(|(_, r)| r)
  }
}

/// One legacy `access_policies.namespace`, and what it becomes.
///
/// This is the inventory validation rule 8 checks against. A namespace that reaches a live
/// `service_ap!` or `ap::ns` and has no entry here is a boot failure, not a silent fall-through —
/// which is the whole point: a namespace left out is a resource that becomes unreachable or
/// unprotected in a running product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NamespaceEntry {
  /// The legacy namespace string, e.g. `org:credit`, `board_post`.
  pub namespace:  String,
  /// Whether a route gate exists, i.e. whether `service:<namespace>` is a real object. Service
  /// object ids are the namespace strings, colons included, which is the shape the CASL client
  /// reads out of the `/me/access-policies` payload.
  pub gated:      bool,
  /// The definition row-level checks on this namespace resolve to, when there are any.
  pub definition: Option<String>,
  /// The rule file the namespace was declared in. Report-only — it names the second declaration
  /// when two files collide, and nothing compares it against anything.
  pub source:     String,
}

/// Further members for a definition **declared in another file** (`extend NAME { … }`).
///
/// A consumer's resource families are the consumer's business: sumida declares `organization` and
/// cannot enumerate the kinds of thing a product will scope to it. `extend` is how the product adds
/// them without redeclaring the definition — which is refused, because two definitions of one name
/// is two answers to one question.
///
/// Applied after every file is parsed, so the order of the files does not decide whether an
/// extension finds its target. A target that does not exist, and a member that collides with one
/// the target already has, are both boot failures: silently overriding a framework member is the
/// single worst outcome this construct could produce.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DefinitionExtension {
  pub target:      String,
  pub relations:   Vec<Relation>,
  pub permissions: Vec<(String, Rewrite)>,
}

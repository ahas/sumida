//! The vocabulary, as `GET /authz/schema` serves it (design §3.2, §9.6).
//!
//! The `/relation-tuples` screen's three selects — resource type, relation, subject type — have to
//! come from the same place the evaluator reads, or the form offers grants that cannot take effect.
//! That is the defect the old free-text form had: `access_policies` had no vocabulary at all, which
//! is why site-app's reversed `admin:user` row sat in the table looking like authority and matching
//! nothing, forever.
//!
//! **Served from the compiled registry, not from `relation_definitions`.** §3.2 describes the two
//! tables as a projection of the registry that exists for the foreign key and for this endpoint,
//! and they are the same content whenever they are in sync — but nothing in this tree rewrites them
//! at boot (`Authz::sync_schema` has no production call site; `trainer.rs` calls `boot_or_abort`, which
//! validates and does not project). A screen fed from a stale projection offers a relation the
//! evaluator does not know, which is precisely the failure this endpoint exists to prevent. The
//! registry cannot be stale: it *is* what `Authz::check` compiles.
//!
//! What is deliberately **not** exposed: `relation_definitions.source`, the `Derivation` behind a
//! derived relation. It names a table, a column, its join hops and a raw SQL filter fragment — a
//! partial map of the database schema — and the client needs none of it. All the form has to know
//! about a derived relation is that it is derived, so that it is never offered as something a
//! person can grant (`Authz::write` refuses it, and the form should not ask). `kind` carries that
//! and nothing more.
//!
//! `rewrite` **is** exposed, per §3.2: it lets the screen show *why* a permission is held and lets
//! a support engineer read the effective rule without a build. It is safe to expose where `source`
//! is not, because a `Rewrite` tree is written entirely in the vocabulary this same payload already
//! declares — relation, permission and definition names — and mentions no table and no column.

use serde::Serialize;

use super::ir::{Definition, RelationSource, Rewrite};
use super::registry::Registry;
use super::{Authz, AuthzError};

/// One grantable-or-derived relation, in the form the `/relation-tuples` form's selects need.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationView {
  pub resource_type:   String,
  pub relation:        String,
  /// `stored` — a tuple may be written for it. `derived` — it comes from a domain column, and
  /// writing a tuple would store authority the evaluator never reads. The form must not offer
  /// `derived`; `Authz::write` refuses it either way.
  pub kind:            &'static str,
  /// What the relation accepts as a subject, in the canonical form the subject field is validated
  /// against: `user`, `role#member`, `organization#member`, `apikey`, `user:*`. Empty for a derived
  /// relation, which accepts nothing.
  pub subject_types:   Vec<String>,
  /// Whether `user:*` may be written. Declared per relation, never assumed: it is meaningful on a
  /// public board and meaningless on `platform#admin`.
  pub allows_wildcard: bool,
}

/// One permission and the rule behind it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionView {
  pub resource_type: String,
  pub permission:    String,
  /// The serialised `Rewrite` tree. Read-only, and read-only in the sense that there is no write
  /// path at all: rules are compiled into the binary (E1), so a rule edit is a deploy.
  pub rewrite:       Rewrite,
}

/// The whole vocabulary. Both lists are sorted, so a client can diff two deployments' schemas.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaView {
  pub relations:   Vec<RelationView>,
  pub permissions: Vec<PermissionView>,
}

impl SchemaView {
  /// Projects the process-wide registry.
  pub fn current() -> Result<Self, AuthzError> {
    Ok(Self::of(Authz::registry()?))
  }

  pub fn of(registry: &Registry) -> Self {
    let mut relations: Vec<RelationView> = Vec::new();
    let mut permissions: Vec<PermissionView> = Vec::new();

    for definition in registry.definitions() {
      Self::collect(definition, &mut relations, &mut permissions);
    }

    relations.sort_by(|a, b| (&a.resource_type, &a.relation).cmp(&(&b.resource_type, &b.relation)));
    permissions.sort_by(|a, b| (&a.resource_type, &a.permission).cmp(&(&b.resource_type, &b.permission)));

    Self {
      relations,
      permissions,
    }
  }

  fn collect(
    definition: &Definition,
    relations: &mut Vec<RelationView>,
    permissions: &mut Vec<PermissionView>,
  ) {
    for relation in &definition.relations {
      let (kind, subject_types, allows_wildcard) = match &relation.source {
        RelationSource::Stored {
          subjects,
          allows_wildcard,
        } => ("stored", subjects.iter().map(ToString::to_string).collect(), *allows_wildcard),
        // The derivation itself stays behind: see this module's header.
        RelationSource::Derived(_) => ("derived", Vec::new(), false),
      };

      relations.push(RelationView {
        resource_type: definition.name.clone(),
        relation: relation.name.clone(),
        kind,
        subject_types,
        allows_wildcard,
      });
    }

    for (permission, rewrite) in &definition.permissions {
      permissions.push(PermissionView {
        resource_type: definition.name.clone(),
        permission:    permission.clone(),
        rewrite:       rewrite.clone(),
      });
    }
  }
}

#[cfg(test)]
mod schema_view_tests {
  use super::*;

  /// The one thing a person can act on is a **stored** relation, and the form's select is built
  /// from `kind`. A derived relation offered as grantable is a grant that stores authority the
  /// evaluator never reads — which reads as a permission and is not one.
  #[test]
  fn a_derived_relation_is_marked_derived_and_offers_no_subject_type() {
    let view = SchemaView::current().expect("the rule set must validate");

    let member = view
      .relations
      .iter()
      .find(|r| r.resource_type == "organization" && r.relation == "member")
      .expect("`organization#member` is derived from users.organization_id");
    assert_eq!(member.kind, "derived");
    assert!(member.subject_types.is_empty(), "a derived relation accepts no written subject");

    let manager = view
      .relations
      .iter()
      .find(|r| r.resource_type == "organization" && r.relation == "manager")
      .expect("`organization#manager` is the one tuple R2 is made of");
    assert_eq!(manager.kind, "stored");
    assert!(
      manager.subject_types.iter().any(|s| s == "user"),
      "the subject select would offer nothing: {:?}",
      manager.subject_types
    );
  }

  /// The payload must carry no table and no column name. `source` is where those live, and it is
  /// left out deliberately — a schema map is reconnaissance, and the client has no use for it.
  ///
  /// Asserted over the serialised payload rather than over the struct, because "the field is not in
  /// the type" is a claim that only holds until somebody adds it back.
  #[test]
  fn the_payload_names_no_table_and_no_column() {
    let json = serde_json::to_string(&SchemaView::current().expect("the rule set must validate"))
      .expect("the schema view must serialise");

    for leaked in ["users.is_admin", "organizations.parent_id", "board_posts.user_id", "\"table\""] {
      assert!(!json.contains(leaked), "`GET /authz/schema` leaked `{leaked}`");
    }
  }

  /// A permission's rule is exposed, because §3.2 asks for it: the screen shows *why* a permission
  /// is held. What is pinned here is that it arrives as a tree and not as an opaque string, so the
  /// client can render it rather than print it.
  #[test]
  fn a_permission_carries_its_rule() {
    let view = SchemaView::current().expect("the rule set must validate");
    let grant = view
      .permissions
      .iter()
      .find(|p| p.resource_type == "organization" && p.permission == "grant")
      .expect("`organization#grant` is the authority every tuple write is checked against");

    // `= manage` names another permission on the same definition. Which of the two node types
    // carries it is `validate`'s business, not this endpoint's, so both are accepted.
    assert!(
      matches!(&grant.rewrite, Rewrite::This(name) | Rewrite::Computed(name) if name == "manage"),
      "{:?}",
      grant.rewrite
    );
  }
}

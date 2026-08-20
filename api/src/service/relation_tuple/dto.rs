use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::relation_tuples::Column;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{EntityTrait, FromQueryResult, IntoSimpleExpr, QueryFilter, QueryTrait, Select};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

/// The list filter, one field per column of the `/relation-tuples` screen (design §9.6).
///
/// Every field is an **exact** match, not the `ILIKE '%…%'` `AccessPolicyFilter` uses. A tuple's
/// columns are identifiers — a resource type, a relation, a canonical subject key — and a substring
/// search over identifiers is how `user:1` finds `user:12` and a support engineer revokes the wrong
/// grant. The one thing this screen exists to make precise is *which* row.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleFilter {
  pub resource_type: Option<OneOrMany<String>>,
  pub resource_id:   Option<OneOrMany<String>>,
  pub relation:      Option<OneOrMany<String>>,
  /// A canonical subject key — `user:7`, `role:3#member`, `organization:100#member`, `user:*`.
  pub subject:       Option<OneOrMany<String>>,
}

impl RelationTupleFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(listed(self.resource_type.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(Column::ResourceType))
      })
      .apply_if(listed(self.resource_id.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(Column::ResourceId))
      })
      .apply_if(listed(self.relation.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::Relation)))
      .apply_if(listed(self.subject.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::SubjectKey)))
  }
}

/// What `/relation-tuples` accepts, declared once.
///
/// Every key compares exactly, never as a wrapped `ILIKE`: a tuple's columns are identifiers, and a
/// substring search over identifiers is how `user:1` finds `user:12` and a support engineer revokes
/// the wrong grant. The one thing this screen exists to make precise is *which* row.
const RELATION_TUPLE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("resourceType", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_sort()
    .with_facet(),
  FieldSpec::new("resourceId", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_sort()
    .with_facet(),
  FieldSpec::new("relation", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_sort()
    .with_facet(),
  FieldSpec::new("subject", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_sort()
    .with_facet(),
  FieldSpec::sort_only("expiresAt", FieldKind::DateTime),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
];

impl FilterSchema for RelationTupleFilter {
  fn fields() -> &'static [FieldSpec] {
    RELATION_TUPLE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "resourceType" => Column::ResourceType.into_simple_expr(),
      "resourceId" => Column::ResourceId.into_simple_expr(),
      "relation" => Column::Relation.into_simple_expr(),
      // The generated column, which is the form the payload carries and the screen types.
      "subject" => Column::SubjectKey.into_simple_expr(),
      "expiresAt" => Column::ExpiresAt.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "createdAt"
  }
}

/// One tuple as the screen renders it.
///
/// The shape is §9.6's new column set, and what is *missing* is the point: there is no `effect`.
/// Deny is a rule construct now (`board_post#view` subtracts `blocked`), not a row, so a column for
/// it would be a field nobody can set and every reader would ask about.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleListItem {
  pub id:            i64,
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  /// `relation_tuples.subject_key`, the generated column — one field rather than the stored triple,
  /// because that is the form every other surface in this subsystem speaks (`SubjectSet::keys`,
  /// `created_by`, the design's own `user:7` / `role:3#member` notation).
  pub subject:       String,
  pub expires_at:    Option<DateTimeWithTimeZone>,
  pub created_at:    DateTimeWithTimeZone,
  /// Who granted it. `access_policies` recorded no grantor at all, which is half the reason
  /// `assert_policy_within_authority` could not check that a granter holds what they grant.
  pub created_by:    String,
}

impl IntoResponse for RelationTupleListItem {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRelationTuple {
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  pub subject:       String,
  /// The one caveat this model has (§13.4). Absent is a grant that does not expire.
  #[serde(default)]
  pub expires_at:    Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleCreated {
  pub id: i64,
}

impl IntoResponse for RelationTupleCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct RelationTupleDeleted;

impl IntoResponse for RelationTupleDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    RelationTupleFilter::validate().expect("the tuple filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "resourceType=board&resourceId=1&relation=viewer&subject=user:7";
    let filter: RelationTupleFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.resource_type.is_some());
    assert!(filter.resource_id.is_some());
    assert!(filter.relation.is_some());
    assert!(filter.subject.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: RelationTupleFilter =
      serde_urlencoded::from_str("resourceType=board,board_post&relation=viewer,editor")
        .expect("a list parses");

    assert_eq!(filter.resource_type.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.relation.as_ref().map(OneOrMany::len), Some(2));

    let filter: RelationTupleFilter =
      serde_urlencoded::from_str("resourceType=&resourceId=&relation=&subject=")
        .expect("an empty value parses");

    assert!(listed(filter.resource_type.as_ref()).is_none());
    assert!(listed(filter.resource_id.as_ref()).is_none());
    assert!(listed(filter.relation.as_ref()).is_none());
    assert!(listed(filter.subject.as_ref()).is_none());
  }

  /// The read scope is one predicate emitted into the list statement, so nothing is answered by a
  /// refusal before the query and there is no scope-selecting parameter to withhold. The subject
  /// triple behind `subject` is not nameable either — only the generated key the payload carries.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in [
      "subject_key",
      "subject_type",
      "subject_id",
      "subject_relation",
      "resource_type",
      "created_by",
      "resource_id_int",
    ] {
      assert!(RelationTupleFilter::field(spelling).is_none(), "{spelling}");
      assert!(RelationTupleFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(RelationTupleFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["resourceType", "resourceId", "relation", "subject", "expiresAt", "createdAt"] {
      assert!(RelationTupleFilter::sort_column(key).is_some(), "{key}");
    }
  }
}

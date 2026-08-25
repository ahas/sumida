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

/// The list filter, one field per column of the `/relation-tuples` screen.
///
/// Every field is an **exact** match, never a substring one. A tuple's columns are identifiers — a resource type, a relation, a canonical subject key — and a substring
/// search over identifiers is how `user:1` finds `user:12` and a support engineer revokes the wrong
/// grant. The one thing this screen exists to make precise is *which* row.
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleFilter {
  #[cfg_attr(feature = "openapi", param(inline))]
  pub resource_type: Option<OneOrMany<String>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub resource_id:   Option<OneOrMany<String>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub relation:      Option<OneOrMany<String>>,
  /// A canonical subject key — `user:7`, `role:3#member`, `organization:100#member`, `user:*`.
  #[cfg_attr(feature = "openapi", param(inline))]
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
/// What is *missing* is the point: there is no `effect`. Blocking is a rule construct
/// (`board_post#view` subtracts `blocked`), not a row, so a column for it would be a field nobody
/// can set and every reader would ask about.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRelationTuple {
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  pub subject:       String,
  /// Expiry is the only condition a grant can carry. Absent is a grant that does not expire.
  #[serde(default)]
  pub expires_at:    Option<DateTimeWithTimeZone>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
  use entity::relation_tuples::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

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

  /// One caller for the rule compiler. Which tuples it would match is never read — what these
  /// assert is the shape of the statement, not the rows it returns.
  fn caller() -> SubjectSet {
    SubjectSet::from_parts(vec!["user:7".to_owned(), "role:3#member".to_owned()], vec![7])
  }

  /// The read scope the list is built from. The projection and the joins the service adds are left
  /// out on purpose: what is under test is how the scope and the condition tree meet in the
  /// `WHERE` clause, since a filter may only narrow an already-authorized scope
  /// (`docs/list-filters.md`).
  fn scope() -> Select<Entity> {
    Authz::filter("relation_tuple", "view", &caller(), Entity::find()).expect("the read scope compiles")
  }

  fn where_of(query: Select<Entity>) -> String {
    use sea_orm::QueryTrait;
    use sea_orm::sea_query::PostgresQueryBuilder;

    let sql = query.as_query().to_string(PostgresQueryBuilder);

    sql.split_once(" WHERE ").expect("a WHERE clause").1.to_owned()
  }

  /// The tree as the one condition a list adds to a query it has already scoped.
  fn compiled(raw: &str) -> Condition {
    ConditionTree::parse(raw)
      .expect("the tree parses")
      .compile::<RelationTupleFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"relation","op":"eq","value":"viewer"},
      {"type":"field","key":"subject","op":"eq","value":"user:7"}
    ]}"#;

    let bare = where_of(Entity::find().filter(compiled(TREE)));
    let scoped = where_of(scope().filter(compiled(TREE)));

    assert!(bare.contains(" OR "), "{bare}");
    assert!(scoped.contains(&where_of(scope())), "the scope must survive whole: {scoped}");
    assert!(
      scoped.ends_with(&format!(" AND ({bare})")),
      "the tree must be one more conjunct: {scoped}"
    );
  }
}

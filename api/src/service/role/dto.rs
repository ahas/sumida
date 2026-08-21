use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::roles::{ActiveModel, Column};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, IntoSimpleExpr, QueryTrait, Set};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::grants::ApForUser;
use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RoleFilter {
  pub name:        Option<String>,
  pub description: Option<String>,
}

impl RoleFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.description.as_ref(), |q, v| {
        q.filter(wrap_ilike(Column::Description.into_expr(), v))
      })
  }
}

/// What `/roles` accepts, declared once.
const ROLE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("description", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::sort_only("id", FieldKind::Integer),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("updatedAt", FieldKind::DateTime),
];

impl FilterSchema for RoleFilter {
  fn fields() -> &'static [FieldSpec] {
    ROLE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "name" => Column::Name.into_simple_expr(),
      "description" => Column::Description.into_simple_expr(),
      "id" => Column::Id.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      "updatedAt" => Column::UpdatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "id"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct RoleListItem {
  pub id:          i32,
  pub name:        String,
  pub description: Option<String>,
  pub created_at:  DateTimeWithTimeZone,
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetRole {
  pub id:              i32,
  pub name:            String,
  pub description:     Option<String>,
  #[sea_orm(skip)]
  pub access_policies: Vec<ApForUser>,
  pub created_at:      DateTimeWithTimeZone,
  pub updated_at:      DateTimeWithTimeZone,
}

impl IntoResponse for GetRole {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRole {
  pub id:              Optional<i32>,
  pub name:            String,
  #[serde(default)]
  pub description:     Nullable<String>,
  pub access_policies: Vec<ApForUser>,
}

impl IntoActiveModel<ActiveModel> for CreateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCreated {
  pub id: i32,
}

impl IntoResponse for RoleCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRole {
  pub name:            Optional<String>,
  #[serde(default)]
  pub description:     Nullable<String>,
  pub access_policies: Option<Vec<ApForUser>>,
}

impl IntoActiveModel<ActiveModel> for UpdateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleUpdated;

impl IntoResponse for RoleUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDeleted;

impl IntoResponse for RoleDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use entity::roles::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    RoleFilter::validate().expect("the role filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: RoleFilter =
      serde_urlencoded::from_str("name=a&description=b").expect("the flat filter parses");

    assert!(filter.name.is_some());
    assert!(filter.description.is_some());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["created_at", "updated_at", "access_policies", "user_id"] {
      assert!(RoleFilter::field(spelling).is_none(), "{spelling}");
      assert!(RoleFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(RoleFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["name", "id", "createdAt", "updatedAt"] {
      assert!(RoleFilter::sort_column(key).is_some(), "{key}");
    }
    // Declared, but not sortable.
    assert!(RoleFilter::sort_column("description").is_none());
  }

  /// One caller for the rule compiler. Which tuples it would match is never read — what these
  /// assert is the shape of the statement, not the rows it returns.
  fn caller() -> SubjectSet {
    SubjectSet::from_parts(vec!["user:7".to_owned(), "role:3#member".to_owned()], vec![7])
  }

  /// The read scope the list is built from. The projection and the joins the service adds are left
  /// out on purpose: what §11 turns on is how the scope and the tree meet in the `WHERE` clause.
  fn scope() -> Select<Entity> {
    Authz::filter(super::super::RESOURCE, "view", &caller(), Entity::find())
      .expect("the read scope compiles")
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
      .compile::<RoleFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"name","op":"contains","value":"admin"},
      {"type":"field","key":"description","op":"contains","value":"admin"}
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

use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::users::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, CommonTableExpression, Query, SimpleExpr, UnionType, WithClause};
use sea_orm::{Condition, FromQueryResult, IntoSimpleExpr, JoinType, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UserFilter {
  pub search: Option<String>,
  pub username: Option<String>,
  pub name: Option<String>,
  pub organization_id: Option<OneOrMany<i32>>,
  pub role_id: Option<OneOrMany<i32>>,
  /// Whether `organization_id` matches by equality or through the organization tree. It has no
  /// expression of its own — it selects which mechanism the other key uses — so it is a modifier of
  /// that key rather than a field.
  pub recursive: Option<bool>,
}

impl UserFilter {
  pub fn apply<E: EntityTrait>(&self, mut query: Select<E>) -> Select<E> {
    query = query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(wrap_ilike(Column::Username.into_expr(), v))
            .add(wrap_ilike(Column::FullName.into_expr(), v)),
        )
      })
      .apply_if(self.username.as_ref(), |q, v| q.filter(wrap_ilike(Column::Username.into_expr(), v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::FullName.into_expr(), v)))
      .apply_if(listed(self.role_id.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(Column::RoleId))
      });

    // An empty list means unfiltered, so it never narrows the query.
    if let Some(org_id) = self.organization_id.as_ref().filter(|v| !v.is_empty()) {
      if self.recursive.unwrap_or_default() {
        let mut base =
          org_id.into_select_statement(entity::organizations::Entity, entity::organizations::Column::Id);

        let recurse = Query::select()
          .column((Alias::new("org_child"), entity::organizations::Column::Id))
          .from_as(entity::organizations::Entity, Alias::new("org_child"))
          .join(
            JoinType::InnerJoin,
            Alias::new("org_tree"),
            Expr::col((Alias::new("org_child"), entity::organizations::Column::ParentId))
              .equals((Alias::new("org_tree"), Alias::new("id"))),
          )
          .to_owned();

        let cte_query = base.union(UnionType::All, recurse).to_owned();
        let mut org_tree_cte = CommonTableExpression::new();
        org_tree_cte.table_name(Alias::new("org_tree")).query(cte_query);

        let mut with = WithClause::new();
        with.recursive(true);
        with.cte(org_tree_cte.clone());

        let mut sub = Query::select().column(Alias::new("id")).from(Alias::new("org_tree")).to_owned();
        sub.with_cte(with);

        query = query.filter(Expr::col(Column::OrganizationId).in_subquery(sub));
      } else {
        query = match org_id {
          OneOrMany::One(v) => query.filter(Column::OrganizationId.eq(*v)),
          OneOrMany::Many(v) => query.filter(Column::OrganizationId.is_in(v.clone())),
        };
      }
    }

    query
  }
}

/// What `/users` accepts, declared once.
///
/// `search` widens the username and the full name into one predicate, and `organization_id` matches
/// either by equality or through a recursive walk of the organization tree depending on a second
/// parameter. Neither reduces to one expression under one operator, so both are the resource's own
/// to build; the keys that do reduce say so.
const USER_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("username", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("organization_id", FieldKind::Integer, MatchStrategy::Custom, &[Op::Eq, Op::In])
    .with_relation("/organizations", "name"),
  FieldSpec::new("role_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/roles", "name"),
  FieldSpec::sort_only("email", FieldKind::Text),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("updatedAt", FieldKind::DateTime),
];

impl FilterSchema for UserFilter {
  fn fields() -> &'static [FieldSpec] {
    USER_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "username" => Column::Username.into_simple_expr(),
      "name" => Column::FullName.into_simple_expr(),
      "role_id" => Column::RoleId.into_simple_expr(),
      "email" => Column::Email.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      "updatedAt" => Column::UpdatedAt.into_simple_expr(),
      // `search` spans two columns and `organization_id` spans two matching mechanisms, so neither
      // has a single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "username"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserOrganization {
  #[sea_orm(from_alias = "org_id")]
  pub id: i32,
  #[sea_orm(from_alias = "org_name")]
  pub name: String,
}

impl From<infra::session::UserOrganization> for UserOrganization {
  fn from(value: infra::session::UserOrganization) -> Self {
    Self {
      id: value.id,
      name: value.name,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserRole {
  #[sea_orm(from_alias = "role_id")]
  pub id: i32,
  #[sea_orm(from_alias = "role_name")]
  pub name: String,
}

impl From<infra::session::UserRole> for UserRole {
  fn from(value: infra::session::UserRole) -> Self {
    Self {
      id: value.id,
      name: value.name,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserListItem {
  pub id: i32,
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  #[sea_orm(from_alias = "is_banned")]
  pub is_banned: bool,
  #[sea_orm(nested, alias = "org")]
  pub organization: UserOrganization,
  #[sea_orm(nested, alias = "role")]
  pub role: UserRole,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUser {
  pub id: i32,
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  #[sea_orm(from_alias = "is_banned")]
  pub is_banned: bool,
  #[sea_orm(nested)]
  pub organization: UserOrganization,
  #[sea_orm(nested)]
  pub role: UserRole,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetUser {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateUser {
  pub id: Optional<i32>,
  pub username: String,
  pub password: String,
  pub full_name: Nullable<String>,
  pub email: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization_id: i32,
  pub role_id: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
  pub id: i32,
}

impl IntoResponse for UserCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUser {
  pub username: Optional<String>,
  pub password: Optional<String>,
  pub email: Nullable<String>,
  pub full_name: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization_id: Optional<i32>,
  pub role_id: Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated;

impl IntoResponse for UserUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted;

impl IntoResponse for UserDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
  pub username: String,
  pub password: String,
  pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivation {
  pub user_id: i32,
  pub password: String,
  pub uuid: Uuid,
}

#[cfg(test)]
mod filter_tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    UserFilter::validate().expect("the user filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "search=a&username=b&name=c&organization_id=1&role_id=2";
    let filter: UserFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
    assert!(filter.organization_id.is_some());
    assert!(filter.role_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: UserFilter =
      serde_urlencoded::from_str("organization_id=1,2&role_id=3,4").expect("a list parses");

    assert_eq!(filter.organization_id.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.role_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: UserFilter =
      serde_urlencoded::from_str("organization_id=&role_id=").expect("an empty value parses");

    assert!(listed(filter.organization_id.as_ref()).is_none());
    assert!(listed(filter.role_id.as_ref()).is_none());
  }

  /// `recursive` modifies how `organization_id` matches and has no expression of its own, so it is
  /// not a field. Nothing may reach it, and neither may any key the resource never declared.
  #[test]
  fn the_non_field_parameters_resolve_to_nothing() {
    for spelling in ["recursive", "password", "is_admin", "organizationId", "roleId"] {
      assert!(UserFilter::field(spelling).is_none(), "{spelling}");
      assert!(UserFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(UserFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  /// The two keys whose predicate spans several mechanisms reduce to no expression, so the base has
  /// nothing to compile for them; their flat parameters are untouched.
  #[test]
  fn the_custom_fields_have_no_column() {
    for key in ["search", "organization_id"] {
      let field = UserFilter::field(key).unwrap_or_else(|| panic!("`{key}` is declared"));
      assert!(matches!(field.strategy, MatchStrategy::Custom), "{key}");
      assert!(UserFilter::column(key).is_none(), "{key}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["username", "name", "email", "createdAt", "updatedAt"] {
      assert!(UserFilter::sort_column(key).is_some(), "{key}");
    }
    for key in ["search", "organization_id", "role_id"] {
      assert!(UserFilter::sort_column(key).is_none(), "{key}");
    }
  }
}

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
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
pub struct UserFilter {
  /// Text looked for in the username and in the full name at once.
  pub search:          Option<String>,
  /// Text looked for anywhere in the username.
  pub username:        Option<String>,
  /// Text looked for anywhere in the full name.
  pub name:            Option<String>,
  /// One organization or several. With `recursive`, everything under them as well.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub organization_id: Option<OneOrMany<i32>>,
  /// One role or several.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub role_id:         Option<OneOrMany<i32>>,
  /// Only accounts that may sign in, or only accounts that may not.
  pub is_enabled:      Option<bool>,
  /// Only administrators, or only accounts that are not.
  pub is_admin:        Option<bool>,
  /// Whether `organization_id` matches by equality or through the organization tree. It has no
  /// expression of its own — it selects which mechanism the other key uses — so it is a modifier of
  /// that key rather than a field.
  pub recursive:       Option<bool>,
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
      .apply_if(listed(self.role_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::RoleId)))
      .apply_if(self.is_enabled, |q, v| q.filter(Column::IsEnabled.eq(v)))
      .apply_if(self.is_admin, |q, v| q.filter(Column::IsAdmin.eq(v)));

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
    .with_relation("/organizations", "name")
    .with_facet(),
  FieldSpec::new("role_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/roles", "name")
    .with_facet(),
  // Two classes over the whole table, which is what makes narrowing to one worth offering.
  FieldSpec::new("is_enabled", FieldKind::Boolean, MatchStrategy::Comparison, &[Op::Eq])
    .with_sort()
    .with_facet(),
  FieldSpec::new("is_admin", FieldKind::Boolean, MatchStrategy::Comparison, &[Op::Eq])
    .with_sort()
    .with_facet(),
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
      "is_enabled" => Column::IsEnabled.into_simple_expr(),
      "is_admin" => Column::IsAdmin.into_simple_expr(),
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserOrganization {
  #[sea_orm(from_alias = "org_id")]
  pub id:   i32,
  #[sea_orm(from_alias = "org_name")]
  pub name: String,
}

impl From<infra::session::UserOrganization> for UserOrganization {
  fn from(value: infra::session::UserOrganization) -> Self {
    Self {
      id:   value.id,
      name: value.name,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserRole {
  #[sea_orm(from_alias = "role_id")]
  pub id:   i32,
  #[sea_orm(from_alias = "role_name")]
  pub name: String,
}

impl From<infra::session::UserRole> for UserRole {
  fn from(value: infra::session::UserRole) -> Self {
    Self {
      id:   value.id,
      name: value.name,
    }
  }
}

/// One account on a page of them.
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserListItem {
  pub id:           i32,
  pub username:     String,
  pub full_name:    Option<String>,
  pub email:        Option<String>,
  pub phone:        Option<String>,
  pub is_admin:     bool,
  pub is_enabled:   bool,
  #[sea_orm(from_alias = "is_banned")]
  pub is_banned:    bool,
  #[sea_orm(nested, alias = "org")]
  pub organization: UserOrganization,
  #[sea_orm(nested, alias = "role")]
  pub role:         UserRole,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,
}

/// One account, read on its own.
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GetUser {
  pub id:           i32,
  pub username:     String,
  pub full_name:    Option<String>,
  pub email:        Option<String>,
  pub phone:        Option<String>,
  pub is_admin:     bool,
  pub is_enabled:   bool,
  #[sea_orm(from_alias = "is_banned")]
  pub is_banned:    bool,
  #[sea_orm(nested)]
  pub organization: UserOrganization,
  #[sea_orm(nested)]
  pub role:         UserRole,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,
}

impl IntoResponse for GetUser {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// What opening an account takes.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateUser {
  /// Left out, the server picks the identifier.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub id:              Optional<i32>,
  /// What the account signs in with.
  pub username:        String,
  /// Stored hashed, and read back by no route.
  pub password:        String,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub full_name:       Nullable<String>,
  /// Where an invitation and a verification code are sent. An account can be opened without one,
  /// but it cannot then be invited.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub email:           Nullable<String>,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub phone:           Nullable<String>,
  /// Forced to false unless the caller is an administrator.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub is_admin:        Optional<bool>,
  /// An account that is not enabled cannot sign in. Activating an invitation enables it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub is_enabled:      Optional<bool>,
  /// Forced to the caller's own organization unless the caller is an administrator.
  pub organization_id: i32,
  pub role_id:         i32,
}

/// The identifier of the account that was opened.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
  pub id: i32,
}

impl IntoResponse for UserCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// What changing somebody else's account takes.
///
/// Every property may be left out, but leaving one out does not always leave it alone. `email`,
/// `fullName` and `phone` are **cleared** by their absence — an absent property and an explicit
/// `null` are the same request for those three — while everything else keeps what is stored. To
/// keep an address, send it back.
///
/// The four that decide privilege — `isAdmin`, `roleId`, `organizationId`, `isEnabled` — are dropped
/// from a request that is not an administrator's rather than refusing it.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UpdateUser {
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub username:        Optional<String>,
  /// A new password. It does not put out credentials already handed out; `/users/change-password`
  /// is what does that.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub password:        Optional<String>,
  /// Left out, it is cleared. Send it back to keep it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub email:           Nullable<String>,
  /// Left out, it is cleared. Send it back to keep it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub full_name:       Nullable<String>,
  /// Left out, it is cleared. Send it back to keep it.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub phone:           Nullable<String>,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub is_admin:        Optional<bool>,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub is_enabled:      Optional<bool>,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub organization_id: Optional<i32>,
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub role_id:         Optional<i32>,
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

/// What setting a new password against a verification code takes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
  /// The account whose password is being set.
  pub username: String,
  /// The new password.
  pub password: String,
  /// The six digits sent for kind `password-change`. It is spent whether or not what follows
  /// succeeds, so a second attempt needs a second code.
  pub code:     String,
}

/// What turning an invitation into a working account takes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserActivation {
  /// The account the invitation was issued for. An invitation is good for that one account only.
  pub user_id:  i32,
  /// The password the account signs in with from now on.
  pub password: String,
  /// The invitation token. It is spent here.
  pub uuid:     Uuid,
}

#[cfg(test)]
mod filter_tests {
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    UserFilter::validate().expect("the user filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "search=a&username=b&name=c&organization_id=1&role_id=2&is_enabled=true&is_admin=false";
    let filter: UserFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
    assert!(filter.organization_id.is_some());
    assert!(filter.role_id.is_some());
    assert!(filter.is_enabled.is_some());
    assert!(filter.is_admin.is_some());
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
    for spelling in ["recursive", "password", "organizationId", "roleId"] {
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
      .compile::<UserFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"username","op":"contains","value":"kim"},
      {"type":"field","key":"role_id","op":"in","value":[1,2]}
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

/// What the document says may be left out, put to the reader that decides it.
///
/// An annotation sits beside a field rather than inside it, so it is free to disagree with the
/// reader. Each check below drops every property the schema names, one at a time, and fails wherever
/// the document and serde do not answer the same way.
#[cfg(all(test, feature = "openapi"))]
mod schema_tests {
  use sea_orm_crud::openapi::{assert_params_required_matches_serde, assert_required_matches_serde};
  use serde_json::json;

  use super::*;

  #[test]
  fn what_opening_an_account_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<CreateUser>(json!({
      "id": 1,
      "username": "kim",
      "password": "hunter2",
      "fullName": "Kim",
      "email": "kim@example.com",
      "phone": "010-0000-0000",
      "isAdmin": false,
      "isEnabled": true,
      "organizationId": 1,
      "roleId": 1,
    }));
  }

  #[test]
  fn what_changing_an_account_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<UpdateUser>(json!({
      "username": "kim",
      "password": "hunter2",
      "email": "kim@example.com",
      "fullName": "Kim",
      "phone": "010-0000-0000",
      "isAdmin": false,
      "isEnabled": true,
      "organizationId": 1,
      "roleId": 1,
    }));
  }

  #[test]
  fn what_the_two_pre_credential_routes_take_is_what_the_document_asks_for() {
    assert_required_matches_serde::<ChangePassword>(json!({
      "username": "kim",
      "password": "hunter2",
      "code": "123456",
    }));

    assert_required_matches_serde::<UserActivation>(json!({
      "userId": 1,
      "password": "hunter2",
      "uuid": "0195b0a1-0000-7000-8000-000000000000",
    }));
  }

  #[test]
  fn every_parameter_the_list_names_is_one_the_reader_may_be_left_without() {
    assert_params_required_matches_serde::<UserFilter>(&[
      ("search", "kim"),
      ("username", "kim"),
      ("name", "Kim"),
      ("organization_id", "1,2"),
      ("role_id", "3"),
      ("is_enabled", "true"),
      ("is_admin", "false"),
      ("recursive", "true"),
    ]);
  }
}

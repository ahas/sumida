use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::user_invitations::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
pub struct UserInvitationFilter {
  /// One invited account or several.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub user_id: Option<OneOrMany<i32>>,
  /// One token or several. A value that is not a token is refused rather than dropped.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub uuid:    Option<OneOrMany<Uuid>>,
}

impl UserInvitationFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(listed(self.uuid.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::Uuid)))
      .apply_if(listed(self.user_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::UserId)))
  }
}

/// What `/user-invitations` accepts, declared once.
const USER_INVITATION_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("uuid", FieldKind::Uuid, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_sort(),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_sort()
    .with_facet(),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for UserInvitationFilter {
  fn fields() -> &'static [FieldSpec] {
    USER_INVITATION_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "uuid" => Column::Uuid.into_simple_expr(),
      "user_id" => Column::UserId.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

/// One invitation on a page of them, whether or not it is still good.
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationListItem {
  /// The account the invitation was issued for.
  pub user_id: i32,
  /// The token. It is what is presented to `POST /users/activate`.
  pub uuid:    Uuid,

  /// When it stops working. 30 days from when it was issued.
  pub expires_at: DateTimeUtc,
  pub created_at: DateTimeUtc,
}

/// One invitation, read on its own. Only one that is still good is ever answered with.
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GetUserInvitation {
  /// The account the invitation was issued for.
  pub user_id: i32,
  /// The token. It is what is presented to `POST /users/activate`.
  pub uuid:    Uuid,

  /// When it stops working. 30 days from when it was issued.
  pub expires_at: DateTimeUtc,
  pub created_at: DateTimeUtc,
}

impl IntoResponse for GetUserInvitation {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// Carries nothing. The route that reads it names the account in its path, and that route cannot
/// be reached.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateUserInvitation {}

/// The token that was issued, and when it stops working.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationCreated {
  pub uuid:       Uuid,
  pub expires_at: DateTimeUtc,
}

impl IntoResponse for UserInvitationCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// Carries nothing. The route that reads it names the account in its path, and that route cannot
/// be reached.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserInvitation {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationUpdated {
  pub uuid:       Uuid,
  pub expires_at: DateTimeUtc,
}

impl IntoResponse for UserInvitationUpdated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationDeleted;

impl IntoResponse for UserInvitationDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    UserInvitationFilter::validate().expect("the invitation filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: UserInvitationFilter =
      serde_urlencoded::from_str("uuid=8b2f5d3e-0f2c-4f9a-9f6b-3f1a2c4d5e6f&user_id=1")
        .expect("the flat filter parses");

    assert!(filter.uuid.is_some());
    assert!(filter.user_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: UserInvitationFilter = serde_urlencoded::from_str(
      "uuid=8b2f5d3e-0f2c-4f9a-9f6b-3f1a2c4d5e6f,1c9e3a70-2b44-4d8e-8f11-77a0c2e5b913&user_id=1,2",
    )
    .expect("a list parses");

    assert_eq!(filter.uuid.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: UserInvitationFilter =
      serde_urlencoded::from_str("uuid=&user_id=").expect("an empty value parses");

    assert!(listed(filter.uuid.as_ref()).is_none());
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  #[test]
  fn an_unreadable_identifier_is_refused() {
    assert!(serde_urlencoded::from_str::<UserInvitationFilter>("uuid=not-a-uuid").is_err());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["expires_at", "userId", "createdAt", "id"] {
      assert!(UserInvitationFilter::field(spelling).is_none(), "{spelling}");
      assert!(UserInvitationFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(UserInvitationFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["uuid", "user_id", "created_at"] {
      assert!(UserInvitationFilter::sort_column(key).is_some(), "{key}");
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
      .compile::<UserInvitationFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"user_id","op":"in","value":[1,2]},
      {"type":"field","key":"uuid","op":"eq","value":"0195b0a1-0000-7000-8000-000000000000"}
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
#[cfg(all(test, feature = "openapi"))]
mod schema_tests {
  use sea_orm_crud::openapi::{assert_params_required_matches_serde, assert_required_matches_serde};
  use serde_json::json;

  use super::*;

  /// Both bodies carry nothing today, and the check is what keeps a field from being added to one
  /// of them without being answered for.
  #[test]
  fn the_two_bodies_that_carry_nothing_ask_for_nothing() {
    assert_required_matches_serde::<CreateUserInvitation>(json!({}));
    assert_required_matches_serde::<UpdateUserInvitation>(json!({}));
  }

  #[test]
  fn every_parameter_the_list_names_is_one_the_reader_may_be_left_without() {
    assert_params_required_matches_serde::<UserInvitationFilter>(&[
      ("user_id", "1,2"),
      ("uuid", "0195b0a1-0000-7000-8000-000000000000"),
    ]);
  }
}

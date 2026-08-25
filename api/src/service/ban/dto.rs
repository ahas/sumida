use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bans::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, ExprTrait, SimpleExpr};
use sea_orm::{Condition, DeriveIntoActiveModel, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

/// The banned user, as the list query joins them.
fn banned_user(column: entity::users::Column) -> SimpleExpr {
  Expr::col((Alias::new("u"), column)).into()
}

#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
pub struct BanFilter {
  /// Text looked for in the banned account's username and full name at once.
  pub search:     Option<String>,
  /// One banned account or several.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub user_id:    Option<OneOrMany<i32>>,
  /// Text looked for anywhere in the banned account's username.
  pub username:   Option<String>,
  /// Text looked for anywhere in the banned account's full name.
  pub name:       Option<String>,
  /// Text looked for anywhere in the banned address.
  pub ip_address: Option<String>,
  /// Text looked for anywhere in the reason.
  pub reason:     Option<String>,
  /// Text looked for anywhere in the note.
  pub note:       Option<String>,
}

impl BanFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(wrap_ilike(banned_user(entity::users::Column::Username), v))
            .add(wrap_ilike(banned_user(entity::users::Column::FullName), v)),
        )
      })
      .apply_if(listed(self.user_id.as_ref()), |q, v| {
        q.filter(match v {
          OneOrMany::One(id) => banned_user(entity::users::Column::Id).eq(*id),
          OneOrMany::Many(ids) => banned_user(entity::users::Column::Id).is_in(ids.clone()),
        })
      })
      .apply_if(self.username.as_ref(), |q, v| {
        q.filter(wrap_ilike(banned_user(entity::users::Column::Username), v))
      })
      .apply_if(self.name.as_ref(), |q, v| {
        q.filter(wrap_ilike(banned_user(entity::users::Column::FullName), v))
      })
      .apply_if(self.ip_address.as_ref(), |q, v| q.filter(wrap_ilike(Column::IpAddress.into_expr(), v)))
      .apply_if(self.reason.as_ref(), |q, v| q.filter(wrap_ilike(Column::Reason.into_expr(), v)))
      .apply_if(self.note.as_ref(), |q, v| q.filter(wrap_ilike(Column::Note.into_expr(), v)))
  }
}

/// What `/bans` accepts, declared once.
///
/// `search` widens the username and the full name into one predicate, so it reduces to no single
/// expression; the two keys that stand on those columns one at a time do reduce, and each says so.
const BAN_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_sort()
    .with_facet(),
  FieldSpec::new("username", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("ip_address", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::new("reason", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::new("note", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BanFilter {
  fn fields() -> &'static [FieldSpec] {
    BAN_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "user_id" => banned_user(entity::users::Column::Id),
      "username" => banned_user(entity::users::Column::Username),
      "name" => banned_user(entity::users::Column::FullName),
      "ip_address" => Column::IpAddress.into_simple_expr(),
      "reason" => Column::Reason.into_simple_expr(),
      "note" => Column::Note.into_simple_expr(),
      "created_at" => Column::CreatedAt.into_simple_expr(),
      // `search` spans the username and the full name, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

/// The account a ban names.
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sea_orm(entity = "entity::users::Entity", alias = "u", from_query_result)]
pub struct BanUser {
  pub id:        i32,
  pub username:  String,
  pub full_name: String,
}

/// The account that wrote a ban.
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sea_orm(entity = "entity::users::Entity", alias = "c", from_query_result)]
pub struct BanCreatedBy {
  pub id:        i32,
  pub username:  String,
  pub full_name: String,
}

/// One ban on a page of them. The reason and the note are not here; read the ban to see them.
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BanListItem {
  pub id:         i32,
  /// Null on a ban that names only an address.
  #[sea_orm(nested, alias = "u")]
  pub user:       Option<BanUser>,
  /// Null on a ban that names only an account.
  pub ip_address: Option<String>,
  pub created_at: DateTimeUtc,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanCreatedBy>,
  // pub reason: String,
  /// Null on a ban that stands until it is lifted.
  pub expires_at: Option<DateTimeUtc>,
  /// When it was lifted, or null while it still stands.
  pub lifted_at:  Option<DateTimeUtc>,
  // pub note: Option<String>,
}

/// One ban, read on its own.
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBan {
  pub id:         i32,
  /// Null on a ban that names only an address.
  #[sea_orm(nested, alias = "u")]
  pub user:       Option<BanUser>,
  /// Null on a ban that names only an account.
  pub ip_address: Option<String>,
  pub created_at: DateTimeUtc,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanUser>,
  /// What a person is told, if anything was written down.
  pub reason:     Option<String>,
  /// Null on a ban that stands until it is lifted.
  pub expires_at: Option<DateTimeUtc>,
  /// When it was lifted, or null while it still stands.
  pub lifted_at:  Option<DateTimeUtc>,
  /// What the operators are told, which is not what the banned person is told.
  pub note:       Option<String>,
}

impl IntoResponse for GetBan {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

/// What shutting one account out takes, where the path already says which account.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BanSettings {
  /// What the banned person is told.
  #[serde(default)]
  pub reason:     Nullable<String>,
  /// When it stops. Null shuts the account out until the ban is lifted.
  #[cfg_attr(feature = "openapi", schema(required = false, value_type = Option<DateTimeUtc>))]
  pub expires_at: Nullable<DateTimeUtc>,
  /// What the operators are told, which is not what the banned person is told.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub note:       Optional<String>,
}

/// What writing a ban takes.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreateBan {
  /// The account to shut out. Null on a ban that names only an address.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub user_id:    Nullable<i32>,
  /// The address to shut out. Null on a ban that names only an account.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub ip_address: Nullable<String>,
  /// What the banned person is told.
  #[serde(default)]
  pub reason:     Nullable<String>,
  /// When it stops. Null shuts the target out until the ban is lifted.
  #[cfg_attr(feature = "openapi", schema(required = false, value_type = Option<DateTimeUtc>))]
  pub expires_at: Nullable<DateTimeUtc>,
  /// What the operators are told, which is not what the banned person is told.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub note:       Optional<String>,
}

/// The identifier of the ban that was written.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct BanCreated {
  pub id: i32,
}

impl IntoResponse for BanCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

/// What changing a ban takes.
///
/// Every property may be left out, but leaving one out does not always leave it alone. `userId`,
/// `ipAddress` and `expiresAt` are **cleared** by their absence — which for `expiresAt` turns the
/// ban into one that stands until it is lifted — while `reason` and `note` keep what is stored. To
/// keep any of the three, send it back.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct UpdateBan {
  /// The account shut out. Null, or left out, makes the ban name only an address.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub user_id:    Nullable<i32>,
  /// The address shut out. Null, or left out, makes the ban name only an account.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub ip_address: Nullable<String>,
  /// What the banned person is told.
  #[serde(default)]
  pub reason:     Nullable<String>,
  /// When it stops. A moment that has passed ends the ban; null, or left out, makes it stand until
  /// it is lifted.
  #[cfg_attr(feature = "openapi", schema(required = false, value_type = Option<DateTimeUtc>))]
  pub expires_at: Nullable<DateTimeUtc>,
  /// What the operators are told, which is not what the banned person is told.
  #[cfg_attr(feature = "openapi", schema(required = false))]
  pub note:       Optional<String>,
}

impl IntoResponse for UpdateBan {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanUpdated;

impl IntoResponse for BanUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanDeleted;

impl IntoResponse for BanDeleted {
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
    BanFilter::validate().expect("the ban filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "search=a&user_id=1&username=b&name=c&ip_address=d&reason=e&note=f";
    let filter: BanFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.user_id.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
    assert!(filter.ip_address.is_some());
    assert!(filter.reason.is_some());
    assert!(filter.note.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BanFilter = serde_urlencoded::from_str("user_id=1,2").expect("a list parses");
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BanFilter = serde_urlencoded::from_str("user_id=").expect("an empty value parses");
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  /// The read scope is one predicate emitted into the list statement — `user_id` narrows inside it
  /// rather than choosing it — so nothing is withheld on gate grounds. What stays unreachable is
  /// every key the resource never declared.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["fullName", "userId", "created_by", "expires_at", "lifted_at", "user_agent"] {
      assert!(BanFilter::field(spelling).is_none(), "{spelling}");
      assert!(BanFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BanFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["user_id", "username", "name", "created_at"] {
      assert!(BanFilter::sort_column(key).is_some(), "{key}");
    }
    for key in ["search", "ip_address", "reason", "note"] {
      assert!(BanFilter::sort_column(key).is_none(), "{key}");
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
      .compile::<BanFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"reason","op":"contains","value":"spam"},
      {"type":"field","key":"note","op":"contains","value":"spam"}
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

  #[test]
  fn what_writing_a_ban_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<CreateBan>(json!({
      "userId": 1,
      "ipAddress": "203.0.113.7",
      "reason": "abuse",
      "expiresAt": "2030-01-01T00:00:00+00:00",
      "note": "second offence",
    }));
  }

  #[test]
  fn what_changing_a_ban_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<UpdateBan>(json!({
      "userId": 1,
      "ipAddress": "203.0.113.7",
      "reason": "abuse",
      "expiresAt": "2030-01-01T00:00:00+00:00",
      "note": "second offence",
    }));
  }

  #[test]
  fn what_shutting_one_account_out_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<BanSettings>(json!({
      "reason": "abuse",
      "expiresAt": "2030-01-01T00:00:00+00:00",
      "note": "second offence",
    }));
  }

  #[test]
  fn every_parameter_the_list_names_is_one_the_reader_may_be_left_without() {
    assert_params_required_matches_serde::<BanFilter>(&[
      ("search", "kim"),
      ("user_id", "1,2"),
      ("username", "kim"),
      ("name", "Kim"),
      ("ip_address", "203.0.113"),
      ("reason", "abuse"),
      ("note", "second"),
    ]);
  }
}

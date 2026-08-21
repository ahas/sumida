use axum::Json;
use axum::response::IntoResponse;
use entity::access_logs::{AccessMethod, AccessResultCode, Column};
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, ExprTrait, SimpleExpr};
use sea_orm::{Condition, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Bounds, OneOrMany};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

/// The acting user, as the list query joins them. The projection renames these into the payload,
/// but a predicate and an `ORDER BY` have to name the joined column itself.
fn actor(column: entity::users::Column) -> SimpleExpr {
  Expr::col((Alias::new("user"), column)).into()
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AccessLogFilter {
  pub search:     Option<String>,
  pub user_id:    Option<OneOrMany<i32>>,
  pub username:   Option<String>,
  pub name:       Option<String>,
  /// The log's own columns. Everything above narrows by the account, which is no help on a page
  /// already pinned to one; these narrow by what the attempt was.
  pub code:       Option<OneOrMany<AccessResultCode>>,
  pub method:     Option<OneOrMany<AccessMethod>>,
  pub ip_address: Option<String>,
  pub user_agent: Option<String>,
  /// When the attempt was made. The first range parameter this crate carries — until now a date
  /// could only be ordered by, which answers "newest first" and never "that afternoon".
  ///
  /// Named as the wire names it. The key was already spoken as `createdAt` because it is what a
  /// caller orders by, and a key cannot be one word for sorting and another for filtering.
  #[serde(rename = "createdAt")]
  pub created_at: Option<Bounds<DateTimeWithTimeZone>>,
}

impl AccessLogFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(actor(entity::users::Column::Id).cast_as("text").eq(v))
            .add(wrap_ilike(actor(entity::users::Column::Username), v))
            .add(wrap_ilike(actor(entity::users::Column::FullName), v)),
        )
      })
      .apply_if(listed(self.user_id.as_ref()), |q, v| {
        q.filter(match v {
          OneOrMany::One(id) => actor(entity::users::Column::Id).eq(*id),
          OneOrMany::Many(ids) => actor(entity::users::Column::Id).is_in(ids.clone()),
        })
      })
      .apply_if(self.username.as_ref(), |q, v| {
        q.filter(wrap_ilike(actor(entity::users::Column::Username), v))
      })
      .apply_if(self.name.as_ref(), |q, v| {
        q.filter(wrap_ilike(actor(entity::users::Column::FullName), v))
      })
      .apply_if(listed(self.code.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(entity::access_logs::Column::Code))
      })
      .apply_if(listed(self.method.as_ref()), |q, v| {
        q.filter(v.into_simple_expr(entity::access_logs::Column::Method))
      })
      .apply_if(self.ip_address.as_ref(), |q, v| {
        q.filter(wrap_ilike(entity::access_logs::Column::IpAddress.into_expr(), v))
      })
      .apply_if(self.user_agent.as_ref(), |q, v| {
        q.filter(wrap_ilike(entity::access_logs::Column::UserAgent.into_expr(), v))
      })
      // A range that names neither end narrows nothing, so it never reaches the query.
      .apply_if(self.created_at.as_ref().filter(|v| !v.is_empty()), |q, v| {
        q.filter(v.into_simple_expr(Column::CreatedAt))
      })
  }
}

/// What `/access-logs` accepts, declared once.
///
/// `search` widens the acting user's id, username and full name into one predicate, so it reduces
/// to no single expression; the three keys that stand on those columns one at a time do reduce.
/// The operators a range notation can carry. A date and a number both read the same way.
const RANGE_OPS: &[Op] = &[Op::Eq, Op::Gte, Op::Lte, Op::Between];

const ACCESS_LOG_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_sort()
    .with_facet(),
  FieldSpec::new("username", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("code", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&[
      "success",
      "account-not-found",
      "password-mismatch",
      "account-inactive",
      "account-banned",
    ])
    .with_sort()
    .with_facet(),
  FieldSpec::new("method", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["password", "bearer", "api-key"])
    .with_sort()
    .with_facet(),
  FieldSpec::new("ip_address", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains])
    .with_sort()
    .with_facet(),
  FieldSpec::new("user_agent", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_facet(),
  FieldSpec::new("createdAt", FieldKind::DateTime, MatchStrategy::Comparison, RANGE_OPS).with_sort(),
];

impl FilterSchema for AccessLogFilter {
  fn fields() -> &'static [FieldSpec] {
    ACCESS_LOG_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "user_id" => actor(entity::users::Column::Id),
      "username" => actor(entity::users::Column::Username),
      "name" => actor(entity::users::Column::FullName),
      "code" => Column::Code.into_simple_expr(),
      "method" => Column::Method.into_simple_expr(),
      "ip_address" => Column::IpAddress.into_simple_expr(),
      "user_agent" => Column::UserAgent.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      // `search` spans three columns of the acting user, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "createdAt"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AccessLogUser {
  #[sea_orm(from_alias = "user_id")]
  pub id:        i32,
  #[sea_orm(from_alias = "user_username")]
  pub username:  String,
  #[sea_orm(from_alias = "user_full_name")]
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AccessLogListItem {
  pub id:     i32,
  /// Absent where the attempt names no account this installation holds: a sign-in for a username
  /// that does not exist is logged too, and an account deleted afterwards leaves its attempts
  /// behind. Requiring the account here made the whole list fail on the first such row.
  #[sea_orm(nested, alias = "user")]
  pub user:   Option<AccessLogUser>,
  pub method: AccessMethod,
  pub code:   AccessResultCode,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family:      String,
  pub os_major:       Option<String>,
  pub os_minor:       Option<String>,
  pub os_patch:       Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand:  Option<String>,
  pub device_model:  Option<String>,

  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetAccessLog {
  pub id:     i32,
  /// Absent where the attempt names no account this installation holds: a sign-in for a username
  /// that does not exist is logged too, and an account deleted afterwards leaves its attempts
  /// behind. Requiring the account here made the whole list fail on the first such row.
  #[sea_orm(nested, alias = "user")]
  pub user:   Option<AccessLogUser>,
  pub method: AccessMethod,
  pub code:   AccessResultCode,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family:      String,
  pub os_major:       Option<String>,
  pub os_minor:       Option<String>,
  pub os_patch:       Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand:  Option<String>,
  pub device_model:  Option<String>,

  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetAccessLog {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessLogDeleted;

impl IntoResponse for AccessLogDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use entity::access_logs::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    AccessLogFilter::validate().expect("the access log filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: AccessLogFilter =
      serde_urlencoded::from_str("search=a&user_id=1&username=b&name=c").expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.user_id.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: AccessLogFilter = serde_urlencoded::from_str("user_id=1,2").expect("a list parses");
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: AccessLogFilter = serde_urlencoded::from_str("user_id=").expect("an empty value parses");
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  /// The read scope is one predicate emitted into the list statement — `user_id` narrows inside it
  /// rather than choosing it — so nothing is withheld on gate grounds. Every key the resource never
  /// declared stays unreachable, the earlier camelCase sort spelling included.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["userId", "created_at", "ipAddress", "userAgent", "os_family", "device_family"] {
      assert!(AccessLogFilter::field(spelling).is_none(), "{spelling}");
      assert!(AccessLogFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(AccessLogFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["user_id", "username", "name", "createdAt"] {
      assert!(AccessLogFilter::sort_column(key).is_some(), "{key}");
    }
    assert!(AccessLogFilter::sort_column("search").is_none());
    assert!(AccessLogFilter::column("search").is_none());
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
      .compile::<AccessLogFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"ip_address","op":"contains","value":"10."},
      {"type":"field","key":"method","op":"eq","value":"api-key"}
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

  /// The column stores text, so the bound value needs no cast. A key backed by a database enum
  /// names its type in the declaration instead, and the cast lands in the predicate.
  #[test]
  fn a_text_backed_enum_binds_without_a_cast() {
    use sea_orm::QueryFilter;

    let predicate = where_of(
      Entity::find().filter(compiled(r#"{"type":"field","key":"method","op":"eq","value":"api-key"}"#)),
    );

    assert!(AccessLogFilter::enum_type("method").is_none());
    assert!(predicate.contains("'api-key'"), "{predicate}");
    assert!(!predicate.contains("CAST("), "{predicate}");
  }
}

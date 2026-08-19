use axum::Json;
use axum::response::IntoResponse;
use entity::access_logs::{AccessMethod, AccessResultCode, Column};
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, ExprTrait, SimpleExpr};
use sea_orm::{Condition, FromQueryResult, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::OneOrMany;
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
  pub search: Option<String>,
  pub user_id: Option<OneOrMany<i32>>,
  pub username: Option<String>,
  pub name: Option<String>,
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
  }
}

/// What `/access-logs` accepts, declared once.
///
/// `search` widens the acting user's id, username and full name into one predicate, so it reduces
/// to no single expression; the three keys that stand on those columns one at a time do reduce.
const ACCESS_LOG_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("search", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_sort(),
  FieldSpec::new("username", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
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
  pub id: i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AccessLogListItem {
  pub id: i32,
  #[sea_orm(nested, alias = "user")]
  pub user: AccessLogUser,
  pub method: AccessMethod,
  pub code: AccessResultCode,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family: String,
  pub os_major: Option<String>,
  pub os_minor: Option<String>,
  pub os_patch: Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand: Option<String>,
  pub device_model: Option<String>,

  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetAccessLog {
  pub id: i32,
  #[sea_orm(nested, alias = "user")]
  pub user: AccessLogUser,
  pub method: AccessMethod,
  pub code: AccessResultCode,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family: String,
  pub os_major: Option<String>,
  pub os_minor: Option<String>,
  pub os_patch: Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand: Option<String>,
  pub device_model: Option<String>,

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
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    AccessLogFilter::validate().expect("the access log filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: AccessLogFilter = serde_urlencoded::from_str("search=a&user_id=1&username=b&name=c")
      .expect("the flat filter parses");

    assert!(filter.search.is_some());
    assert!(filter.user_id.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: AccessLogFilter =
      serde_urlencoded::from_str("user_id=1,2").expect("a list parses");
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: AccessLogFilter =
      serde_urlencoded::from_str("user_id=").expect("an empty value parses");
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  /// The read scope is one predicate emitted into the list statement — `user_id` narrows inside it
  /// rather than choosing it — so nothing is withheld on gate grounds. Every key the resource never
  /// declared stays unreachable, the earlier camelCase sort spelling included.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["userId", "ip_address", "user_agent", "method", "code", "created_at"] {
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
}

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
pub struct BanFilter {
  pub search:     Option<String>,
  pub user_id:    Option<OneOrMany<i32>>,
  pub username:   Option<String>,
  pub name:       Option<String>,
  pub ip_address: Option<String>,
  pub reason:     Option<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::users::Entity", alias = "u", from_query_result)]
pub struct BanUser {
  pub id:        i32,
  pub username:  String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::users::Entity", alias = "c", from_query_result)]
pub struct BanCreatedBy {
  pub id:        i32,
  pub username:  String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BanListItem {
  pub id:         i32,
  #[sea_orm(nested, alias = "u")]
  pub user:       Option<BanUser>,
  pub ip_address: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanCreatedBy>,
  // pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at:  Option<DateTimeWithTimeZone>,
  // pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBan {
  pub id:         i32,
  #[sea_orm(nested, alias = "u")]
  pub user:       Option<BanUser>,
  pub ip_address: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanUser>,
  pub reason:     Option<String>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at:  Option<DateTimeWithTimeZone>,
  pub note:       Option<String>,
}

impl IntoResponse for GetBan {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct BanSettings {
  #[serde(default)]
  pub reason:     Nullable<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note:       Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBan {
  pub user_id:    Nullable<i32>,
  pub ip_address: Nullable<String>,
  #[serde(default)]
  pub reason:     Nullable<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note:       Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BanCreated {
  pub id: i32,
}

impl IntoResponse for BanCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBan {
  pub user_id:    Nullable<i32>,
  pub ip_address: Nullable<String>,
  #[serde(default)]
  pub reason:     Nullable<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
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
  use super::*;

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
}

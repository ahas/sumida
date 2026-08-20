use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::api_keys::*;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, ExprTrait, SimpleExpr};
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::grants::ApForUser;
use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

/// The key's holder, as the list query joins them. The projection renames these into the payload,
/// but a predicate and an `ORDER BY` have to name the joined column itself.
fn holder(column: entity::users::Column) -> SimpleExpr {
  Expr::col((Alias::new("user"), column)).into()
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ApiKeyFilter {
  pub key:      Option<String>,
  pub user_id:  Option<OneOrMany<i32>>,
  pub username: Option<String>,
  pub name:     Option<String>,
}

impl ApiKeyFilter {
  pub fn apply<E>(&self, query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    use entity::api_keys::Column;

    query
      .apply_if(self.key.as_ref(), |q, v| q.filter(wrap_ilike(Column::Key.into_expr(), v)))
      .apply_if(listed(self.user_id.as_ref()), |q, v| {
        q.filter(match v {
          OneOrMany::One(id) => holder(entity::users::Column::Id).eq(*id),
          OneOrMany::Many(ids) => holder(entity::users::Column::Id).is_in(ids.clone()),
        })
      })
      .apply_if(self.username.as_ref(), |q, v| {
        q.filter(wrap_ilike(holder(entity::users::Column::Username), v))
      })
      .apply_if(self.name.as_ref(), |q, v| {
        q.filter(wrap_ilike(holder(entity::users::Column::FullName), v))
      })
  }
}

/// What `/api-keys` accepts, declared once.
const API_KEY_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("key", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("user_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/users", "username")
    .with_facet(),
  FieldSpec::new("username", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
];

impl FilterSchema for ApiKeyFilter {
  fn fields() -> &'static [FieldSpec] {
    API_KEY_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "key" => Column::Key.into_simple_expr(),
      "user_id" => holder(entity::users::Column::Id),
      "username" => holder(entity::users::Column::Username),
      "name" => holder(entity::users::Column::FullName),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "key"
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "entity::users::Entity")]
pub struct ApiKeyUser {
  #[sea_orm(from_alias = "user_id")]
  pub id:       i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListItem {
  pub key:        String,
  pub key_prefix: Option<String>,
  #[sea_orm(nested, alias = "user")]
  pub user:       ApiKeyUser,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_secured: bool,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetApiKey {
  pub key:             String,
  pub key_prefix:      Option<String>,
  #[sea_orm(nested, alias = "user")]
  pub user:            ApiKeyUser,
  #[sea_orm(skip)]
  pub access_policies: Option<Vec<ApForUser>>,
  pub expires_at:      Option<DateTimeWithTimeZone>,
  pub is_secured:      bool,
  pub created_at:      DateTimeWithTimeZone,
}

impl IntoResponse for GetApiKey {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKey {
  pub user_id:         i32,
  pub expires_at:      Nullable<DateTimeWithTimeZone>,
  pub access_policies: Option<Vec<ApForUser>>,
  pub is_secured:      Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for CreateApiKey {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      user_id: Set(self.user_id),
      expires_at: self.expires_at.into_active_value(),
      is_secured: self.is_secured.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCreated {
  pub key: String,
}

impl IntoResponse for ApiKeyCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiKey {
  pub expires_at:      Nullable<DateTimeWithTimeZone>,
  pub access_policies: Option<Vec<ApForUser>>,
  pub is_secured:      Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for UpdateApiKey {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      expires_at: self.expires_at.into_active_value(),
      is_secured: self.is_secured.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyUpdated;

impl IntoResponse for ApiKeyUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDeleted;

impl IntoResponse for ApiKeyDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_declaration_is_sound() {
    ApiKeyFilter::validate().expect("the api key filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: ApiKeyFilter =
      serde_urlencoded::from_str("key=a&user_id=1&username=b&name=c").expect("the flat filter parses");

    assert!(filter.key.is_some());
    assert!(filter.user_id.is_some());
    assert!(filter.username.is_some());
    assert!(filter.name.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: ApiKeyFilter = serde_urlencoded::from_str("user_id=1,2").expect("a list parses");
    assert_eq!(filter.user_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: ApiKeyFilter = serde_urlencoded::from_str("user_id=").expect("an empty value parses");
    assert!(listed(filter.user_id.as_ref()).is_none());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable — the key hash
  /// prefix and the secured flag among them.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["key_prefix", "is_secured", "expires_at", "userId", "created_at"] {
      assert!(ApiKeyFilter::field(spelling).is_none(), "{spelling}");
      assert!(ApiKeyFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(ApiKeyFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["key", "username", "name"] {
      assert!(ApiKeyFilter::sort_column(key).is_some(), "{key}");
    }
    // Declared, but not sortable.
    assert!(ApiKeyFilter::sort_column("user_id").is_none());
  }
}

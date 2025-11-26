use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bans::*;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm::{Condition, DeriveIntoActiveModel, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BanFilter {
  pub search: Option<String>,
  pub user_id: Option<i32>,
  pub username: Option<String>,
  pub name: Option<String>,
  pub ip_address: Option<String>,
  pub reason: Option<String>,
  pub note: Option<String>,
}

impl BanFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let user_alias = Alias::new("u");

    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::Username)), v))
            .add(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::FullName)), v)),
        )
      })
      .apply_if(self.user_id.as_ref(), |q, v| {
        q.filter(Expr::col((user_alias.clone(), entity::users::Column::Id)).eq(*v))
      })
      .apply_if(self.username.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::Username)), v))
      })
      .apply_if(self.name.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::FullName)), v))
      })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::users::Entity", alias = "u", from_query_result)]
pub struct BanUser {
  pub id: i32,
  pub username: String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::users::Entity", alias = "c", from_query_result)]
pub struct BanCreatedBy {
  pub id: i32,
  pub username: String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BanListItem {
  pub id: i32,
  #[sea_orm(nested, alias = "u")]
  pub user: Option<BanUser>,
  pub ip_address: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanCreatedBy>,
  // pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  // pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBan {
  pub id: i32,
  #[sea_orm(nested, alias = "u")]
  pub user: Option<BanUser>,
  pub ip_address: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "c")]
  pub created_by: Option<BanUser>,
  pub reason: Option<String>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  pub note: Option<String>,
}

impl IntoResponse for GetBan {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBan {
  pub user_id: Nullable<i32>,
  pub ip_address: Nullable<String>,
  #[serde(default)]
  pub reason: Nullable<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note: Optional<String>,
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
  pub user_id: Nullable<i32>,
  pub ip_address: Nullable<String>,
  #[serde(default)]
  pub reason: Nullable<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note: Optional<String>,
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

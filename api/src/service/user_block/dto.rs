use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::Condition;
use sea_orm::DeriveIntoActiveModel;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use entity::user_blocks::*;

use crate::util::crud::wrap_ilike;

#[derive(Deserialize)]
pub struct UserBlockFilter {
  pub search: Option<String>,
  pub username: Option<String>,
  pub name: Option<String>,
  pub ip_address: Option<String>,
  pub reason: Option<String>,
  pub note: Option<String>,
}

impl UserBlockFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let user_alias = Alias::new("u");

    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::Username)), &v))
            .add(wrap_ilike(Expr::col((user_alias.clone(), entity::users::Column::FullName)), &v)),
        )
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
pub struct UserBlockUser {
  pub id: i32,
  pub username: String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::users::Entity", alias = "b", from_query_result)]
pub struct UserBlockBlockedBy {
  pub id: i32,
  pub username: String,
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct UserBlockListItem {
  pub id: i32,
  #[sea_orm(nested, alias = "u")]
  pub user: Option<UserBlockUser>,
  pub ip_address: Option<String>,
  pub blocked_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "b")]
  pub blocked_by: Option<UserBlockBlockedBy>,
  // pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  // pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetUserBlock {
  pub id: i32,
  #[sea_orm(nested, alias = "u")]
  pub user: Option<UserBlockUser>,
  pub ip_address: Option<String>,
  pub blocked_at: DateTimeWithTimeZone,
  #[sea_orm(nested, alias = "b")]
  pub blocked_by: Option<UserBlockUser>,
  pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  pub note: Option<String>,
}

impl IntoResponse for GetUserBlock {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserBlock {
  pub user_id: Nullable<i32>,
  pub ip_address: Nullable<String>,
  pub reason: Optional<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBlockCreated {
  pub id: i32,
}

impl IntoResponse for UserBlockCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserBlock {
  pub user_id: Nullable<i32>,
  pub ip_address: Nullable<String>,
  pub reason: Optional<String>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub note: Optional<String>,
}

impl IntoResponse for UpdateUserBlock {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBlockUpdated;

impl IntoResponse for UserBlockUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBlockDeleted;

impl IntoResponse for UserBlockDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

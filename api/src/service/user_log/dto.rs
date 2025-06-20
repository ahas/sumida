use axum::Json;
use axum::response::IntoResponse;
use entity::user_logs::{SignInCode, SignInMethod};
use sea_orm::{FromQueryResult, QueryTrait, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::crud::wrap_ilike_col;

#[derive(Deserialize)]
pub struct UserLogFilter {
  pub search: Option<String>,
  pub username: Option<String>,
}

impl UserLogFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::user_logs::Column;

    query.apply_if(self.username.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::User, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserLogUser {
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub full_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserLogListItem {
  pub id: i32,
  #[sea_orm(nested, alias = "user")]
  pub user: UserLogUser,
  pub method: SignInMethod,
  pub code: SignInCode,

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
pub struct GetUserLog {
  pub id: i32,
  pub user: String,
  pub method: SignInMethod,
  pub code: SignInCode,

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

impl IntoResponse for GetUserLog {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLogDeleted;

impl IntoResponse for UserLogDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

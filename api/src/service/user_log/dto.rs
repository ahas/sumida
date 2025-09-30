use axum::Json;
use axum::response::IntoResponse;
use entity::user_logs::{LoginCode, LoginMethod};
use sea_orm::{Condition, FromQueryResult, IntoSimpleExpr, QueryTrait, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UserLogFilter {
  pub search: Option<String>,
  pub user_id: Option<i32>,
  pub username: Option<String>,
  pub name: Option<String>,
}

impl UserLogFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let col_user_id = Expr::col("user_id");
    let col_username = Expr::col("user_username");
    let col_full_name = Expr::col("user_full_name");

    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any() //
            .add(col_user_id.clone().into_simple_expr().cast_as("text").eq(v))
            .add(wrap_ilike(col_username.clone(), v))
            .add(wrap_ilike(col_full_name.clone(), v)),
        )
      })
      .apply_if(self.user_id, |q, v| q.filter(col_user_id.into_simple_expr().eq(v)))
      .apply_if(self.username.as_ref(), |q, v| q.filter(wrap_ilike(col_username, v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(col_full_name, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserLogUser {
  #[sea_orm(from_alias = "user_id")]
  pub id: i32,
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
  pub method: LoginMethod,
  pub code: LoginCode,

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
  #[sea_orm(nested, alias = "user")]
  pub user: UserLogUser,
  pub method: LoginMethod,
  pub code: LoginCode,

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

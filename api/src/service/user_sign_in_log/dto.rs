use axum::Json;
use axum::response::IntoResponse;
use sea_orm::{prelude::*, FromQueryResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

use entity::user_sign_in_logs::*;

#[derive(Deserialize)]
pub struct UserSignInLogFilter {
  pub search: Option<String>,
  pub username: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserSignInLogListItem {
  pub id: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUserSignInLog {
  pub id: i32,
}

impl IntoResponse for GetUserSignInLog {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSignInLogDeleted;

impl IntoResponse for UserSignInLogDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

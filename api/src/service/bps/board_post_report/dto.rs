use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostReportFilter {
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportListItem {
  pub id: i32,
  pub post_id: i32,
  pub user_id: i32,
  pub reason: String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPostReport {
  pub id: i32,
  pub post_id: i32,
  pub user_id: i32,
  pub reason: String,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPostReport {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPostReport {
  pub post_id: i32,
  pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostReportCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReportDeleted;

impl IntoResponse for BoardPostReportDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

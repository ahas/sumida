use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::prelude::*;
use sea_orm::FromQueryResult;
use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// 시스템 설정 단건 응답 — camelCase 계약(소비 서비스 공용).
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettingItem {
  pub key:         String,
  pub value:       JsonValue,
  pub data_type:   SystemSettingDataType,
  pub label:       String,
  pub description: Option<String>,
  pub min:         Option<JsonValue>,
  pub max:         Option<JsonValue>,
  pub is_editable: bool,
  pub updated_at:  DateTimeWithTimeZone,
  pub updated_by:  Option<i32>,
}

impl IntoResponse for SystemSettingItem {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
pub struct UpdateSystemSettingRequest {
  pub value: JsonValue,
}

/// 갱신 성공 — 본문 없는 200.
pub struct UpdateSystemSettingResult;

impl IntoResponse for UpdateSystemSettingResult {
  fn into_response(self) -> axum::response::Response {
    StatusCode::OK.into_response()
  }
}

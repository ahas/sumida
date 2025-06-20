use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::api_keys::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm::{QueryTrait, prelude::*};
use sea_orm_crud::Nullable;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::crud::wrap_ilike_col;

#[derive(Deserialize)]
pub struct ApiKeyFilter {
  pub key: Option<String>,
  pub user: Option<String>,
}

impl ApiKeyFilter {
  pub fn apply<E>(&self, query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    use entity::api_keys::Column;

    query
      .apply_if(self.key.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Key, v)))
      .apply_if(self.user.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::User, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListItem {
  pub key: String,
  pub user: String,
  pub ability: Option<serde_json::Value>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetApiKey {
  pub key: String,
  pub user: String,
  pub ability: Option<serde_json::Value>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetApiKey {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKey {
  pub user: String,
  #[serde(default)]
  pub ability: Nullable<serde_json::Value>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
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

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiKey {
  #[serde(default)]
  pub ability: Nullable<serde_json::Value>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
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

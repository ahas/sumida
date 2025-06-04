use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::entity::mail_forms::*;

#[derive(Deserialize)]
pub struct MailFormFilter {
  pub sender: Option<String>,
  pub subject: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MailFormListItem {
  pub kind: String,
  pub sender: String,
  pub subject: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetMailForm {
  pub kind: String,
  pub sender: String,
  pub subject: String,
  pub content: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetMailForm {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateMailForm {
  pub kind: String,
  pub sender: String,
  pub subject: String,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormCreated {
  pub kind: String,
}

impl IntoResponse for MailFormCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMailForm {
  pub kind: Optional<String>,
  pub sender: Optional<String>,
  pub subject: Optional<String>,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormUpdated;

impl IntoResponse for MailFormUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFormDeleted;

impl IntoResponse for MailFormDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

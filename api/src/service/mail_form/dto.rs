use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::mail_forms::*;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult, QueryTrait};
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MailFormFilter {
  pub sender:  Option<String>,
  pub subject: Option<String>,
}

impl MailFormFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.sender.as_ref(), |q, v| q.filter(wrap_ilike(Column::Sender.into_expr(), v)))
      .apply_if(self.subject.as_ref(), |q, v| q.filter(wrap_ilike(Column::Subject.into_expr(), v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MailFormListItem {
  pub kind:    String,
  pub sender:  String,
  pub subject: String,

  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetMailForm {
  pub kind:    String,
  pub sender:  String,
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
  pub kind:    String,
  pub sender:  String,
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
  pub kind:    Optional<String>,
  pub sender:  Optional<String>,
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

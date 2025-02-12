use crate::entity::user_invitations::*;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct UserInvitationFilter {
  pub username: Option<String>,
  pub uuid: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationListItem {
  pub username: String,
  pub uuid: Uuid,

  pub expired_at: DateTimeWithTimeZone,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUserInvitation {
  pub username: String,
  pub uuid: Uuid,

  pub expired_at: DateTimeWithTimeZone,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetUserInvitation {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserInvitation {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationCreated {
  pub uuid: Uuid,
  pub expired_at: DateTimeWithTimeZone,
}

impl IntoResponse for UserInvitationCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserInvitation {}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationUpdated {
  pub uuid: Uuid,
  pub expired_at: DateTimeWithTimeZone,
}

impl IntoResponse for UserInvitationUpdated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationDeleted;

impl IntoResponse for UserInvitationDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

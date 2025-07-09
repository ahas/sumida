use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm::{QueryTrait, prelude::*};
use serde::{Deserialize, Serialize};
use serde_json::json;

use entity::user_invitations::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationFilter {
  pub user_id: Option<i32>,
  pub uuid: Option<Uuid>,
}

impl UserInvitationFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::user_invitations::Column;

    query.apply_if(self.uuid, |q, v| q.filter(Column::Uuid.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserInvitationListItem {
  pub user_id: i32,
  pub uuid: Uuid,

  pub expires_at: DateTimeWithTimeZone,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUserInvitation {
  pub user_id: i32,
  pub uuid: Uuid,

  pub expires_at: DateTimeWithTimeZone,
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
  pub expires_at: DateTimeWithTimeZone,
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
  pub expires_at: DateTimeWithTimeZone,
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

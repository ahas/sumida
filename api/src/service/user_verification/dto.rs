use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::user_verifications::*;
use sea_orm::DeriveIntoActiveModel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct SendUserVerification {
  pub user_id: i32,
  pub kind:    String,
  pub method:  String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserVerificationSent;

impl IntoResponse for UserVerificationSent {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

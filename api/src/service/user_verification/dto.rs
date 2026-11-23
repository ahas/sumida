use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::user_verifications::*;
use sea_orm::DeriveIntoActiveModel;
use serde::{Deserialize, Serialize};

/// What asking for a verification code takes.
#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SendUserVerification {
  /// The account the code is for.
  pub user_id: i32,
  /// What the code will be spent on. It has to be named again where the code is spent, and a code
  /// asked for under one kind is no good under another. `password-change` is what
  /// `POST /users/change-password` reads.
  pub kind:    String,
  /// The channel to send it over. `email` is the only one, and only in a deployment built with
  /// mail; a code asked for over anything else, or where there is no mail, is never made at all.
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

/// What the document says may be left out, put to the reader that decides it.
#[cfg(all(test, feature = "openapi"))]
mod schema_tests {
  use sea_orm_crud::openapi::assert_required_matches_serde;
  use serde_json::json;

  use super::*;

  #[test]
  fn what_asking_for_a_code_takes_is_what_the_document_asks_for() {
    assert_required_matches_serde::<SendUserVerification>(json!({
      "userId": 1,
      "kind": "password-change",
      "method": "email",
    }));
  }
}

use crate::entity::users::*;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::prelude::*;
use sea_orm::FromQueryResult;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::session::{OrganizationData, RoleData};

#[derive(Deserialize)]
pub struct UserFilter {
  pub username: Option<String>,
  pub name: Option<String>,
  pub organization: Option<i32>,
  pub role: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct UserListItem {
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  pub organization: i32,
  pub role: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetUser {
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  pub organization: i32,
  pub role: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetUser {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateUser {
  pub username: String,
  pub password: String,
  pub full_name: Nullable<String>,
  pub email: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization: i32,
  pub role: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreated {
  pub username: String,
}

impl IntoResponse for UserCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUser {
  pub password: Optional<String>,
  pub email: Nullable<String>,
  pub full_name: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization: Optional<i32>,
  pub role: Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdated;

impl IntoResponse for UserUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDeleted;

impl IntoResponse for UserDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInUser {
  pub username: String,
  pub password: String,
  pub remember_me: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
  pub username: String,
  pub full_name: String,
  pub organization: OrganizationData,
  pub role: RoleData,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
}

impl IntoResponse for UserProfile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
  pub username: String,
  pub password: String,
  pub code: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserActivation {
  pub username: String,
  pub password: String,
  pub uuid: Uuid,
}

use axum::{Json, response::IntoResponse};
use entity::users::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult, prelude::DateTimeWithTimeZone};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::service::user::dto::{UserOrganization, UserRole};

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct MeSession {
  pub id: i32,
  pub username: String,
  pub full_name: Option<String>,
  pub email: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  #[sea_orm(from_alias = "is_blocked")]
  pub is_blocked: bool,
  #[sea_orm(nested)]
  pub organization: UserOrganization,
  #[sea_orm(nested)]
  pub role: UserRole,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for MeSession {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeProfile {
  pub username: String,
  pub full_name: String,
  pub organization: UserOrganization,
  pub role: UserRole,
  pub ability: JsonValue,
  pub is_admin: bool,
  pub is_authorized: bool,
  pub first_seen_at: DateTimeWithTimeZone,
  pub last_seen_at: DateTimeWithTimeZone,
}

impl From<core::session::UserProfile> for MeProfile {
  fn from(value: core::session::UserProfile) -> Self {
    Self {
      username: value.username.clone(),
      full_name: value.full_name.clone(),
      organization: UserOrganization::from(value.organization.clone()),
      role: UserRole::from(value.role.clone()),
      ability: value.ability.clone(),
      is_admin: value.is_admin,
      is_authorized: value.is_authorized,
      first_seen_at: value.first_seen_at,
      last_seen_at: value.last_seen_at,
    }
  }
}

impl IntoResponse for MeProfile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeProfile {
  pub username: Optional<String>,
  pub password: Optional<String>,
  pub email: Nullable<String>,
  pub full_name: Nullable<String>,
  pub phone: Nullable<String>,
  pub is_admin: Optional<bool>,
  pub is_enabled: Optional<bool>,
  pub organization_id: Optional<i32>,
  pub role_id: Optional<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeProfileUpdated;

impl IntoResponse for MeProfileUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

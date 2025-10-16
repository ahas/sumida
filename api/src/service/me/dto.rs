use axum::Json;
use axum::response::IntoResponse;
use entity::users::*;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::user::dto::{UserOrganization, UserRole};

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct Me {
  pub id:            i32,
  pub username:      String,
  pub full_name:     Option<String>,
  pub email:         Option<String>,
  pub phone:         Option<String>,
  pub is_admin:      bool,
  pub is_enabled:    bool,
  #[sea_orm(from_alias = "is_blocked")]
  pub is_blocked:    bool,
  #[sea_orm(nested)]
  pub organization:  UserOrganization,
  #[sea_orm(nested)]
  pub role:          UserRole,
  #[sea_orm(skip)]
  pub first_seen_at: DateTimeWithTimeZone,
  #[sea_orm(skip)]
  pub last_seen_at:  DateTimeWithTimeZone,
  pub created_at:    DateTimeWithTimeZone,
  pub updated_at:    DateTimeWithTimeZone,
}

impl IntoResponse for Me {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMe {
  pub username:  Optional<String>,
  pub password:  Optional<String>,
  pub email:     Nullable<String>,
  pub full_name: Nullable<String>,
  pub phone:     Nullable<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeUpdated;

impl IntoResponse for MeUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

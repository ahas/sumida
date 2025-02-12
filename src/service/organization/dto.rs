use crate::entity::organizations::*;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct OrganizationFilter {
  pub name: Option<String>,
  pub parent: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationListItem {
  pub id: i32,
  pub name: String,
  #[serde(default)]
  pub parent: Nullable<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetOrganization {
  pub id: i32,
  pub name: String,
  pub parent: Option<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetOrganization {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrganization {
  pub id: Optional<i32>,
  pub name: String,
  #[serde(default)]
  pub parent: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationCreated {
  pub id: i32,
}

impl IntoResponse for OrganizationCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrganization {
  pub name: String,
  #[serde(default)]
  pub parent: Nullable<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationUpdated;

impl IntoResponse for OrganizationUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationDeleted;

impl IntoResponse for OrganizationDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

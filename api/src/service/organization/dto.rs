use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::organizations::*;
use sea_orm::{DeriveIntoActiveModel, FromQueryResult};
use sea_orm::{QueryTrait, prelude::*};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::crud::wrap_ilike_col;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationFilter {
  pub name: Option<String>,
  pub parent_id: Option<i32>,
}

impl OrganizationFilter {
  pub fn apply<E>(&self, mut query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    query = query.apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Name, v)));

    query = if let Some(parent_id) = self.parent_id {
      query.filter(Column::ParentId.eq(parent_id))
    } else {
      query.filter(Column::ParentId.is_null())
    };

    query
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationListItem {
  pub id: i32,
  pub name: String,
  #[serde(default)]
  pub parent_id: Nullable<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetOrganization {
  pub id: i32,
  pub name: String,
  pub parent_id: Option<i32>,
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
  pub parent_id: Nullable<i32>,
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
  pub name: Optional<String>,
  #[serde(default)]
  pub parent_id: Nullable<i32>,
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

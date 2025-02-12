use crate::entity::roles::ActiveModel;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use sea_orm::prelude::*;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, Set};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

#[derive(Deserialize)]
pub struct RoleFilter {
  pub name: Option<String>,
  pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct RoleListItem {
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetRole {
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  pub ability: JsonValue,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetRole {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRole {
  pub id: Optional<i32>,
  pub name: String,
  #[serde(default)]
  pub description: Nullable<String>,
  pub ability: Optional<JsonValue>,
}

impl IntoActiveModel<ActiveModel> for CreateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
      ability: self.ability.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCreated {
  pub id: i32,
}

impl IntoResponse for RoleCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRole {
  pub name: Optional<String>,
  #[serde(default)]
  pub description: Nullable<String>,
  pub ability: Optional<JsonValue>,
}

impl IntoActiveModel<ActiveModel> for UpdateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
      ability: self.ability.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleUpdated;

impl IntoResponse for RoleUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDeleted;

impl IntoResponse for RoleDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

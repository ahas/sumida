use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::roles::ActiveModel;
use sea_orm::{
  DerivePartialModel, FromQueryResult, IntoActiveModel, IntoActiveValue, QueryTrait, Set, prelude::*,
};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::crud::wrap_ilike_col;

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::access_policies::Entity", alias = "ap_allow", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct RoleAccessPolicy {
  namespace: String,
  object: String,
  action: Vec<String>,
}

#[derive(Deserialize)]
pub struct RoleFilter {
  pub name: Option<String>,
  pub description: Option<String>,
}

impl RoleFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::roles::Column;

    query
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Name, v)))
      .apply_if(self.description.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Description, v)))
  }
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
  #[sea_orm(skip)]
  pub access_policies: Vec<RoleAccessPolicy>,
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
  pub access_policies: Vec<RoleAccessPolicy>,
}

impl IntoActiveModel<ActiveModel> for CreateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
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
  pub service_policies: Option<Vec<RoleAccessPolicy>>,
}

impl IntoActiveModel<ActiveModel> for UpdateRole {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
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

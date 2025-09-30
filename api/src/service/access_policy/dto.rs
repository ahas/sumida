use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::access_policies::ActiveModel;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, QueryTrait, Set, prelude::*};
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike_col;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AccessPolicyFilter {
  pub namespace: Option<String>,
  pub effect: Option<String>,
  pub object: Option<String>,
  pub user: Option<String>,
}

impl AccessPolicyFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::access_policies::Column;

    query
      .apply_if(self.namespace.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Namespace, v)))
      .apply_if(self.effect.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Effect, v)))
      .apply_if(self.object.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Object, v)))
      .apply_if(self.user.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::User, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AccessPolicyListItem {
  pub uuid: Uuid,
  pub namespace: String,
  pub effect: String,
  pub object: String,
  pub action: Vec<String>,
  pub user: String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetAccessPolicy {
  pub uuid: Uuid,
  pub namespace: String,
  pub effect: String,
  pub object: String,
  pub action: Vec<String>,
  pub user: String,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetAccessPolicy {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccessPolicy {
  pub uuid: Optional<Uuid>,
  pub namespace: String,
  pub effect: String,
  pub object: String,
  pub action: Vec<String>,
  pub user: String,
}

impl IntoActiveModel<ActiveModel> for CreateAccessPolicy {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      uuid: self.uuid.into_active_value(),
      namespace: Set(self.namespace),
      effect: Set(self.effect),
      object: Set(self.object),
      action: Set(self.action),
      user: Set(self.user),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPolicyCreated {
  pub uuid: Uuid,
}

impl IntoResponse for AccessPolicyCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccessPolicy {
  pub namespace: Optional<String>,
  pub effect: Optional<String>,
  pub object: Optional<String>,
  pub action: Optional<Vec<String>>,
  pub user: Optional<String>,
}

impl IntoActiveModel<ActiveModel> for UpdateAccessPolicy {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      namespace: self.namespace.into_active_value(),
      effect: self.effect.into_active_value(),
      object: self.object.into_active_value(),
      action: self.action.into_active_value(),
      user: self.user.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPolicyUpdated;

impl IntoResponse for AccessPolicyUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPolicyDeleted;

impl IntoResponse for AccessPolicyDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}


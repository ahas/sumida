use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::api_keys::*;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Alias;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue};
use sea_orm::{QueryTrait, prelude::*};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::ap::ApForUser;
use crate::util::db::{wrap_ilike, wrap_ilike_col};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ApiKeyFilter {
  pub key: Option<String>,
  pub user_id: Option<i32>,
  pub username: Option<String>,
  pub name: Option<String>,
}

impl ApiKeyFilter {
  pub fn apply<E>(&self, query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    use entity::api_keys::Column;

    query
      .apply_if(self.key.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Key, v)))
      .apply_if(self.user_id, |q, v| q.filter(Column::UserId.eq(v)))
      .apply_if(self.username.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col(Alias::new("user_username")), v))
      })
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Expr::col(Alias::new("user_full_name")), v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "entity::users::Entity")]
pub struct ApiKeyUser {
  #[sea_orm(from_alias = "user_id")]
  pub id: i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyListItem {
  pub key: String,
  #[sea_orm(nested, alias = "user")]
  pub user: ApiKeyUser,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_secured: bool,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetApiKey {
  pub key: String,
  #[sea_orm(nested, alias = "user")]
  pub user: ApiKeyUser,
  #[sea_orm(skip)]
  pub access_policies: Option<Vec<ApForUser>>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub is_secured: bool,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetApiKey {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKey {
  pub user_id: i32,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub access_policies: Option<Vec<ApForUser>>,
  pub is_secured: Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for CreateApiKey {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      user_id: Set(self.user_id),
      expires_at: self.expires_at.into_active_value(),
      is_secured: self.is_secured.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCreated {
  pub key: String,
}

impl IntoResponse for ApiKeyCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApiKey {
  pub expires_at: Nullable<DateTimeWithTimeZone>,
  pub access_policies: Option<Vec<ApForUser>>,
  pub is_secured: Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for UpdateApiKey {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      expires_at: self.expires_at.into_active_value(),
      is_secured: self.is_secured.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyUpdated;

impl IntoResponse for ApiKeyUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyDeleted;

impl IntoResponse for ApiKeyDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::boards::*;
use entity::prelude::*;
use sea_orm::ActiveValue::*;
use sea_orm::prelude::*;
use sea_orm::{Condition, EntityTrait, IntoActiveModel, IntoActiveValue, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFilter {
  pub search: Option<String>,
  pub name: Option<String>,
  pub group_id: Option<i32>,
}

impl BoardFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Name.into_expr(), v))
            .add(wrap_ilike(Column::Description.into_expr(), v)),
        )
      })
      .apply_if(self.name.as_ref(), |q, v| q.filter(Column::Name.eq(v)))
      .apply_if(self.group_id, |q, v| q.filter(Column::GroupId.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardGroups", from_query_result)]
pub struct BoardBoardGroup {
  pub id: i32,
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Boards", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardListItem {
  pub id: i32,
  #[sea_orm(nested)]
  pub group: Option<BoardBoardGroup>,
  pub name: String,
  pub description: Option<String>,
  pub order: i32,
  pub can_upload_file: bool,
  pub is_versioned: bool,
  pub is_enabled: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Boards", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoard {
  pub id: i32,
  #[sea_orm(nested)]
  pub group: Option<BoardBoardGroup>,
  pub name: String,
  pub description: Option<String>,
  pub order: i32,
  pub can_upload_file: bool,
  pub max_file_size: i64,
  pub max_file_count: i32,
  pub allowed_file_types: Option<JsonValue>,
  pub reaction_visibility: BoardPostRecordVisibility,
  pub viewer_visibility: BoardPostRecordVisibility,
  pub content_visibility: BoardPostRecordVisibility,
  pub is_versioned: bool,
  pub is_enabled: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoard {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoard {
  pub id: Optional<i32>,
  #[serde(default)]
  pub group_id: Nullable<i32>,
  #[serde(default)]
  pub topics: Nullable<Vec<String>>,
  pub name: String,
  #[serde(default)]
  pub description: Nullable<String>,
  pub order: Optional<i32>,
  pub can_upload_file: Optional<bool>,
  pub max_file_size: Optional<i64>,
  pub max_file_count: Optional<i32>,
  #[serde(default)]
  pub allowed_file_types: Nullable<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility: Optional<BoardPostRecordVisibility>,
  pub is_versioned: Optional<bool>,
  pub is_enabled: Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for CreateBoard {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      group_id: self.group_id.into_active_value(),
      name: Set(self.name),
      description: self.description.into_active_value(),
      order: self.order.into_active_value(),
      can_upload_file: self.can_upload_file.into_active_value(),
      max_file_size: self.max_file_size.into_active_value(),
      max_file_count: self.max_file_count.into_active_value(),
      allowed_file_types: self.allowed_file_types.into_active_value(),
      reaction_visibility: self.reaction_visibility.into_active_value(),
      viewer_visibility: self.viewer_visibility.into_active_value(),
      is_versioned: self.is_versioned.into_active_value(),
      is_enabled: self.is_enabled.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCreated {
  pub id: i32,
}

impl IntoResponse for BoardCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoard {
  #[serde(default)]
  pub group_id: Nullable<i32>,
  #[serde(default)]
  pub topics: Nullable<Vec<String>>,
  pub name: Optional<String>,
  pub description: Nullable<String>,
  pub order: Optional<i32>,
  pub can_upload_file: Optional<bool>,
  pub max_file_size: Optional<i64>,
  pub max_file_count: Optional<i32>,
  #[serde(default)]
  pub allowed_file_types: Nullable<JsonValue>,
  pub reaction_visibility: Optional<BoardPostRecordVisibility>,
  pub viewer_visibility: Optional<BoardPostRecordVisibility>,
  pub is_versioned: Optional<bool>,
  pub is_enabled: Optional<bool>,
}

impl IntoActiveModel<ActiveModel> for UpdateBoard {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      group_id: self.group_id.into_active_value(),
      name: self.name.into_active_value(),
      description: self.description.into_active_value(),
      order: self.order.into_active_value(),
      can_upload_file: self.can_upload_file.into_active_value(),
      max_file_size: self.max_file_size.into_active_value(),
      max_file_count: self.max_file_count.into_active_value(),
      allowed_file_types: self.allowed_file_types.into_active_value(),
      reaction_visibility: self.reaction_visibility.into_active_value(),
      viewer_visibility: self.viewer_visibility.into_active_value(),
      is_versioned: self.is_versioned.into_active_value(),
      is_enabled: self.is_enabled.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardUpdated;

impl IntoResponse for BoardUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardDeleted;

impl IntoResponse for BoardDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

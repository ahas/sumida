use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_post_reactions::ActiveModel;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, QueryTrait, Set, prelude::*};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike_col;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostReactionFilter {
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
  pub value: Option<String>,
}

impl BoardPostReactionFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::bs::board_post_reactions::Column;

    query
      .apply_if(self.post_id, |q, v| q.filter(Column::PostId.eq(v)))
      .apply_if(self.user_id, |q, v| q.filter(Column::UserId.eq(v)))
      .apply_if(self.value.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Value, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionListItem {
  pub id: i32,
  pub post_id: i32,
  pub user_id: Option<i32>,
  pub value: String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPostReaction {
  pub id: i32,
  pub post_id: i32,
  pub user_id: Option<i32>,
  pub value: String,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPostReaction {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPostReaction {
  pub id: Optional<i32>,
  pub post_id: i32,
  pub user_id: Nullable<i32>,
  pub value: String,
}

impl IntoActiveModel<ActiveModel> for CreateBoardPostReaction {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      post_id: Set(self.post_id),
      user_id: self.user_id.into_active_value(),
      value: Set(self.value),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostReactionCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPostReaction {
  pub post_id: Optional<i32>,
  pub user_id: Nullable<i32>,
  pub value: Optional<String>,
}

impl IntoActiveModel<ActiveModel> for UpdateBoardPostReaction {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      post_id: self.post_id.into_active_value(),
      user_id: self.user_id.into_active_value(),
      value: self.value.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionUpdated;

impl IntoResponse for BoardPostReactionUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostReactionDeleted;

impl IntoResponse for BoardPostReactionDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}


use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_post_views::ActiveModel;
use sea_orm::prelude::*;
use sea_orm::{FromQueryResult, IntoActiveModel, IntoActiveValue, QueryTrait, Set};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostViewFilter {
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
}

impl BoardPostViewFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    use entity::bs::board_post_views::Column;

    query
      .apply_if(self.post_id, |q, v| q.filter(Column::PostId.eq(v)))
      .apply_if(self.user_id, |q, v| q.filter(Column::UserId.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostViewListItem {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    Option<i32>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPostView {
  pub id:         i32,
  pub post_id:    i32,
  pub user_id:    Option<i32>,
  pub created_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPostView {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPostView {
  pub id:      Optional<i32>,
  pub post_id: i32,
  pub user_id: Nullable<i32>,
}

impl IntoActiveModel<ActiveModel> for CreateBoardPostView {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      id: self.id.into_active_value(),
      post_id: Set(self.post_id),
      user_id: self.user_id.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostViewCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostViewCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPostView {
  pub post_id: Optional<i32>,
  pub user_id: Nullable<i32>,
}

impl IntoActiveModel<ActiveModel> for UpdateBoardPostView {
  fn into_active_model(self) -> ActiveModel {
    ActiveModel {
      post_id: self.post_id.into_active_value(),
      user_id: self.user_id.into_active_value(),
      ..Default::default()
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostViewUpdated;

impl IntoResponse for BoardPostViewUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostViewDeleted;

impl IntoResponse for BoardPostViewDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

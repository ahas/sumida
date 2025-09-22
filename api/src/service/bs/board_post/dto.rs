use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_posts::*;
use sea_orm::Condition;
use sea_orm::DeriveIntoActiveModel;
use sea_orm::EntityTrait;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Deserialize)]
pub struct BoardPostFilter {
  pub search: Option<String>,
  pub title: Option<String>,
  pub board_id: Option<i32>,
}

impl BoardPostFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let alias = Alias::new("bp");
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Title)), v))
            .add(wrap_ilike(Expr::col((alias.clone(), Column::Content)), v)),
        )
      })
      .apply_if(self.title.as_ref(), |q, v| q.filter(Expr::col(Column::Title).eq(v)))
      .apply_if(self.board_id.as_ref(), |q, v| q.filter(Expr::col(Column::BoardId).eq(*v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  pub id: i32,
  pub board_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub title: String,
  pub content: Option<String>,
  pub status: String,
  pub published_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(skip)]
  pub view_count: i32,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPost {
  pub id: i32,
  pub board_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub title: String,
  pub content: Option<String>,
  pub status: String,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  pub auto_save_content: Option<String>,
  pub last_auto_save_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(skip)]
  pub view_count: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl From<Model> for GetBoardPost {
  fn from(model: Model) -> Self {
    Self {
      id: model.id,
      board_id: model.board_id,
      parent_id: model.parent_id,
      user_id: model.user_id,
      title: model.title,
      content: model.content,
      status: model.status,
      published_at: model.published_at,
      scheduled_at: model.scheduled_at,
      auto_save_content: model.auto_save_content,
      last_auto_save_at: model.last_auto_save_at,
      view_count: 0, // This should be fetched separately
      created_at: model.created_at,
      updated_at: model.updated_at,
    }
  }
}

impl IntoResponse for GetBoardPost {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPost {
  pub id: Optional<i32>,
  pub board_id: i32,
  pub parent_id: Nullable<i32>,
  pub user_id: Nullable<i32>,
  pub title: String,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostCreated {
  pub id: i32,
}

impl IntoResponse for BoardPostCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPost {
  pub title: Optional<String>,
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostUpdated;

impl IntoResponse for BoardPostUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostDeleted;

impl IntoResponse for BoardPostDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

// Post history management DTOs
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSavePost {
  pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSaveResult {
  pub success: bool,
}

impl IntoResponse for AutoSaveResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
  pub success: bool,
}

impl IntoResponse for PublishResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePost {
  pub scheduled_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleResult {
  pub success: bool,
}

impl IntoResponse for ScheduleResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_posts::*;
use sea_orm::ActiveValue::Set;
use sea_orm::Condition;
use sea_orm::EntityTrait;
use sea_orm::IntoActiveValue;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::service::prelude::BoardFileListItem;
use crate::util::db::wrap_ilike;

#[derive(Deserialize)]
pub struct BoardPostFilter {
  pub search: Option<String>,
  pub title: Option<String>,
  pub board_id: Option<i32>,
  pub parent_id: Option<i32>,
  pub kind: Option<String>,
  pub status: Option<String>,
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
      .apply_if(self.parent_id.as_ref(), |q, v| q.filter(Expr::col(Column::ParentId).eq(*v)))
      .apply_if(self.kind.as_ref(), |q, v| q.filter(Expr::col(Column::Kind).eq(v)))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Expr::col(Column::Status).eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  pub id: i32,
  pub board_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: i32,
  pub title: String,
  pub content: Option<JsonValue>,
  pub kind: String,
  pub status: String,
  pub tags: Option<Vec<String>>,
  pub published_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(skip)]
  pub view_count: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
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
  pub kind: BoardPostKind,
  pub status: BoardPostStatus,
  pub tags: Option<Vec<String>>,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  pub views: i32,
  pub reactions: JsonValue,
  #[sea_orm(skip)]
  pub attached_files: Vec<BoardFileListItem>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPost {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPost {
  pub id: Optional<i32>,
  pub board_id: i32,
  pub parent_id: Nullable<i32>,
  pub title: String,
  pub content: Nullable<String>,
  pub kind: Optional<BoardPostKind>,
  pub status: Optional<BoardPostStatus>,
  pub published_at: Nullable<DateTimeWithTimeZone>,
  pub tags: Nullable<Vec<String>>,
  pub attached_file_ids: Option<Vec<i32>>,
}

impl CreateBoardPost {
  pub fn into_active_model(self) -> entity::bs::board_posts::ActiveModel {
    entity::bs::board_posts::ActiveModel {
      id: self.id.into_active_value(),
      board_id: Set(self.board_id),
      parent_id: self.parent_id.into_active_value(),
      title: Set(self.title),
      content: self.content.into_active_value(),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardPost {
  pub title: Optional<String>,
  pub content: Nullable<String>,
  pub kind: Optional<BoardPostKind>,
  pub status: Optional<BoardPostStatus>,
  pub published_at: Nullable<DateTimeWithTimeZone>,
  pub tags: Nullable<Vec<String>>,
  pub attached_file_ids: Option<Vec<i32>>,
}

impl UpdateBoardPost {
  pub fn into_active_model(self) -> entity::bs::board_posts::ActiveModel {
    entity::bs::board_posts::ActiveModel {
      title: self.title.into_active_value(),
      content: self.content.into_active_value(),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }
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
  pub content: JsonValue,
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

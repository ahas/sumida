use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_posts::*;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm::{Condition, EntityTrait, FromQueryResult, IntoActiveValue, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::service::prelude::BoardFileListItem;
use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostFilter {
  pub search:      Option<String>,
  pub title:       Option<String>,
  pub category_id: Option<i32>,
  pub board_id:    Option<i32>,
  pub parent_id:   Option<i32>,
  pub kind:        Option<BoardPostKind>,
  pub status:      Option<BoardPostStatus>,
}

impl BoardPostFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Title.into_expr(), v))
            .add(wrap_ilike(Column::Content.into_expr(), v)),
        )
      })
      .apply_if(self.title.as_ref(), |q, v| q.filter(Column::Title.eq(v)))
      .apply_if(self.category_id, |q, v| q.filter(Expr::col((Alias::new("board"), "category_id")).eq(v)))
      .apply_if(self.board_id, |q, v| q.filter(Column::BoardId.eq(v)))
      .apply_if(self.parent_id, |q, v| q.filter(Column::ParentId.eq(v)))
      .apply_if(self.kind.as_ref(), |q, v| q.filter(Column::Kind.eq(v.clone().into_value())))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Column::Status.eq(v.clone().into_value())))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "Users")]
pub struct BoardPostUser {
  #[sea_orm(from_alias = "user_id")]
  pub id:       i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "Boards")]
#[serde(rename_all = "camelCase")]
pub struct BoardPostBoard {
  #[sea_orm(from_alias = "board_id")]
  pub id:          i32,
  #[sea_orm(from_alias = "board_name")]
  pub name:        String,
  #[sea_orm(from_alias = "board_category_id")]
  pub category_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  pub id:           i32,
  pub parent_id:    Option<i32>,
  #[sea_orm(nested, alias = "board")]
  pub board:        BoardPostBoard,
  #[sea_orm(nested, alias = "user")]
  pub user:         BoardPostUser,
  pub title:        String,
  pub content:      Option<String>,
  pub kind:         BoardPostKind,
  pub status:       BoardPostStatus,
  pub tags:         Option<Vec<String>>,
  pub views:        i32,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPost {
  pub id:             i32,
  pub parent_id:      Option<i32>,
  #[sea_orm(nested, alias = "board")]
  pub board:          BoardPostBoard,
  #[sea_orm(nested, alias = "user")]
  pub user:           BoardPostUser,
  pub title:          String,
  pub content:        Option<String>,
  pub kind:           BoardPostKind,
  pub status:         BoardPostStatus,
  pub tags:           Option<Vec<String>>,
  pub published_at:   Option<DateTimeWithTimeZone>,
  pub scheduled_at:   Option<DateTimeWithTimeZone>,
  pub views:          i32,
  pub reactions:      Option<JsonValue>,
  #[sea_orm(skip)]
  pub attached_files: Vec<BoardFileListItem>,
  pub created_at:     DateTimeWithTimeZone,
  pub updated_at:     DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardPost {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardPost {
  pub id:                Optional<i32>,
  pub board_id:          i32,
  pub parent_id:         Nullable<i32>,
  pub title:             String,
  pub content:           Nullable<String>,
  pub kind:              Optional<BoardPostKind>,
  pub status:            Optional<BoardPostStatus>,
  pub published_at:      Nullable<DateTimeWithTimeZone>,
  pub tags:              Nullable<Vec<String>>,
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
pub struct CreateBoardDraft {
  pub board_id:          i32,
  pub parent_id:         Nullable<i32>,
  pub title:             String,
  pub content:           Nullable<String>,
  pub kind:              Optional<BoardPostKind>,
  pub tags:              Nullable<Vec<String>>,
  pub attached_file_ids: Option<Vec<i32>>,
}

impl CreateBoardDraft {
  pub fn into_active_model(self) -> entity::bs::board_posts::ActiveModel {
    entity::bs::board_posts::ActiveModel {
      board_id: Set(self.board_id),
      parent_id: self.parent_id.into_active_value(),
      title: Set(self.title),
      content: self.content.into_active_value(),
      kind: self.kind.into_active_value(),
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
  pub title:             Optional<String>,
  pub content:           Nullable<String>,
  pub kind:              Optional<BoardPostKind>,
  pub status:            Optional<BoardPostStatus>,
  pub published_at:      Nullable<DateTimeWithTimeZone>,
  pub tags:              Nullable<Vec<String>>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostPublished {
  pub success: bool,
}

impl IntoResponse for BoardPostPublished {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

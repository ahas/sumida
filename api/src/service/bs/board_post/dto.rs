use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_posts::*;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::*;
use sea_orm::{Condition, EntityTrait, IntoActiveValue, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::service::prelude::BoardFileListItem;
use crate::util::board::HtmlContent;
use crate::util::db::{tsquery, wrap_ilike};

fn parse_plain_text(
  content_type: BoardPostContentType,
  content: Nullable<String>,
) -> Result<String, SumidaError> {
  let content: Option<String> = content.into();
  let Some(content) = content else {
    return Ok("".to_string());
  };

  Ok(match content_type {
    BoardPostContentType::Html => HtmlContent(&content).to_plain_text(),
  })
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardPostFilter {
  pub search:         Option<String>,
  pub title:          Option<String>,
  pub text:           Option<String>,
  pub topic_id:       Option<i32>,
  pub board_id:       Option<i32>,
  pub parent_id:      Option<i32>,
  pub kind:           Option<BoardPostKind>,
  pub status:         Option<BoardPostStatus>,
  #[serde(default)]
  pub with_snippet:   bool,
  pub snippet_length: Option<i32>,
}

impl BoardPostFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(Column::Title.into_expr(), v))
            .add(tsquery(Column::TextTsv, v)),
        )
      })
      .apply_if(self.title.as_ref(), |q, v| q.filter(wrap_ilike(Column::Title.into_expr(), v)))
      .apply_if(self.text.as_ref(), |q, v| q.filter(tsquery(Column::TextTsv, v)))
      .apply_if(self.topic_id, |q, v| q.filter(Column::TopicId.eq(v)))
      .apply_if(self.board_id, |q, v| q.filter(Column::BoardId.eq(v)))
      .apply_if(self.parent_id, |q, v| q.filter(Column::ParentId.eq(v)))
      .apply_if(self.kind.as_ref(), |q, v| q.filter(Column::Kind.eq(v.clone().into_value())))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Column::Status.eq(v.clone().into_value())))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Users", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostUser {
  pub id:       i32,
  pub username: String,
  #[sea_orm(from_col = "full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Boards", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostBoard {
  pub id:       i32,
  pub name:     String,
  pub group_id: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardTopics", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostTopic {
  pub id:   i32,
  pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardPosts", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardPostListItem {
  pub id:           i32,
  pub parent_id:    Option<i32>,
  #[sea_orm(nested)]
  pub board:        BoardPostBoard,
  #[sea_orm(nested, nullable)]
  pub topic:        Option<BoardPostTopic>,
  #[sea_orm(nested, nullable)]
  pub user:         Option<BoardPostUser>,
  pub title:        String,
  pub text:      Option<String>,
  pub kind:         BoardPostKind,
  pub status:       BoardPostStatus,
  pub tags:         Option<Vec<String>>,
  pub views:        i32,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardPosts", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardPost {
  pub id:             i32,
  pub parent_id:      Option<i32>,
  #[sea_orm(nested)]
  pub board:          BoardPostBoard,
  #[sea_orm(nested, nullable)]
  pub topic:          BoardPostTopic,
  #[sea_orm(nested, nullable)]
  pub user:           Option<BoardPostUser>,
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
  pub topic_id:          Nullable<i32>,
  pub parent_id:         Nullable<i32>,
  pub title:             String,
  pub content:           Nullable<String>,
  pub content_type:      BoardPostContentType,
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
      topic_id: self.topic_id.into_active_value(),
      parent_id: self.parent_id.into_active_value(),
      title: Set(self.title),
      content: self.content.into_active_value(),
      content_type: Set(self.content_type),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }

  pub fn parse_plain_text(&self) -> Result<String, SumidaError> {
    parse_plain_text(self.content_type.clone(), self.content.clone())
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardDraft {
  pub board_id:          i32,
  pub topic_id:          Nullable<i32>,
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
      topic_id: self.topic_id.into_active_value(),
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
  #[serde(default)]
  pub topic_id:          Nullable<i32>,
  pub title:             Optional<String>,
  #[serde(default)]
  pub content:           Nullable<String>,
  pub kind:              Optional<BoardPostKind>,
  pub status:            Optional<BoardPostStatus>,
  #[serde(default)]
  pub published_at:      Nullable<DateTimeWithTimeZone>,
  #[serde(default)]
  pub tags:              Nullable<Vec<String>>,
  pub attached_file_ids: Option<Vec<i32>>,
}

impl UpdateBoardPost {
  pub fn into_active_model(self) -> entity::bs::board_posts::ActiveModel {
    entity::bs::board_posts::ActiveModel {
      topic_id: self.topic_id.into_active_value(),
      title: self.title.into_active_value(),
      content: self.content.into_active_value(),
      kind: self.kind.into_active_value(),
      status: self.status.into_active_value(),
      published_at: self.published_at.into_active_value(),
      tags: self.tags.into_active_value(),
      ..Default::default()
    }
  }

  pub fn parse_plain_text(&self, content_type: BoardPostContentType) -> Result<String, SumidaError> {
    parse_plain_text(content_type, self.content.clone())
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

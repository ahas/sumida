use crate::util::crud::wrap_ilike;
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_comments::{ActiveModel, Column, Entity};
use sea_orm::EntityTrait;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct BoardCommentFilter {
  pub search: Option<String>,
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
}

impl BoardCommentFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    let alias = Alias::new("bc");
    query
      .apply_if(self.search.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col((alias.clone(), Column::Content)), v))
      })
      .apply_if(self.post_id, |q, v| q.filter(Expr::col(Column::PostId).eq(v)))
      .apply_if(self.user_id, |q, v| q.filter(Expr::col(Column::UserId).eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardCommentListItem {
  pub id: i32,
  pub post_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub content: Option<String>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardComment {
  pub id: i32,
  pub post_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub content: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardComment {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardComment {
  pub id: Optional<i32>,
  pub parent_id: Nullable<i32>,
  pub post_id: i32,
  pub user_id: i32,
  pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardComment {
  pub content: Optional<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCommentCreated {
  pub id: i32,
}

impl IntoResponse for BoardCommentCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCommentUpdated;

impl IntoResponse for BoardCommentUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCommentDeleted;

impl IntoResponse for BoardCommentDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

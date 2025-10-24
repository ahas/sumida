use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_files::*;
use entity::prelude::*;
use sea_orm::prelude::*;
use sea_orm::{DeriveIntoActiveModel, EntityTrait, QueryTrait};
use sea_orm_crud::Optional;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::fs::dto::{FileListItem, GetFile};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFileFilter {
  pub id:      Option<i32>,
  pub post_id: Option<i32>,
}

impl BoardFileFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.id.as_ref(), |q, v| q.filter(Expr::col(Column::FileId).eq(*v)))
      .apply_if(self.post_id.as_ref(), |q, v| q.filter(Expr::col(Column::PostId).eq(*v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardFiles", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileListItem {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub file:    FileListItem,
  pub post_id: i32,
  // pub path:          String,
  // pub original_name: String,
  // pub uri:           Option<String>,
  // pub size:          i64,
  // pub mime_type:     String,
  // pub status:        String,
  // pub is_temporary:  bool,
  // pub expires_at:    Option<DateTimeWithTimeZone>,
  // pub created_at:    DateTimeWithTimeZone,
  // pub updated_at:    DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardFiles", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardFile {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub file:    GetFile,
  pub post_id: i32,
  // pub id:            i32,
  // pub user_id:       Option<i32>,
  // pub path:          String,
  // pub original_name: String,
  // pub uri:           Option<String>,
  // pub size:          i64,
  // pub mime_type:     String,
  // pub hash:          Option<String>,
  // pub status:        String,
  // pub is_temporary:  bool,
  // pub expires_at:    Option<DateTimeWithTimeZone>,
  // pub created_at:    DateTimeWithTimeZone,
  // pub updated_at:    DateTimeWithTimeZone,
}

impl IntoResponse for GetBoardFile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Debug, Deserialize, DeriveIntoActiveModel)]
#[sea_orm(entity = "Entity")]
#[serde(rename_all = "camelCase")]
pub struct CreateBoardFile {
  pub post_id: i32,
  pub file_id: i32,
}

#[derive(Deserialize, DeriveIntoActiveModel)]
#[sea_orm(entity = "Entity")]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardFile {
  pub post_id: Optional<i32>,
  pub file_id: Optional<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileCreated {
  pub id:      i32,
  pub post_id: i32,
}

impl IntoResponse for BoardFileCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(json!(self))).into_response()
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileUpdated;

impl IntoResponse for BoardFileUpdated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::OK, Json(json!(self))).into_response()
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileDeleted;

impl IntoResponse for BoardFileDeleted {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::OK, Json(json!(self))).into_response()
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachFileToPost {
  pub id:      i32,
  pub post_id: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachFileFromPost {
  pub id:      i32,
  pub post_id: i32,
}

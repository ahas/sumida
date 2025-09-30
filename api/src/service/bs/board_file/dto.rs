use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bs::board_files::*;
use sea_orm::DeriveIntoActiveModel;
use sea_orm::EntityTrait;
use sea_orm::QueryTrait;
use sea_orm::prelude::*;
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::db::wrap_ilike;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFileFilter {
  pub board_id: Option<i32>,
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
  pub mime_type: Option<String>,
  pub upload_status: Option<String>,
  pub is_temporary: Option<bool>,
  pub filename: Option<String>,
}

impl BoardFileFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.board_id.as_ref(), |q, v| q.filter(Expr::col(Column::BoardId).eq(*v)))
      .apply_if(self.post_id.as_ref(), |q, v| q.filter(Expr::col(Column::PostId).eq(*v)))
      .apply_if(self.user_id.as_ref(), |q, v| q.filter(Expr::col(Column::UserId).eq(*v)))
      .apply_if(self.mime_type.as_ref(), |q, v| q.filter(Expr::col(Column::MimeType).eq(v)))
      .apply_if(self.upload_status.as_ref(), |q, v| q.filter(Expr::col(Column::UploadStatus).eq(v)))
      .apply_if(self.is_temporary.as_ref(), |q, v| q.filter(Expr::col(Column::IsTemporary).eq(*v)))
      .apply_if(self.filename.as_ref(), |q, v| {
        q.filter(wrap_ilike(Expr::col(Column::OriginalFilename), v))
      })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Entity", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileListItem {
  pub id: i32,
  pub board_id: Option<i32>,
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
  pub original_filename: String,
  pub file_url: Option<String>,
  pub file_size: i64,
  pub mime_type: String,
  pub upload_status: String,
  pub is_temporary: bool,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardFile {
  pub id: i32,
  pub board_id: Option<i32>,
  pub post_id: Option<i32>,
  pub user_id: Option<i32>,
  pub original_filename: String,
  pub file_path: String,
  pub file_url: Option<String>,
  pub file_size: i64,
  pub mime_type: String,
  pub file_hash: Option<String>,
  pub upload_status: String,
  pub is_temporary: bool,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl From<Model> for GetBoardFile {
  fn from(model: Model) -> Self {
    Self {
      id: model.id,
      board_id: model.board_id,
      post_id: model.post_id,
      user_id: model.user_id,
      original_filename: model.original_filename,
      file_path: model.file_path,
      file_url: model.file_url,
      file_size: model.file_size,
      mime_type: model.mime_type,
      file_hash: model.file_hash,
      upload_status: model.upload_status,
      is_temporary: model.is_temporary,
      expires_at: model.expires_at,
      created_at: model.created_at,
      updated_at: model.updated_at,
    }
  }
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
  pub board_id: Optional<i32>,
  pub post_id: Optional<i32>,
  pub user_id: Optional<i32>,
  pub original_filename: String,
  pub file_path: String,
  pub file_url: Optional<String>,
  pub file_size: i64,
  pub mime_type: String,
  pub file_hash: Optional<String>,
  pub upload_status: String,
  pub is_temporary: Optional<bool>,
  pub expires_at: Optional<DateTimeWithTimeZone>,
}

#[derive(Deserialize, DeriveIntoActiveModel)]
#[sea_orm(entity = "Entity")]
#[serde(rename_all = "camelCase")]
pub struct UpdateBoardFile {
  pub file_url: Nullable<String>,
  pub file_hash: Nullable<String>,
  pub upload_status: Optional<String>,
  pub is_temporary: Optional<bool>,
  pub expires_at: Nullable<DateTimeWithTimeZone>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileCreated {
  pub id: i32,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileStats {
  pub total_files: i64,
  pub total_size: i64,
  pub by_mime_type: Vec<FileMimeTypeStats>,
  pub by_status: Vec<FileStatusStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMimeTypeStats {
  pub mime_type: String,
  pub count: i64,
  pub total_size: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatusStats {
  pub status: String,
  pub count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileDownload {
  pub content_type: String,
  pub filename: String,
  pub file_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkCompleted {
  pub file_url: Option<String>,
  pub file_hash: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupFailedParams {
  pub older_than_hours: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
  pub deleted_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeInfo {
  pub is_image: bool,
  pub is_video: bool,
  pub is_audio: bool,
  pub is_document: bool,
  pub file_extension: Option<String>,
  pub formatted_size: String,
  pub is_expired: bool,
  pub upload_status: entity::bs::board_files::UploadStatus,
}

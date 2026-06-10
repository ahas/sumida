use async_trait::async_trait;
use axum::Json;
use axum::extract::Multipart;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_files::*;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::*;
use sea_orm::{EntityTrait, QueryTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::fs::dto::{
  FileListItem,
  FileUploadData,
  FileUploadOptions,
  FileUploadTrait,
  GetFile,
  UploadFile,
};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFileFilter {
  pub id:      Option<i32>,
  pub post_id: Option<i32>,
}

impl BoardFileFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(self.id.as_ref(), |q, v| q.filter(Expr::col(Column::FileUuid).eq(*v)))
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

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBoardFile {
  pub board_id: i32,
  pub file:     FileUploadData,
  pub options:  FileUploadOptions,
}

#[async_trait]
impl FileUploadTrait for UploadBoardFile {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions) {
    (self.file, self.options)
  }

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError> {
    let mut board_id: Option<i32> = None;
    let mut file_data: Option<FileUploadData> = None;
    let mut options: Option<FileUploadOptions> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|err| SumidaError::BadRequest(format!("Multipart error: {err}")))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
        "board_id" => {
          let text = field
            .text()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Invalid board_id: {err}")))?;
          board_id = Some(
            text
              .trim()
              .parse::<i32>()
              .map_err(|err| SumidaError::BadRequest(format!("Invalid board_id: {err}")))?,
          );
        }
        "metadata" => {
          let text = field
            .text()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Invalid metadata: {err}")))?;
          options = Some(
            serde_json::from_str(&text)
              .map_err(|err| SumidaError::BadRequest(format!("Invalid metadata JSON: {err}")))?,
          );
        }
        "file" => {
          let filename = field.file_name().unwrap_or("unknown").to_string();
          let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
          let data = field
            .bytes()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Failed to read file: {err}")))?;

          file_data = Some(FileUploadData {
            original_name: filename,
            bytes:         data.to_vec(),
            mime_type:     content_type,
          });
        }
        _ => {}
      }
    }

    Ok(Self {
      board_id: board_id.ok_or_else(|| SumidaError::BadRequest("Missing field: board_id".into()))?,
      file:     file_data.ok_or_else(|| SumidaError::BadRequest("Missing field: file".into()))?,
      options:  options.ok_or_else(|| SumidaError::BadRequest("Missing field: metadata".into()))?,
    })
  }
}

impl From<UploadBoardFile> for UploadFile {
  fn from(value: UploadBoardFile) -> Self {
    UploadFile {
      file:    value.file,
      options: value.options,
    }
  }
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBoardFile {
  #[serde(default)]
  pub hard:      bool,
  pub post_id:   i32,
  pub file_uuid: Uuid,
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

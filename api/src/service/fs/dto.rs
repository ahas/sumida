use axum::Json;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::*;
use sea_orm::sea_query::Alias;
use sea_orm::{Condition, QueryTrait};
use sea_orm_crud::Nullable;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::SumidaState;
use crate::util::ap::ApForUser;
use crate::util::db::{wrap_ilike, wrap_ilike_col};
use crate::util::storage::StaticFile;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FileFilter {
  pub user:      Option<String>,
  pub dir:       Option<String>,
  pub name:      Option<String>,
  pub uri:       Option<String>,
  pub status:    Option<String>,
  pub mime_type: Option<String>,
}

impl FileFilter {
  pub fn apply<E>(&self, query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    use entity::fs::files::Column;

    query
      .apply_if(self.user.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike_col(entity::users::Column::Username, v))
            .add(wrap_ilike(Expr::col((Users, Alias::new("full_name"))), v)),
        )
      })
      .apply_if(self.dir.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Dir, v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Name, v)))
      .apply_if(self.uri.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Uri, v)))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Column::Status.eq(v)))
      .apply_if(self.mime_type.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::MimeType, v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Users", from_query_result)]
pub struct FileUser {
  pub id:       i32,
  pub username: String,
  #[sea_orm(from_col = "full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Files", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct FileListItem {
  pub id:            i32,
  #[sea_orm(nested)]
  pub user:          FileUser,
  pub original_name: String,
  pub dir:           String,
  pub name:          String,
  pub uri:           Option<String>,
  pub size:          i64,
  pub mime_type:     String,
  pub status:        String,
  pub tags:          Option<Vec<String>>,
  pub refs:          i32,
  pub expires_at:    Option<DateTimeWithTimeZone>,
  pub created_at:    DateTimeWithTimeZone,
  pub updated_at:    DateTimeWithTimeZone,
  pub deleted_at:    Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Files", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetFile {
  pub id:            i32,
  #[sea_orm(nested)]
  pub user:          FileUser,
  pub original_name: String,
  pub dir:           String,
  pub name:          String,
  pub uri:           Option<String>,
  pub size:          i64,
  pub mime_type:     String,
  pub hash:          Option<String>,
  pub tags:          Option<Vec<String>>,
  pub status:        String,
  pub refs:          i32,
  pub expires_at:    Option<DateTimeWithTimeZone>,
  pub created_at:    DateTimeWithTimeZone,
  pub updated_at:    DateTimeWithTimeZone,
  pub deleted_at:    Option<DateTimeWithTimeZone>,

  #[sea_orm(skip)]
  pub access_policies: Option<Vec<ApForUser>>,
}

impl IntoResponse for GetFile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploadOptions {
  #[serde(default)]
  pub tags:       Option<Vec<String>>,
  #[serde(default)]
  pub dir:        String,
  #[serde(default)]
  pub expires_at: Nullable<DateTimeWithTimeZone>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploadData {
  pub original_name: String,
  pub bytes:         Vec<u8>,
  pub mime_type:     String,
}

impl FileUploadData {
  pub fn hash(&self) -> String {
    blake3::hash(&self.bytes).to_hex().to_string()
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
  pub file:    FileUploadData,
  pub options: FileUploadOptions,
}

impl UploadFile {
  pub async fn from_request<S: SumidaState>(state: S, request: Request) -> Result<Self, SumidaError> {
    use axum::http::header;

    let content_type = request
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|ct| ct.to_str().ok())
      .unwrap_or("");
    if content_type.starts_with("multipart/form-data") {
      let mut multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid multipart data".into()))?;

      Self::from_multipart(&mut multipart).await
    } else {
      Err(SumidaError::BadRequest("Only multipart/form-data is supported for file uploads".into()))
    }
  }

  pub async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError> {
    let mut file_data: Option<FileUploadData> = None;
    let mut options: Option<FileUploadOptions> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|e| SumidaError::BadRequest(format!("Multipart error: {}", e)))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
        "metadata" => {
          let text = field
            .text()
            .await
            .map_err(|e| SumidaError::BadRequest(format!("Invalid metadata: {}", e)))?;
          options = Some(
            serde_json::from_str(&text)
              .map_err(|e| SumidaError::BadRequest(format!("Invalid metadata JSON: {}", e)))?,
          );
        }
        "file" => {
          let filename = field.file_name().unwrap_or("unknown").to_string();
          let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
          let data = field
            .bytes()
            .await
            .map_err(|e| SumidaError::BadRequest(format!("Failed to read file: {}", e)))?;

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
      file:    file_data.unwrap(),
      options: options.unwrap(),
    })
  }

  pub fn into_static_file(self) -> StaticFile {
    use std::path::Path;

    let file_extension = Path::new(&self.file.original_name)
      .extension()
      .and_then(|ext| ext.to_str())
      .unwrap_or("");
    let unique_filename = format!("{}.{}", Uuid::new_v4(), file_extension);

    StaticFile {
      dir:           self.options.dir,
      name:          unique_filename,
      original_name: self.file.original_name,
      mime_type:     self.file.mime_type,
      bytes:         self.file.bytes,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploaded {
  pub id: i32,
}

impl IntoResponse for FileUploaded {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFile {
  #[serde(default)]
  pub hard: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleted;

impl IntoResponse for FileDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStats {
  pub total_files:  i64,
  pub total_size:   i64,
  pub by_mime_type: Vec<FileMimeTypeStats>,
  pub by_status:    Vec<FileStatusStats>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMimeTypeStats {
  pub mime_type:  String,
  pub count:      i64,
  pub total_size: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatusStats {
  pub status: String,
  pub count:  i64,
}

use async_trait::async_trait;
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
use crate::util::db::wrap_ilike;
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
            .add(wrap_ilike(entity::users::Column::Username.into_expr(), v))
            .add(wrap_ilike(Expr::col((Users, Alias::new("full_name"))), v)),
        )
      })
      .apply_if(self.dir.as_ref(), |q, v| q.filter(wrap_ilike(Column::Dir.into_expr(), v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.uri.as_ref(), |q, v| q.filter(wrap_ilike(Column::Uri.into_expr(), v)))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Column::Status.eq(v)))
      .apply_if(self.mime_type.as_ref(), |q, v| q.filter(wrap_ilike(Column::MimeType.into_expr(), v)))
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

  pub fn get_extension(&self) -> Option<String> {
    std::path::Path::new(&self.original_name)
      .extension()
      .and_then(|ext| ext.to_str())
      .map(|x| x.to_owned())
  }
}

#[async_trait]
pub trait FileUploadTrait: Sized {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions);

  async fn from_request<S: SumidaState>(state: S, request: Request) -> Result<Self, SumidaError> {
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

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError>;

  fn into_static_file(self) -> StaticFile {
    let (file, options) = self.into_parts();
    let file_extension = file.get_extension();
    let file_uuid = Uuid::new_v4();
    let unique_filename = if let Some(ref ext) = file_extension {
      format!("{}.{}", file_uuid, ext)
    } else {
      file_uuid.to_string()
    };

    StaticFile {
      name:          unique_filename,
      dir:           options.dir,
      original_name: file.original_name,
      mime_type:     file.mime_type,
      bytes:         file.bytes,
      extension:     file_extension,
    }
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
  pub file:    FileUploadData,
  pub options: FileUploadOptions,
}

#[async_trait]
impl FileUploadTrait for UploadFile {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions) {
    (self.file, self.options)
  }

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError> {
    let mut file_data: Option<FileUploadData> = None;
    let mut options: Option<FileUploadOptions> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|err| SumidaError::BadRequest(format!("Multipart error: {err}")))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
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
      file:    file_data.unwrap(),
      options: options.unwrap(),
    })
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

use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::fs::files::*;
use sea_orm::prelude::*;
use sea_orm::{Condition, DeriveIntoActiveModel, FromQueryResult, QueryTrait};
use sea_orm_crud::{Nullable, Optional};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::util::ap::ApForUser;
use crate::util::db::{wrap_ilike, wrap_ilike_col};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FileFilter {
  pub user:         Option<String>,
  pub path:         Option<String>,
  pub uri:          Option<String>,
  pub status:       Option<String>,
  pub mime_type:    Option<String>,
  pub is_temporary: Option<bool>,
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
            .add(wrap_ilike(Expr::col("user_username"), v))
            .add(wrap_ilike(Expr::col("user_full_name"), v)),
        )
      })
      .apply_if(self.path.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Path, v)))
      .apply_if(self.uri.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::Uri, v)))
      .apply_if(self.status.as_ref(), |q, v| q.filter(Column::Status.eq(v)))
      .apply_if(self.mime_type.as_ref(), |q, v| q.filter(wrap_ilike_col(Column::MimeType, v)))
      .apply_if(self.is_temporary, |q, v| q.filter(Column::IsTemporary.eq(v)))
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[sea_orm(entity = "entity::users::Entity")]
pub struct FileUser {
  #[sea_orm(from_alias = "user_id")]
  pub id:       i32,
  #[sea_orm(from_alias = "user_username")]
  pub username: String,
  #[sea_orm(from_alias = "user_full_name")]
  pub name:     String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct FileListItem {
  pub id:           i32,
  #[sea_orm(nested, alias = "user")]
  pub user:         FileUser,
  pub path:         String,
  pub uri:          Option<String>,
  pub size:         i64,
  pub mime_type:    String,
  pub status:       String,
  pub is_temporary: bool,
  pub expires_at:   Option<DateTimeWithTimeZone>,
  pub created_at:   DateTimeWithTimeZone,
  pub updated_at:   DateTimeWithTimeZone,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetFile {
  pub id:              i32,
  #[sea_orm(nested, alias = "user")]
  pub user:            FileUser,
  pub path:            String,
  pub uri:             Option<String>,
  pub size:            i64,
  pub mime_type:       String,
  pub hash:            Option<String>,
  pub status:          String,
  pub is_temporary:    bool,
  pub expires_at:      Option<DateTimeWithTimeZone>,
  pub created_at:      DateTimeWithTimeZone,
  pub updated_at:      DateTimeWithTimeZone,
  #[sea_orm(skip)]
  pub access_policies: Option<Vec<ApForUser>>,
}

impl IntoResponse for GetFile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct CreateFile {
  pub path:         String,
  pub mime_type:    String,
  pub is_temporary: Optional<bool>,
  #[serde(default)]
  pub expires_at:   Nullable<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileMetadata {
  pub original: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCreated {
  pub id: i32,
}

impl IntoResponse for FileCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DeriveIntoActiveModel)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFile {
  pub path:         Optional<String>,
  pub mime_type:    Optional<String>,
  pub is_temporary: Optional<bool>,
  #[serde(default)]
  pub expires_at:   Nullable<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUpdated;

impl IntoResponse for FileUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
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
pub struct FileUploadMeta {
  pub is_temporary: Option<bool>,
  pub expires_at:   Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDownload {
  pub content_type: String,
  pub filename:     String,
  pub file_path:    String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkFileCompleted {
  pub uri:  Option<String>,
  pub hash: Option<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
  pub deleted_count: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupFailedParams {
  pub older_than_hours: Option<i64>,
}

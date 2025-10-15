use chrono::Utc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
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

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "crate::users::Entity",
    from = "Column::UserId",
    to = "crate::users::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  User,
}

impl Related<crate::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl Related<crate::bs::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    crate::bs::board_files::Relation::Post.def()
  }

  fn via() -> Option<RelationDef> {
    Some(crate::bs::board_files::Relation::File.def().rev())
  }
}

impl ActiveModelBehavior for ActiveModel {}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum UploadStatus {
  Pending,
  Processing,
  Completed,
  Failed,
  Deleted,
}

impl From<UploadStatus> for String {
  fn from(status: UploadStatus) -> Self {
    match status {
      UploadStatus::Pending => "pending".to_string(),
      UploadStatus::Processing => "processing".to_string(),
      UploadStatus::Completed => "completed".to_string(),
      UploadStatus::Failed => "failed".to_string(),
      UploadStatus::Deleted => "deleted".to_string(),
    }
  }
}

impl Model {
  pub fn is_image(&self) -> bool {
    self.mime_type.starts_with("image/")
  }

  pub fn is_video(&self) -> bool {
    self.mime_type.starts_with("video/")
  }

  pub fn is_audio(&self) -> bool {
    self.mime_type.starts_with("audio/")
  }

  pub fn is_document(&self) -> bool {
    matches!(
      self.mime_type.as_str(),
      "application/pdf"
        | "application/msword"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.ms-excel"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "text/plain"
        | "text/csv"
    )
  }

  pub fn get_file_extension(&self) -> Option<String> {
    self.original_filename.rfind('.').map(|pos| self.original_filename[pos + 1..].to_lowercase())
  }

  pub fn is_expired(&self) -> bool {
    if let Some(expires_at) = self.expires_at {
      expires_at < Utc::now().fixed_offset()
    } else {
      false
    }
  }

  pub fn format_file_size(&self) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = self.file_size as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
      size /= 1024.0;
      unit_index += 1;
    }

    if unit_index == 0 {
      format!("{} {}", self.file_size, UNITS[unit_index])
    } else {
      format!("{:.1} {}", size, UNITS[unit_index])
    }
  }

  pub fn is_owned_by(&self, user_id: Option<i32>) -> bool {
    if let (Some(file_user_id), Some(check_user_id)) = (self.user_id, user_id) {
      file_user_id == check_user_id
    } else {
      false
    }
  }
}

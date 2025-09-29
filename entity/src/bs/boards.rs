use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
pub enum BoardPostRecordVisibility {
  Public,
  Private,
  AuthorOnly,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "boards")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub category_id: Option<i32>,
  pub name: String,
  pub description: Option<String>,
  pub can_upload_file: bool,
  pub max_file_size: i64,
  pub max_file_count: i32,
  pub allowed_file_types: Json,
  pub emotion_visibility: BoardPostRecordVisibility,
  pub viewer_visibility: BoardPostRecordVisibility,
  pub is_logged: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_categories::Entity",
    from = "Column::CategoryId",
    to = "super::board_categories::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Category,

  #[sea_orm(has_many = "super::board_posts::Entity")]
  Posts,
}

impl Related<super::board_categories::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Category.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
  pub fn get_allowed_file_types(&self) -> Vec<String> {
    match &self.allowed_file_types {
      Json::Array(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
      _ => vec![], // fallback to empty if not an array
    }
  }

  pub fn is_file_type_allowed(&self, file_type: &str) -> bool {
    let allowed_types = self.get_allowed_file_types();
    let file_extension = file_type.to_lowercase();
    allowed_types.iter().any(|t| t.to_lowercase() == file_extension)
  }

  pub fn can_upload_more_files(&self, current_count: i32) -> bool {
    current_count < self.max_file_count
  }
}

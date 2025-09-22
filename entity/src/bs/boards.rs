use sea_orm::entity::prelude::*;


#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "boards")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub category_id: Option<i32>,
  pub name: String,
  pub description: Option<String>,
  pub is_system: bool,
  pub system_category: Option<String>,
  pub max_file_size: i64,
  pub allowed_file_types: Json,
  pub max_files_per_post: i32,
  pub enable_file_upload: bool,
  pub enable_post_history: bool,
  pub max_history_count: i32,
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
  pub fn is_file_upload_allowed(&self) -> bool {
    self.enable_file_upload
  }

  pub fn get_max_file_size(&self) -> i64 {
    self.max_file_size
  }

  pub fn get_allowed_file_types(&self) -> Vec<String> {
    match &self.allowed_file_types {
      Json::Array(arr) => arr
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect(),
      _ => vec![], // fallback to empty if not an array
    }
  }

  pub fn is_file_type_allowed(&self, file_type: &str) -> bool {
    let allowed_types = self.get_allowed_file_types();
    let file_extension = file_type.to_lowercase();
    allowed_types.iter().any(|t| t.to_lowercase() == file_extension)
  }

  pub fn can_upload_more_files(&self, current_count: i32) -> bool {
    current_count < self.max_files_per_post
  }

  pub fn is_history_enabled(&self) -> bool {
    self.enable_post_history
  }

  pub fn get_max_history_count(&self) -> i32 {
    self.max_history_count
  }

  pub fn is_system_board(&self) -> bool {
    self.is_system
  }

  pub fn get_system_category(&self) -> Option<&str> {
    self.system_category.as_deref()
  }

}

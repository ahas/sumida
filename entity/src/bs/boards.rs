use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
  Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, EnumIter, DeriveActiveEnum,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BoardPostRecordVisibility {
  #[serde(rename = "public")]
  Public = 0,
  #[serde(rename = "author-only")]
  AuthorOnly,
  #[serde(rename = "private")]
  Private,
}

#[derive(
  Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, EnumIter, DeriveActiveEnum,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BoardPostContentVisibility {
  #[serde(rename = "public")]
  Public = 0,
  #[serde(rename = "user-only")]
  UserOnly,
  #[serde(rename = "private")]
  Private,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "boards")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  #[sea_orm(nullable)]
  pub group_id: Option<i32>,
  pub name: String,
  #[sea_orm(nullable)]
  pub description: Option<String>,
  pub order: i32,
  pub can_upload_file: bool,
  pub max_file_size: i64,
  pub max_file_count: i32,
  #[sea_orm(nullable)]
  pub allowed_file_types: Option<Json>,
  pub reaction_visibility: BoardPostRecordVisibility,
  pub viewer_visibility: BoardPostRecordVisibility,
  pub content_visibility: BoardPostContentVisibility,
  pub is_logged: bool,
  pub is_enabled: bool,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
  #[sea_orm(nullable)]
  pub deleted_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_groups::Entity",
    from = "Column::GroupId",
    to = "super::board_groups::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Group,

  #[sea_orm(has_many = "super::board_posts::Entity")]
  Posts,
}

impl Related<super::board_groups::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Group.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
  pub fn get_allowed_file_types(&self) -> Vec<String> {
    match &self.allowed_file_types {
      Some(Json::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect(),
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

use chrono::Utc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BoardPostStatus {
  Draft,
  Published,
  Archived,
  Deleted,
  Scheduled,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BoardPostKind {
  Article,
  Comment,
  Reply,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_posts")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub board_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub kind: BoardPostKind,
  pub status: BoardPostStatus,
  pub revision: Option<i32>,
  pub title: String,
  pub compression_type: Option<String>,
  pub content_type: Option<String>,
  pub content: Option<String>,
  pub text: Option<String>,
  pub tags: Option<Vec<String>>,
  pub views: i32,
  #[sea_orm(column_type = "JsonBinary")]
  pub reactions: Option<Json>,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
  pub deleted_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::boards::Entity",
    from = "Column::BoardId",
    to = "super::boards::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Board,
  #[sea_orm(has_many = "super::board_post_views::Entity")]
  Views,
  #[sea_orm(belongs_to = "Entity", from = "Column::ParentId", to = "Column::Id", on_delete = "Cascade")]
  Parent,
  #[sea_orm(belongs_to = "crate::users::Entity", from = "Column::UserId", to = "crate::users::Column::Id")]
  User,
}

impl Related<super::boards::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Board.def()
  }
}

impl Related<Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Parent.def()
  }
}

impl Related<crate::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
  pub fn is_published(&self) -> bool {
    matches!(self.status, BoardPostStatus::Published)
  }

  pub fn is_draft(&self) -> bool {
    matches!(self.status, BoardPostStatus::Draft)
  }

  pub fn is_scheduled(&self) -> bool {
    matches!(self.status, BoardPostStatus::Scheduled)
  }

  pub fn is_archived(&self) -> bool {
    matches!(self.status, BoardPostStatus::Archived)
  }

  pub fn is_deleted(&self) -> bool {
    matches!(self.status, BoardPostStatus::Deleted)
  }

  pub fn is_article(&self) -> bool {
    matches!(self.kind, BoardPostKind::Article)
  }

  pub fn is_comment(&self) -> bool {
    matches!(self.kind, BoardPostKind::Comment)
  }

  pub fn is_reply(&self) -> bool {
    matches!(self.kind, BoardPostKind::Reply)
  }

  pub fn is_visible_to_public(&self) -> bool {
    self.is_published()
  }

  pub fn should_be_published_now(&self) -> bool {
    if !self.is_scheduled() {
      return false;
    }

    if let Some(scheduled_at) = self.scheduled_at {
      scheduled_at <= Utc::now().fixed_offset()
    } else {
      false
    }
  }

  pub fn get_tags(&self) -> Vec<String> {
    self.tags.clone().unwrap_or_default()
  }

  pub fn has_tag(&self, tag: &str) -> bool {
    self.get_tags().contains(&tag.to_string())
  }

  pub fn has_any_tags(&self, tags: &[String]) -> bool {
    let post_tags = self.get_tags();
    tags.iter().any(|tag| post_tags.contains(tag))
  }

  pub fn add_tag(&mut self, tag: String) {
    let mut current_tags = self.get_tags();
    if !current_tags.contains(&tag) {
      current_tags.push(tag);
      self.tags = Some(current_tags);
    }
  }

  pub fn remove_tag(&mut self, tag: &str) {
    if let Some(ref mut tags) = self.tags {
      tags.retain(|t| t != tag);
      if tags.is_empty() {
        self.tags = None;
      }
    }
  }

  pub fn set_tags(&mut self, tags: Vec<String>) {
    self.tags = if tags.is_empty() { None } else { Some(tags) };
  }
}

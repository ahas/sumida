use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use chrono::Utc;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum PostStatus {
  Draft,
  Published,
  Archived,
  Deleted,
  Scheduled,
}

impl From<String> for PostStatus {
  fn from(value: String) -> Self {
    match value.as_str() {
      "published" => PostStatus::Published,
      "archived" => PostStatus::Archived,
      "deleted" => PostStatus::Deleted,
      "scheduled" => PostStatus::Scheduled,
      _ => PostStatus::Draft,
    }
  }
}

impl From<PostStatus> for String {
  fn from(status: PostStatus) -> Self {
    match status {
      PostStatus::Draft => "draft".to_string(),
      PostStatus::Published => "published".to_string(),
      PostStatus::Archived => "archived".to_string(),
      PostStatus::Deleted => "deleted".to_string(),
      PostStatus::Scheduled => "scheduled".to_string(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_posts")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub board_id: i32,
  pub parent_id: Option<i32>,
  pub user_id: Option<i32>,
  pub title: String,
  pub content: Option<String>,
  pub status: String,
  pub published_at: Option<DateTimeWithTimeZone>,
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  pub auto_save_content: Option<String>,
  pub last_auto_save_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
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
  #[sea_orm(has_many = "super::board_comments::Entity")]
  Comments,
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
  pub fn get_status(&self) -> PostStatus {
    PostStatus::from(self.status.clone())
  }

  pub fn is_published(&self) -> bool {
    matches!(self.get_status(), PostStatus::Published)
  }

  pub fn is_draft(&self) -> bool {
    matches!(self.get_status(), PostStatus::Draft)
  }

  pub fn is_scheduled(&self) -> bool {
    matches!(self.get_status(), PostStatus::Scheduled)
  }

  pub fn is_archived(&self) -> bool {
    matches!(self.get_status(), PostStatus::Archived)
  }

  pub fn is_deleted(&self) -> bool {
    matches!(self.get_status(), PostStatus::Deleted)
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

  pub fn has_auto_saved_content(&self) -> bool {
    self.auto_save_content.is_some() && self.last_auto_save_at.is_some()
  }

  pub fn get_auto_save_content(&self) -> Option<&str> {
    self.auto_save_content.as_deref()
  }

  pub fn is_auto_save_newer_than_content(&self) -> bool {
    if let Some(auto_save_at) = self.last_auto_save_at {
      auto_save_at > self.updated_at
    } else {
      false
    }
  }

  pub fn can_be_edited_by(&self, user_id: Option<i32>, is_admin: bool) -> bool {
    if is_admin {
      return true;
    }

    if self.is_deleted() {
      return false;
    }

    // 작성자만 편집 가능
    if let (Some(post_user_id), Some(check_user_id)) = (self.user_id, user_id) {
      post_user_id == check_user_id
    } else {
      false
    }
  }

  pub fn can_be_viewed_by(&self, user_id: Option<i32>, is_admin: bool, is_board_member: bool) -> bool {
    if is_admin {
      return true;
    }

    match self.get_status() {
      PostStatus::Published => true,
      PostStatus::Draft | PostStatus::Scheduled => {
        // 작성자 또는 보드 멤버만 볼 수 있음
        if let (Some(post_user_id), Some(check_user_id)) = (self.user_id, user_id) {
          post_user_id == check_user_id || is_board_member
        } else {
          is_board_member
        }
      }
      PostStatus::Archived => is_board_member,
      PostStatus::Deleted => false,
    }
  }

  pub fn get_effective_content(&self) -> &str {
    // 자동 저장된 내용이 더 최신이면 그것을 반환, 아니면 기본 내용
    if self.is_auto_save_newer_than_content() {
      self.get_auto_save_content().unwrap_or(self.content.as_deref().unwrap_or(""))
    } else {
      self.content.as_deref().unwrap_or("")
    }
  }
}

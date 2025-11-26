use chrono::Utc;
use sea_orm::UpdateResult;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

use crate::bs::boards::BoardPostContentVisibility;

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

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)", rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BoardPostContentType {
  Html,
  Markdown,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_posts")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  #[sea_orm(indexed)]
  pub board_id: i32,
  #[sea_orm(indexed, nullable)]
  pub topic_id: Option<i32>,
  #[sea_orm(indexed, nullable)]
  pub parent_id: Option<i32>,
  #[sea_orm(indexed, nullable)]
  pub user_id: Option<i32>,
  #[sea_orm(indexed)]
  pub kind: BoardPostKind,
  #[sea_orm(indexed)]
  pub status: BoardPostStatus,
  #[sea_orm(nullable)]
  pub revision: Option<i32>,
  pub title: String,
  #[sea_orm(nullable)]
  pub password: Option<String>,
  #[sea_orm(nullable)]
  pub compression_type: Option<String>,
  pub content_type: BoardPostContentType,
  #[sea_orm(nullable, column_type = "Text")]
  pub content: Option<String>,
  #[sea_orm(nullable, default_value = "'private'")]
  pub content_visibility: BoardPostContentVisibility,
  #[sea_orm(nullable, column_type = "Text")]
  pub text: Option<String>,
  #[sea_orm(nullable, select_as = "TEXT")]
  pub text_tsv: Option<String>,
  #[sea_orm(nullable)]
  pub tags: Option<Vec<String>>,
  #[sea_orm(default_value = 0)]
  pub views: i32,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub reactions: Option<Json>,
  #[sea_orm(nullable)]
  pub published_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub scheduled_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
  #[sea_orm(nullable)]
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
  #[sea_orm(
    belongs_to = "super::board_topics::Entity",
    from = "Column::TopicId",
    to = "super::board_topics::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  Topic,
  #[sea_orm(has_many = "super::board_post_views::Entity")]
  Views,
  #[sea_orm(
    belongs_to = "Entity",
    from = "Column::ParentId",
    to = "Column::Id",
    on_delete = "Cascade"
  )]
  Parent,
  #[sea_orm(
    belongs_to = "crate::users::Entity",
    from = "Column::UserId",
    to = "crate::users::Column::Id"
  )]
  User,
}

impl Related<super::boards::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Board.def()
  }
}

impl Related<super::board_topics::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Topic.def()
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

impl Related<crate::fs::files::Entity> for Entity {
  fn to() -> RelationDef {
    super::board_files::Relation::File.def()
  }

  fn via() -> Option<RelationDef> {
    Some(super::board_files::Relation::Post.def().rev())
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

impl Entity {
  pub async fn update_text<S: AsRef<str>, C: ConnectionTrait>(
    id: i32,
    text: S,
    db: &C,
  ) -> Result<UpdateResult, DbErr> {
    Self::update_many()
      .col_expr(Column::Text, Expr::val(text.as_ref()).into())
      .col_expr(Column::TextTsv, Expr::cust_with_values("to_tsvector('simple', $1)", [text.as_ref()]))
      .filter(Column::Id.eq(id))
      .exec(db)
      .await
  }
}

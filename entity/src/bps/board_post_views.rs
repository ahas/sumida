// entity/board_post_views.rs
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_post_views")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id:         i32,
  #[sea_orm(indexed)]
  pub post_id:    i32,
  #[sea_orm(indexed, nullable)]
  pub user_id:    Option<i32>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_posts::Entity",
    from = "Column::PostId",
    to = "super::board_posts::Column::Id",
    on_delete = "Cascade",
    on_update = "Cascade"
  )]
  Post,
  #[sea_orm(
    belongs_to = "crate::users::Entity",
    from = "Column::UserId",
    to = "crate::users::Column::Id"
  )]
  User,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Post.def()
  }
}

impl Related<crate::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

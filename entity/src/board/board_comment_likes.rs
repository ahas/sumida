use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_comment_likes")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub comment_id: i32,
  pub user_id: i32,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_comments::Entity",
    from = "Column::CommentId",
    to = "super::board_comments::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  BoardComment,
}

impl Related<super::board_comments::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::BoardComment.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

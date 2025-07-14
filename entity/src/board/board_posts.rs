use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_posts")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub board_id: i32,
  pub user_id: i32,
  pub title: String,
  #[sea_orm(column_type = "Text")]
  pub content: String,
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
  BoardComment,

  #[sea_orm(has_many = "super::board_post_views::Entity")]
  BoardPostViews,
}

impl Related<super::boards::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Board.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

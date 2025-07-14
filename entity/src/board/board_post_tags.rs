use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_post_tags")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub post_id: i32,
  #[sea_orm(primary_key, auto_increment = false)]
  pub tag_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_posts::Entity",
    from = "Column::PostId",
    to = "super::board_posts::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  BoardPost,

  #[sea_orm(
    belongs_to = "super::board_tags::Entity",
    from = "Column::TagId",
    to = "super::board_tags::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  BoardTag,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::BoardPost.def()
  }
}

impl Related<super::board_tags::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::BoardTag.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

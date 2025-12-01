use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_files")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub post_id: i32,
  #[sea_orm(primary_key)]
  pub file_id: i32,
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
  Post,
  #[sea_orm(
    belongs_to = "crate::fs::files::Entity",
    from = "Column::FileId",
    to = "crate::fs::files::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  File,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Post.def()
  }
}

impl Related<crate::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::File.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

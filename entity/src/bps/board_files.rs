use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_files")]
pub struct Model {
  // A file may be linked to at most one board post (1:1), so file_uuid is the sole primary key.
  // `post_id` is nullable: a row with `post_id = NULL` is a *pending claim* — uploaded for a board
  // (e.g. tiptap pre-upload) but not yet attached to a created post.
  #[sea_orm(primary_key, auto_increment = false)]
  pub file_uuid: Uuid,
  #[sea_orm(indexed, nullable)]
  pub post_id: Option<i32>,
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
    from = "Column::FileUuid",
    to = "crate::fs::files::Column::Uuid",
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

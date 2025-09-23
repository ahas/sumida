use sea_orm::entity::prelude::*;
use serde_json::Value as JsonValue;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_post_histories")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub post_id: i32,
  #[sea_orm(column_type = "JsonBinary")]
  pub content: JsonValue,
  pub modified_by: i32,
  pub modified_at: DateTimeWithTimeZone,
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
    belongs_to = "super::super::users::Entity",
    from = "Column::ModifiedBy",
    to = "super::super::users::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  User,
}

impl Related<super::board_posts::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Post.def()
  }
}

impl Related<super::super::users::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}
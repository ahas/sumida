use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_topics")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id:          i32,
  #[sea_orm(indexed)]
  pub board_id:    i32,
  pub name:        String,
  #[sea_orm(nullable)]
  pub description: Option<String>,
  #[sea_orm(default_value = "0")]
  pub order:       i32,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:  DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at:  DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::boards::Entity",
    from = "Column::BoardId",
    to = "super::boards::Column::Id",
    on_update = "Cascade"
    on_delete = "Cascade"
  )]
  Board,
  #[sea_orm(has_many = "super::board_posts::Entity")]
  Posts,
}

impl Related<Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Board.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "board_groups")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  #[sea_orm(indexed, nullable)]
  pub parent_id: Option<i32>,
  pub name: String,
  #[sea_orm(nullable)]
  pub description: Option<String>,
  #[sea_orm(default_value = "0")]
  pub order: i32,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_many = "super::boards::Entity")]
  Boards,
  #[sea_orm(
    belongs_to = "Entity",
    from = "Column::ParentId",
    to = "Column::Id",
    on_delete = "Cascade"
  )]
  Parent,
}

impl Related<Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Parent.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

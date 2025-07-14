use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "boards")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,
  pub category_id: i32,
  pub name: String,
  pub description: Option<String>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::board_categories::Entity",
    from = "Column::CategoryId",
    to = "super::board_categories::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  BoardCategory,

  #[sea_orm(has_many = "super::board_posts::Entity")]
  BoardPosts,
}

impl Related<super::board_categories::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::BoardCategory.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

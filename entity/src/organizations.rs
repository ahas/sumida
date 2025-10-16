use sea_orm::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "organizations")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:         i32,
  pub name:       String,
  pub parent_id:  Option<i32>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Entity",
    from = "Column::ParentId",
    to = "Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  Parent,
}

impl ActiveModelBehavior for ActiveModel {}

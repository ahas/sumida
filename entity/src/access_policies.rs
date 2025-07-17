use sea_orm::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "access_policies")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub uuid: Uuid,
  pub namespace: String,
  pub effect: String,
  pub object: String,
  pub action: Vec<String>,
  pub user: String,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

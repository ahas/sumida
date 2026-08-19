use sea_orm::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "access_policies")]
pub struct Model {
  #[sea_orm(primary_key, default = "uuid_generate_v4()")]
  pub uuid:       Uuid,
  #[sea_orm(indexed)]
  pub namespace:  String,
  pub effect:     String,
  #[sea_orm(indexed)]
  pub object:     String,
  #[sea_orm(indexed, default_value = "'{}'")]
  pub action:     Vec<String>,
  #[sea_orm(indexed)]
  pub user:       String,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

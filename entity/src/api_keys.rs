use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub key: String,
  pub user_id: i32,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub ability: Option<Json>,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

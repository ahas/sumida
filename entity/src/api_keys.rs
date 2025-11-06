use sea_orm::entity::prelude::*;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub key:        String,
  #[sea_orm(indexed)]
  pub user_id:    i32,
  #[sea_orm(nullable, default_value = "CURRENT_TIMESTAMP")]
  pub expires_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(default_value = false)]
  pub is_secured: bool,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Users",
    from = "Column::UserId",
    to = "super::users::Column::Id",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  User,
}

impl Related<Users> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

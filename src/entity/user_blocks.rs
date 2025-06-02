use sea_orm::prelude::*;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_blocks")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub user: String,
  pub blocked_at: DateTimeWithTimeZone,
  pub blocked_by: Option<String>,
  pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  pub note: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Users",
    from = "Column::User",
    to = "super::users::Column::Username",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  User,

  #[sea_orm(
    belongs_to = "Users",
    from = "Column::BlockedBy",
    to = "super::users::Column::Username",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  BlockedByUser,
}

impl ActiveModelBehavior for ActiveModel {}

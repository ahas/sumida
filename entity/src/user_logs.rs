use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

use super::users::Entity as Users;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum SignInMethod {
  #[serde(rename = "password")]
  #[sea_orm(string_value = "password")]
  Password,
  #[serde(rename = "bearer")]
  #[sea_orm(string_value = "bearer")]
  Bearer,
  #[serde(rename = "api-key")]
  #[sea_orm(string_value = "api-key")]
  ApiKey,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum SignInCode {
  #[serde(rename = "success")]
  #[sea_orm(num_value = 0)]
  Success,
  #[serde(rename = "account-not-found")]
  #[sea_orm(num_value = 1)]
  AccountNotFound,
  #[serde(rename = "password-mismatch")]
  #[sea_orm(num_value = 2)]
  PasswordMismatch,
  #[serde(rename = "account-inactive")]
  #[sea_orm(num_value = 3)]
  AccountInactive,
  #[serde(rename = "account-blocked")]
  #[sea_orm(num_value = 4)]
  AccountBlocked,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_logs")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub user_id: i32,
  pub method: SignInMethod,
  pub code: SignInCode,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family: String,
  pub os_major: Option<String>,
  pub os_minor: Option<String>,
  pub os_patch: Option<String>,
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  pub device_brand: Option<String>,
  pub device_model: Option<String>,

  pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Users",
    from = "Column::UserId",
    to = "super::users::Column::Id"
    on_update = "Cascade",
    on_delete = "Cascade",
  )]
  User,
}

impl Related<Users> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

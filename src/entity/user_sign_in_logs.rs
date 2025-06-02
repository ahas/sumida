use sea_orm::prelude::*;
use sea_orm::InsertResult;
use sea_orm::Set;
use serde::{Deserialize, Serialize};

use crate::service::user::session::UserEnvironment;

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
  #[sea_orm(num_value = 0)]
  Success,
  #[sea_orm(num_value = 1)]
  AccountNotFound,
  #[sea_orm(num_value = 2)]
  PasswordMismatch,
  #[sea_orm(num_value = 3)]
  AccountDisabled,
  #[sea_orm(num_value = 4)]
  AccountBlocked,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_sign_in_logs")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub user: String,
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
    from = "Column::User",
    to = "super::users::Column::Username"
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

impl Entity {
  pub async fn write<'db, C: ConnectionTrait>(
    username: String,
    method: SignInMethod,
    code: SignInCode,
    user_env: UserEnvironment,
    db: &'db C,
  ) -> Result<InsertResult<ActiveModel>, DbErr> {
    Self::insert(ActiveModel {
      user: Set(username),
      method: Set(method),
      code: Set(code),
      ip_address: Set(user_env.ip_address),
      user_agent: Set(user_env.user_agent),
      device_family: Set(user_env.device_family),
      device_brand: Set(user_env.device_brand),
      device_model: Set(user_env.device_model),
      os_family: Set(user_env.os_family),
      os_major: Set(user_env.os_major),
      os_minor: Set(user_env.os_minor),
      os_patch: Set(user_env.os_patch),
      os_patch_minor: Set(user_env.os_patch_minor),
      ..Default::default()
    })
    .exec(db)
    .await
  }
}

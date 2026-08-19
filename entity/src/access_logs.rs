use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

use super::users::Entity as Users;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum AccessMethod {
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
pub enum AccessResultCode {
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
  #[serde(rename = "account-banned")]
  #[sea_orm(num_value = 4)]
  AccountBanned,
}

/// Reads one segment of a query string as an authentication method. `OneOrMany` runs every
/// comma-separated piece through this, so an unknown value fails to parse and is refused with a
/// 400 rather than quietly dropped.
impl std::str::FromStr for AccessMethod {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    <Self as sea_orm::ActiveEnum>::try_from_value(&s.to_string())
  }
}

/// The same, for the result. The stored value is a number, so the names cannot be delegated to the
/// column's own conversion; they are the ones the wire uses, which is what a caller reads out of
/// the resource's declaration and sends back.
impl std::str::FromStr for AccessResultCode {
  type Err = DbErr;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(match s {
      "success" => Self::Success,
      "account-not-found" => Self::AccountNotFound,
      "password-mismatch" => Self::PasswordMismatch,
      "account-inactive" => Self::AccountInactive,
      "account-banned" => Self::AccountBanned,
      other => return Err(DbErr::Type(format!("unknown access result code `{other}`"))),
    })
  }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "access_logs")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id:      i32,
  pub user_id: i32,
  pub method:  AccessMethod,
  pub code:    AccessResultCode,
  #[sea_orm(nullable)]
  pub count:   Option<i32>,

  pub ip_address: String,
  pub user_agent: String,

  pub os_family:      String,
  #[sea_orm(nullable)]
  pub os_major:       Option<String>,
  #[sea_orm(nullable)]
  pub os_minor:       Option<String>,
  #[sea_orm(nullable)]
  pub os_patch:       Option<String>,
  #[sea_orm(nullable)]
  pub os_patch_minor: Option<String>,

  pub device_family: String,
  #[sea_orm(nullable)]
  pub device_brand:  Option<String>,
  #[sea_orm(nullable)]
  pub device_model:  Option<String>,
  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at:    DateTimeWithTimeZone,
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

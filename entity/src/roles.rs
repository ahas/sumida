use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Ability {
  #[serde(default)]
  pub create: bool,
  #[serde(default)]
  pub update: bool,
  #[serde(default)]
  pub read: bool,
  #[serde(default)]
  pub delete: bool,
}

impl Default for Ability {
  fn default() -> Self {
    Self {
      create: false,
      update: false,
      read: false,
      delete: false,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SumidaRoleAbility {
  #[serde(default)]
  pub role: Ability,
  #[serde(default)]
  pub user: Ability,
  #[serde(default)]
  pub user_invitation: Ability,
  #[serde(default)]
  pub user_block: Ability,
  #[serde(default)]
  pub user_log: Ability,
  #[serde(default)]
  pub organization: Ability,
  #[serde(default)]
  pub api_key: Ability,
}

impl Default for SumidaRoleAbility {
  fn default() -> Self {
    Self {
      role: Ability::default(),
      user: Ability::default(),
      user_invitation: Ability::default(),
      user_block: Ability::default(),
      user_log: Ability::default(),
      organization: Ability::default(),
      api_key: Ability::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "roles")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub name: String,
  pub description: Option<String>,
  #[sea_orm(column_type = "JsonBinary")]
  pub ability: Json,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

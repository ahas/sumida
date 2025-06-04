use sea_orm::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Ability {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub create: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub update: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub read: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub delete: Option<bool>,
}

impl Default for Ability {
  fn default() -> Self {
    Self {
      create: Some(false),
      update: Some(false),
      read: Some(false),
      delete: Some(false),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DefaultRoleAbility {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub role: Option<Ability>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub user: Option<Ability>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub organization: Option<Ability>,
}

impl Default for DefaultRoleAbility {
  fn default() -> Self {
    Self {
      role: Some(Ability::default()),
      user: Some(Ability::default()),
      organization: Some(Ability::default()),
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

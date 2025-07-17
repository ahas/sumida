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

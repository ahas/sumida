use sea_orm::prelude::*;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "bans")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = true)]
  pub id: i32,

  #[sea_orm(indexed, nullable)]
  pub user_id:    Option<i32>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub ip_address: Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub user_agent: Option<String>,

  #[sea_orm(default_value = "*", indexed, nullable)]
  pub os_family:      Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub os_major:       Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub os_minor:       Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub os_patch:       Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub os_patch_minor: Option<String>,

  #[sea_orm(default_value = "*", indexed, nullable)]
  pub device_family: Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub device_brand:  Option<String>,
  #[sea_orm(default_value = "*", indexed, nullable)]
  pub device_model:  Option<String>,

  #[sea_orm(default_value = "CURRENT_TIMESTAMP")]
  pub created_at: DateTimeWithTimeZone,
  #[sea_orm(nullable)]
  pub created_by: Option<i32>,
  #[sea_orm(nullable)]
  pub reason:     Option<String>,
  #[sea_orm(nullable)]
  pub expires_at: Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub lifted_at:  Option<DateTimeWithTimeZone>,
  #[sea_orm(nullable)]
  pub note:       Option<String>,
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

  #[sea_orm(
    belongs_to = "Users",
    from = "Column::CreatedBy",
    to = "super::users::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  CreatedUser,
}

impl Related<Users> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

pub struct CreatedUserLink;

impl Linked for CreatedUserLink {
  type FromEntity = Entity;
  type ToEntity = Users;

  fn link(&self) -> Vec<RelationDef> {
    vec![Relation::CreatedUser.def()]
  }
}

impl ActiveModelBehavior for ActiveModel {}

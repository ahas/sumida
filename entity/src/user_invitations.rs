use chrono::{Duration, Utc};
use sea_orm::ActiveValue::*;
use sea_orm::prelude::*;

// use crate::core::db;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_invitations")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub uuid: Uuid,
  pub user: String,

  pub expired_at: DateTimeWithTimeZone,
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
  pub async fn create<'db, S, C>(username: S, uuid: Uuid, db: &'db C) -> Result<(), DbErr>
  where
    S: AsRef<str>,
    C: ConnectionTrait,
  {
    let username = username.as_ref();
    let active_model = ActiveModel {
      user: Set(username.to_owned()),
      uuid: Set(uuid),
      expired_at: Set((Utc::now() + Duration::days(30)).fixed_offset()),
      ..Default::default()
    };

    let exists = Self::find_by_id(uuid).count(db).await? > 0;

    if exists {
      Self::update(active_model).exec(db).await?;
    } else {
      Self::insert(active_model).exec(db).await?;
    }

    Ok(())
  }
}

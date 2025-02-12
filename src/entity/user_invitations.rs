use crate::core::db;

use chrono::{Duration, Utc};
use sea_orm::prelude::*;
use sea_orm::ActiveValue::*;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_invitations")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub uuid: Uuid,
  pub username: String,

  pub expired_at: DateTimeWithTimeZone,
  pub created_at: DateTimeWithTimeZone,
}

impl Entity {
  pub async fn create<S>(username: S, uuid: Uuid) -> Result<(), DbErr>
  where
    S: AsRef<str>,
  {
    let db_conn = db::get_connection();
    let username = username.as_ref();
    let active_model = ActiveModel {
      username: Set(username.to_owned()),
      uuid: Set(uuid),
      expired_at: Set((Utc::now() + Duration::days(30)).fixed_offset()),
      ..Default::default()
    };

    let exists = Self::find_by_id(uuid).count(db_conn).await? > 0;

    if exists {
      Self::update(active_model).exec(db_conn).await?;
    } else {
      Self::insert(active_model).exec(db_conn).await?;
    }

    Ok(())
  }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Users",
    from = "Column::Username",
    to = "super::users::Column::Username"
    on_update = "Cascade",
    on_delete = "Cascade",
  )]
  Users,
}

impl Related<Users> for Entity {
  fn to() -> RelationDef {
    Relation::Users.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

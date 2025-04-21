use sea_orm::{prelude::*, ActiveValue::*, DatabaseTransaction};

use crate::core::error::SumidaError;

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_verifications")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub username: String,
  pub kind: String,
  pub method: String,
  pub code: String,

  pub expired_at: DateTimeWithTimeZone,
  pub created_at: DateTimeWithTimeZone,
}

impl Entity {
  pub async fn verify<K, U, C>(
    kind: K,
    username: U,
    code: C,
    txn: &DatabaseTransaction,
  ) -> Result<bool, SumidaError>
  where
    K: AsRef<str>,
    U: AsRef<str>,
    C: AsRef<str>,
  {
    let kind = kind.as_ref();
    let username = username.as_ref();
    let code = code.as_ref();

    let Some(_) = Users::find_by_id(username).one(txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    let is_verified = Entity::find()
      .filter(Column::Kind.eq(kind))
      .filter(Column::Username.eq(username))
      .filter(Column::Code.eq(code))
      .count(txn)
      .await?
      > 0;

    if is_verified {
      Self::delete(ActiveModel {
        kind: Set(kind.to_owned()),
        username: Set(username.to_owned()),
        code: Set(code.to_owned()),
        ..Default::default()
      })
      .exec(txn)
      .await?;
    }

    Ok(is_verified)
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

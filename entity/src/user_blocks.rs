use chrono::{FixedOffset, Utc};
use sea_orm::{Condition, QuerySelect, QueryTrait, prelude::*, sea_query::SimpleExpr};

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_blocks")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub user: Option<String>,
  pub ip_address: Option<String>,
  pub blocked_at: DateTimeWithTimeZone,
  pub blocked_by: Option<String>,
  pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  pub note: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "Users",
    from = "Column::User",
    to = "super::users::Column::Username",
    on_update = "Cascade",
    on_delete = "Cascade"
  )]
  User,

  #[sea_orm(
    belongs_to = "Users",
    from = "Column::BlockedBy",
    to = "super::users::Column::Username",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  BlockedByUser,
}

impl ActiveModelBehavior for ActiveModel {}

impl Entity {
  pub fn exists_expr() -> SimpleExpr {
    let now = Utc::now().fixed_offset();
    Expr::exists(
      Entity::find()
        .filter(
          Expr::col(Column::User).eq(Expr::col((super::users::Entity, super::users::Column::Username))),
        )
        .filter(Condition::any().add(Column::LiftedAt.is_null()))
        .filter(Condition::any().add(Column::ExpiresAt.is_null()).add(Column::ExpiresAt.gt(now)))
        .limit(1)
        .into_query(),
    )
  }
}

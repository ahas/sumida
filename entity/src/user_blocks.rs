use chrono::Utc;
use sea_orm::{Condition, QuerySelect, QueryTrait, prelude::*, sea_query::SimpleExpr};

use super::users::Entity as Users;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_blocks")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  pub user_id: Option<i32>,
  pub ip_address: Option<String>,
  pub blocked_at: DateTimeWithTimeZone,
  pub blocked_by: Option<i32>,
  pub reason: String,
  pub expires_at: Option<DateTimeWithTimeZone>,
  pub lifted_at: Option<DateTimeWithTimeZone>,
  pub note: Option<String>,
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
    from = "Column::BlockedBy",
    to = "super::users::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  BlockedByUser,
}

impl Related<Users> for Entity {
  fn to() -> RelationDef {
    Relation::User.def()
  }
}

pub struct BlockedByLink;

impl Linked for BlockedByLink {
  type FromEntity = Entity;
  type ToEntity = Users;

  fn link(&self) -> Vec<RelationDef> {
    vec![Relation::BlockedByUser.def()]
  }
}

impl ActiveModelBehavior for ActiveModel {}

impl Entity {
  pub fn exists_expr() -> SimpleExpr {
    let now = Utc::now().fixed_offset();
    Expr::exists(
      Entity::find()
        .select_only()
        .expr(Expr::val(1))
        .filter(
          Expr::col(Column::UserId).eq(Expr::col((super::users::Entity, super::users::Column::Id))),
        )
        .filter(Condition::any().add(Column::LiftedAt.is_null()))
        .filter(Condition::any().add(Column::ExpiresAt.is_null()).add(Column::ExpiresAt.gt(now)))
        .limit(1)
        .into_query(),
    )
  }
}

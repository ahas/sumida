pub mod dto;

use chrono::Utc;
use infra::db;
use infra::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Asterisk, Expr, SimpleExpr};
use sea_orm::{
  ColumnTrait, Condition, DeleteResult, IntoSimpleExpr, JoinType, PaginatorTrait, QueryFilter, QuerySelect,
  QueryTrait, RelationTrait, Select,
};
use sea_orm::{EntityTrait, InsertResult, IntoActiveModel, TransactionTrait};

use crate::service::ApiService;
use crate::util::ap::{Ap, ApGuard};
use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserBlockService {
  user_ctx: UserContext,
}

impl UserBlockService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

  pub fn exists_expr() -> SimpleExpr {
    let now = Utc::now().fixed_offset();
    Expr::exists(
      UserBlocks::find()
        .select_only()
        .expr(Expr::val(1))
        .filter(
          Expr::col(entity::user_blocks::Column::UserId).eq(Expr::col((Users, entity::users::Column::Id))),
        )
        .filter(Condition::any().add(entity::user_blocks::Column::LiftedAt.is_null()))
        .filter(
          Condition::any()
            .add(entity::user_blocks::Column::ExpiresAt.is_null())
            .add(entity::user_blocks::Column::ExpiresAt.gt(now)),
        )
        .limit(1)
        .into_query(),
    )
  }

  pub async fn list(
    &self,
    filter: UserBlockFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserBlockListItem>, u64), SumidaError> {
    use entity::user_blocks::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(UserBlocks::find()));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "user_id" => Column::UserId.into_simple_expr(),
      "fullName" => Expr::col((Alias::new("u"), entity::users::Column::FullName)).into_simple_expr(),
      _ => Column::BlockedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<UserBlockListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetUserBlock>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let item = self
      .select(UserBlocks::find_by_id(id))
      .select_only()
      .expr(Expr::col((UserBlocks, Asterisk)))
      .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::User.def(), Alias::new("u"))
      .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::BlockedByUser.def(), Alias::new("b"))
      .into_partial_model::<GetUserBlock>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn create(
    &self,
    data: CreateUserBlock,
  ) -> Result<InsertResult<entity::user_blocks::ActiveModel>, SumidaError> {
    let active_model = data.into_active_model();
    let result = UserBlocks::insert(active_model).exec(db::get_connection()).await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateUserBlock,
  ) -> Result<entity::user_blocks::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("update").check(UserBlocks::find_by_id(id), &txn).await?;

    let mut active_model = data.into_active_model();
    active_model.id = Set(id);

    let result = UserBlocks::update(active_model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("delete").check(UserBlocks::find_by_id(id), &txn).await?;

    let result = UserBlocks::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<Ap, UserBlocks> for UserBlockService {
  fn select(&self, query: Select<UserBlocks>) -> Select<UserBlocks> {
    self.ap().action("read").apply({
      let user_alias = Alias::new("u");
      let blocked_by_alias = Alias::new("b");

      query
        .select_only()
        .expr(Expr::col((UserBlocks, Asterisk)))
        .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::User.def(), user_alias.clone())
        .join_as(
          JoinType::LeftJoin,
          entity::user_blocks::Relation::BlockedByUser.def(),
          blocked_by_alias.clone(),
        )
    })
  }

  fn ap(&self) -> Ap {
    use entity::user_blocks::Column;

    Ap::new("user_block") //
      .owner(&self.user_ctx)
      .object(Expr::col((UserBlocks, Column::Id)).cast_as("varchar"))
  }
}

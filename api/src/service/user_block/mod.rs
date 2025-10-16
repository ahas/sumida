pub mod dto;

use chrono::Utc;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Asterisk, Expr, SimpleExpr};
use sea_orm::{
  ColumnTrait,
  Condition,
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  IntoSimpleExpr,
  JoinType,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  QueryTrait,
  RelationTrait,
  Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct UserBlockService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> UserBlockService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
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

    let txn = self.state.db().begin().await?;

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
    let txn = self.state.db().begin().await?;
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
    let result = UserBlocks::insert(active_model).exec(self.state.db()).await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateUserBlock,
  ) -> Result<entity::user_blocks::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", UserBlocks::find_by_id(id), &txn).await?;

    let mut active_model = data.into_active_model();
    active_model.id = Set(id);

    let result = UserBlocks::update(active_model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", UserBlocks::find_by_id(id), &txn).await?;

    let result = UserBlocks::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, UserBlocks> for UserBlockService<S> {
  fn default_query_with(query: Select<UserBlocks>) -> Select<UserBlocks> {
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
  }

  fn ap(&self) -> ApCondition {
    use entity::user_blocks::Column;

    ap::ns("user_block").who(&self.session).join(UserBlocks, Column::Id)
  }

  fn select(&self, query: sea_orm::Select<UserBlocks>) -> sea_orm::Select<UserBlocks> {
    self.ap().action("read").apply(Self::default_query_with(query))
  }
}

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

pub struct BanService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> BanService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub fn exists_expr() -> SimpleExpr {
    let now = Utc::now().fixed_offset();
    Expr::exists(
      Bans::find()
        .select_only()
        .expr(Expr::val(1))
        .filter(Expr::col(entity::bans::Column::UserId).eq(Expr::col((Users, entity::users::Column::Id))))
        .filter(Condition::any().add(entity::bans::Column::LiftedAt.is_null()))
        .filter(
          Condition::any()
            .add(entity::bans::Column::ExpiresAt.is_null())
            .add(entity::bans::Column::ExpiresAt.gt(now)),
        )
        .limit(1)
        .into_query(),
    )
  }

  pub async fn list(
    &self,
    filter: BanFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BanListItem>, u64), SumidaError> {
    use entity::bans::Column;

    let txn = self.state.db().begin().await?;

    let mut query = filter.apply(self.select(Bans::find()));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "user_id" => Column::UserId.into_simple_expr(),
      "fullName" => Expr::col((Alias::new("u"), entity::users::Column::FullName)).into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BanListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBan>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let item = self
      .select(Bans::find_by_id(id))
      .select_only()
      .expr(Expr::col((Bans, Asterisk)))
      .join_as(JoinType::LeftJoin, entity::bans::Relation::User.def(), Alias::new("u"))
      .join_as(JoinType::LeftJoin, entity::bans::Relation::CreatedUser.def(), Alias::new("b"))
      .into_partial_model::<GetBan>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn create(
    &self,
    data: CreateBan,
  ) -> Result<InsertResult<entity::bans::ActiveModel>, SumidaError> {
    let active_model = data.into_active_model();
    let result = Bans::insert(active_model).exec(self.state.db()).await?;

    Ok(result)
  }

  pub async fn update(&self, id: i32, data: UpdateBan) -> Result<entity::bans::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", Bans::find_by_id(id), &txn).await?;

    let mut active_model = data.into_active_model();
    active_model.id = Set(id);

    let result = Bans::update(active_model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", Bans::find_by_id(id), &txn).await?;

    let result = Bans::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, Bans> for BanService<S> {
  fn default_query_with(query: Select<Bans>) -> Select<Bans> {
    let user_alias = Alias::new("u");
    let created_by_alias = Alias::new("c");

    query
      .select_only()
      .expr(Expr::col((Bans, Asterisk)))
      .join_as(JoinType::LeftJoin, entity::bans::Relation::User.def(), user_alias.clone())
      .join_as(
        JoinType::LeftJoin,
        entity::bans::Relation::CreatedUser.def(),
        created_by_alias.clone(),
      )
  }

  fn ap(&self) -> ApCondition {
    use entity::bans::Column;

    ap::ns("ban").who(&self.session).join(Bans, Column::Id)
  }

  fn select(&self, query: sea_orm::Select<Bans>) -> sea_orm::Select<Bans> {
    self.ap().action("read").apply(Self::default_query_with(query))
  }
}

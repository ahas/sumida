pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{DeleteResult, IntoSimpleExpr, JoinType, PaginatorTrait, QuerySelect, RelationTrait};
use sea_orm::{EntityTrait, InsertResult, IntoActiveModel, TransactionTrait};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserBlockService;

impl UserBlockService {
  pub async fn list(
    filter: UserBlockFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserBlockListItem>, u64), SumidaError> {
    use entity::user_blocks::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let user_alias = Alias::new("u");
    let blocked_by_alias = Alias::new("b");

    let mut query = filter.apply(
      UserBlocks::find()
        .select_only()
        .expr(Expr::col((UserBlocks, Asterisk)))
        .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::User.def(), user_alias.clone())
        .join_as(
          JoinType::LeftJoin,
          entity::user_blocks::Relation::BlockedByUser.def(),
          blocked_by_alias.clone(),
        ),
    );

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "user_id" => Column::UserId.into_simple_expr(),
      "fullName" => Expr::col((user_alias.clone(), entity::users::Column::FullName)).into_simple_expr(),
      _ => Column::BlockedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<UserBlockListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetUserBlock>, SumidaError> {
    let db_conn = db::get_connection();

    let item = UserBlocks::find_by_id(id)
      .select_only()
      .expr(Expr::col((UserBlocks, Asterisk)))
      .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::User.def(), Alias::new("u"))
      .join_as(JoinType::LeftJoin, entity::user_blocks::Relation::BlockedByUser.def(), Alias::new("b"))
      .into_partial_model::<GetUserBlock>()
      .one(db_conn)
      .await?;

    Ok(item)
  }

  pub async fn create(
    data: CreateUserBlock,
  ) -> Result<InsertResult<entity::user_blocks::ActiveModel>, SumidaError> {
    let db_conn = db::get_connection();
    let active_model = data.into_active_model();
    let result = UserBlocks::insert(active_model).exec(db_conn).await?;

    Ok(result)
  }

  pub async fn update(id: i32, data: UpdateUserBlock) -> Result<entity::user_blocks::Model, SumidaError> {
    let db_conn = db::get_connection();
    let mut active_model = data.into_active_model();
    active_model.id = Set(id);

    let result = UserBlocks::update(active_model).exec(db_conn).await?;

    Ok(result)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(UserBlocks::delete_by_id(id).exec(db_conn).await?)
  }
}

pub mod dto;

use self::dto::*;
use crate::util::access::Access;
use crate::util::crud::Pagination;
use core::db;
use core::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardService;

impl BoardService {
  pub async fn list(
    user_ctx: &UserContext,
    filter: BoardFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardListItem>, u64), SumidaError> {
    use entity::bs::boards::Column;

    let db = db::get_connection();
    let txn = db.begin().await?;

    let mut query = filter.apply(Boards::find().select_only().expr(Expr::col((Boards, Asterisk))));

    query =
      Access::new().namespace("board").action("viewer").owner(user_ctx).filter(query, (Boards, Column::Id));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetBoard>, SumidaError> {
    Ok(Boards::find_by_id(id).into_partial_model::<GetBoard>().one(db::get_connection()).await?)
  }

  pub async fn create(
    data: CreateBoard,
  ) -> Result<InsertResult<entity::bs::boards::ActiveModel>, SumidaError> {
    Ok(Boards::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(id: i32, data: UpdateBoard) -> Result<entity::bs::boards::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(Boards::update(am).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(Boards::delete_by_id(id).exec(db::get_connection()).await?)
  }
}

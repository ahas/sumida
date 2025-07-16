pub mod dto;

use self::dto::*;
use crate::util::crud::Pagination;
use crate::util::srn::Srn;
use core::db;
use core::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::Select;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardService;

impl BoardService {
  pub fn select(user_ctx: &UserContext, query: Select<Boards>) -> Select<Boards> {
    Self::srn(user_ctx).action("read").apply(query.select_only().expr(Expr::col((Boards, Asterisk))))
  }

  pub fn srn(user_ctx: &UserContext) -> Srn {
    use entity::bs::boards::Column;

    Srn::new("board:?")
      .namespace("sumida")
      .owner(user_ctx)
      .column(Expr::col((Boards, Column::Id)).cast_as("varchar"))
  }

  pub async fn list(
    user_ctx: &UserContext,
    filter: BoardFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardListItem>, u64), SumidaError> {
    use entity::bs::boards::Column;

    let db = db::get_connection();
    let txn = db.begin().await?;

    let mut query = filter.apply(Self::select(user_ctx, Boards::find()));
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

  pub async fn get(user_ctx: &UserContext, id: i32) -> Result<Option<GetBoard>, SumidaError> {
    Ok(
      Self::select(user_ctx, Boards::find_by_id(id))
        .into_partial_model::<GetBoard>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn create(
    user_ctx: &UserContext,
    data: CreateBoard,
  ) -> Result<InsertResult<entity::bs::boards::ActiveModel>, SumidaError> {
    let db = db::get_connection();
    Self::srn(user_ctx).action("create").check(db).await?;
    Ok(Boards::insert(data.into_active_model()).exec(db).await?)
  }

  pub async fn update(
    user_ctx: &UserContext,
    id: i32,
    data: UpdateBoard,
  ) -> Result<entity::bs::boards::Model, SumidaError> {
    let db = db::get_connection();
    Self::srn(user_ctx).action("update").check_with(Boards::find_by_id(id), db).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(Boards::update(am).exec(db).await?)
  }

  pub async fn delete(user_ctx: &UserContext, id: i32) -> Result<DeleteResult, SumidaError> {
    let db = db::get_connection();
    Self::srn(user_ctx).action("delete").check_with(Boards::find_by_id(id), db).await?;
    Ok(Boards::delete_by_id(id).exec(db).await?)
  }
}

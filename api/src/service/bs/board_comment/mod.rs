pub mod dto;

use self::dto::*;
use crate::util::crud::Pagination;
use infra::db;
use entity::bs::board_comments::Column;
use entity::bs::board_comments::Entity as BoardComments;
use error::SumidaError;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardCommentService;

impl BoardCommentService {
  pub async fn list(
    filter: BoardCommentFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardCommentListItem>, u64), SumidaError> {
    let db = db::get_connection();
    let txn = db.begin().await?;

    let mut query =
      filter.apply(BoardComments::find().select_only().expr(Expr::col((BoardComments, Asterisk))));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      _ => Column::Id.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardCommentListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetBoardComment>, SumidaError> {
    Ok(
      BoardComments::find_by_id(id)
        .into_partial_model::<GetBoardComment>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn create(
    data: CreateBoardComment,
  ) -> Result<InsertResult<entity::bs::board_comments::ActiveModel>, SumidaError> {
    Ok(BoardComments::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(
    id: i32,
    data: UpdateBoardComment,
  ) -> Result<entity::bs::board_comments::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardComments::update(am).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardComments::delete_by_id(id).exec(db::get_connection()).await?)
  }
}

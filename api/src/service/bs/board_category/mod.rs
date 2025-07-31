pub mod dto;

use infra::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, PaginatorTrait, QuerySelect,
  TransactionTrait,
};

use self::dto::*;
use crate::util::pagination::Pagination;

pub struct BoardCategoryService;

impl BoardCategoryService {
  pub async fn list(
    filter: BoardCategoryFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardCategoryListItem>, u64), SumidaError> {
    use entity::bs::board_categories::Column;

    let txn = db::get_connection().begin().await?;

    let mut query =
      filter.apply(BoardCategories::find().select_only().expr(Expr::col((BoardCategories, Asterisk))));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardCategoryListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetBoardCategory>, SumidaError> {
    Ok(
      BoardCategories::find_by_id(id)
        .into_partial_model::<GetBoardCategory>()
        .one(db::get_connection())
        .await?,
    )
  }

  pub async fn create(
    data: CreateBoardCategory,
  ) -> Result<InsertResult<entity::bs::board_categories::ActiveModel>, SumidaError> {
    Ok(BoardCategories::insert(data.into_active_model()).exec(db::get_connection()).await?)
  }

  pub async fn update(
    id: i32,
    data: UpdateBoardCategory,
  ) -> Result<entity::bs::board_categories::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardCategories::update(am).exec(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardCategories::delete_by_id(id).exec(db::get_connection()).await?)
  }
}

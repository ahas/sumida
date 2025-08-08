pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, PaginatorTrait, QuerySelect,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::util::pagination::Pagination;

pub struct BoardCategoryService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardCategoryService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardCategoryFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardCategoryListItem>, u64), SumidaError> {
    use entity::bs::board_categories::Column;

    let txn = self.state.db().begin().await?;

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

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardCategory>, SumidaError> {
    Ok(BoardCategories::find_by_id(id).into_partial_model::<GetBoardCategory>().one(self.state.db()).await?)
  }

  pub async fn create(
    &self,
    data: CreateBoardCategory,
  ) -> Result<InsertResult<entity::bs::board_categories::ActiveModel>, SumidaError> {
    Ok(BoardCategories::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardCategory,
  ) -> Result<entity::bs::board_categories::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardCategories::update(am).exec(self.state.db()).await?)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardCategories::delete_by_id(id).exec(self.state.db()).await?)
  }
}

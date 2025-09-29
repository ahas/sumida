pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::Select;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, PaginatorTrait, QuerySelect,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct BoardCategoryService<S: SumidaState> {
  state: S,
  #[allow(dead_code)]
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

    let db = self.state.db();
    let mut query = filter.apply(self.select(BoardCategories::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardCategoryListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardCategory>, SumidaError> {
    Ok(
      self
        .select(BoardCategories::find_by_id(id))
        .into_partial_model::<GetBoardCategory>()
        .one(self.state.db())
        .await?,
    )
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
    let txn = self.state.db().begin().await?;
    self.try_access("update", BoardCategories::find_by_id(id), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardCategories::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", BoardCategories::find_by_id(id), &txn).await?;
    let result = BoardCategories::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardCategories> for BoardCategoryService<S> {
  fn default_query_with(query: Select<BoardCategories>) -> Select<BoardCategories> {
    query.select_only().expr(Expr::col((BoardCategories, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::board_categories::Column;
    ap::ns("board_category").who(&self.session).join(BoardCategories, Column::Id)
  }
}

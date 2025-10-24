pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  IntoSimpleExpr,
  PaginatorTrait,
  QuerySelect,
  Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

pub struct BoardGroupService<S: SumidaState> {
  state:   S,
  #[allow(dead_code)]
  session: UserSession,
}

impl<S: SumidaState> BoardGroupService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardGroupFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardGroupListItem>, u64), SumidaError> {
    use entity::bs::board_groups::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(BoardGroups::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardGroupListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardGroup>, SumidaError> {
    Ok(
      self
        .select(BoardGroups::find_by_id(id))
        .into_partial_model::<GetBoardGroup>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateBoardGroup,
  ) -> Result<InsertResult<entity::bs::board_groups::ActiveModel>, SumidaError> {
    Ok(BoardGroups::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardGroup,
  ) -> Result<entity::bs::board_groups::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", BoardGroups::find_by_id(id), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardGroups::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", BoardGroups::find_by_id(id), &txn).await?;
    let result = BoardGroups::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardGroups> for BoardGroupService<S> {
  fn default_query_with(query: Select<BoardGroups>) -> Select<BoardGroups> {
    query.select_only().expr(Expr::col((BoardGroups, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::board_groups::Column;
    ap::ns("board_group").who(&self.session).join(BoardGroups, Column::Id)
  }
}

pub mod dto;

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::Select;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    BoardService { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardListItem>, u64), SumidaError> {
    use entity::bs::boards::Column;

    let txn = self.state.db().begin().await?;

    let mut query = filter.apply(self.select(Boards::find()));
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

  pub async fn get(&self, id: i32) -> Result<Option<GetBoard>, SumidaError> {
    Ok(self.select(Boards::find_by_id(id)).into_partial_model::<GetBoard>().one(self.state.db()).await?)
  }

  pub async fn create(
    &self,
    data: CreateBoard,
  ) -> Result<InsertResult<entity::bs::boards::ActiveModel>, SumidaError> {
    Ok(Boards::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(&self, id: i32, data: UpdateBoard) -> Result<entity::bs::boards::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.ap().object(id).action("update").try_access(&txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = Boards::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.ap().object(id).action("delete").try_access(&txn).await?;
    let result = Boards::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, Boards> for BoardService<S> {
  fn default_query_with(query: Select<Boards>) -> Select<Boards> {
    query //
      .select_only()
      .expr(Expr::col((Boards, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::boards::Column;

    ap::ns("board") //
      .who(&self.session)
      .object_ref(Expr::col((Boards, Column::Id)).cast_as("varchar"))
  }
}

pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  ColumnTrait, ConnectionTrait, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, PaginatorTrait,
  QueryFilter, QuerySelect, Select, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

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
    use entity::bps::boards::Column;

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
    Ok(
      self
        .select(Boards::find_by_id(id))
        .into_partial_model::<GetBoard>()
        .one(self.state.db())
        .await?,
    )
  }

  async fn update_topics<'db, C: ConnectionTrait>(
    board_id: i32,
    topics: Option<Vec<String>>,
    db: &'db C,
  ) -> Result<(), SumidaError> {
    BoardTopics::delete_many()
      .filter(BoardTopicsColumn::BoardId.eq(board_id))
      .exec(db)
      .await?;

    if let Some(topics) = topics {
      for (order, name) in topics.iter().enumerate() {
        BoardTopics::insert(BoardTopicsActiveModel {
          order: Set(order as i32),
          name: Set(name.to_owned()),
          description: NotSet,
          board_id: Set(board_id),
          ..Default::default()
        })
        .exec(db)
        .await?;
      }
    }

    Ok(())
  }

  pub async fn create(
    &self,
    data: CreateBoard,
  ) -> Result<InsertResult<entity::bps::boards::ActiveModel>, SumidaError> {
    let db = self.state.db();
    let txn = db.begin().await?;
    let topics = data.topics.clone();

    let result = Boards::insert(data.into_active_model()).exec(&txn).await?;
    Self::update_topics(result.last_insert_id, topics.into(), &txn).await?;

    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoard,
  ) -> Result<entity::bps::boards::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", Boards::find_by_id(id), &txn).await?;

    let topics = data.topics.clone();
    Self::update_topics(id, topics.into(), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = Boards::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", Boards::find_by_id(id), &txn).await?;

    // Soft delete: hide the board (and, via the post read filter, its posts) without dropping rows.
    Boards::update_many()
      .set(entity::bps::boards::ActiveModel {
        deleted_at: Set(Some(chrono::Utc::now().fixed_offset())),
        ..Default::default()
      })
      .filter(entity::bps::boards::Column::Id.eq(id))
      .exec(&txn)
      .await?;

    txn.commit().await?;

    Ok(())
  }
}

impl<S: SumidaState> ApiService<ApCondition, Boards> for BoardService<S> {
  fn default_query_with(query: Select<Boards>) -> Select<Boards> {
    query //
      .select_only()
      .expr(Expr::col((Boards, Asterisk)))
      .left_join(BoardGroups)
      // Soft-deleted boards are never returned.
      .filter(entity::bps::boards::Column::DeletedAt.is_null())
  }

  fn ap(&self) -> ApCondition {
    ap::ns("board").who(&self.session).join(Boards, entity::bps::boards::Column::Id)
  }
}

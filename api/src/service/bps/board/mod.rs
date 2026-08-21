pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace board service = board`).
const RESOURCE: &str = "board";

pub struct BoardService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> BoardService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    BoardService { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  fn default_query_with(query: Select<Boards>) -> Select<Boards> {
    query //
      .select_only()
      .expr(Expr::col((Boards, Asterisk)))
      .left_join(BoardGroups)
      // Soft-deleted boards are never returned.
      .filter(entity::bps::boards::Column::DeletedAt.is_null())
  }

  /// 행 스코프는 `board#view` 술어 하나 — 그룹과의 교집합, 조직 관리자, 차단이 전부 규칙
  /// 안에 있다.
  fn select(&self, query: Select<Boards>) -> Result<Select<Boards>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `BoardFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: BoardFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BoardListItem>, u64), SumidaError> {
    use entity::bps::boards::Column;

    let txn = self.state.db().begin().await?;

    let mut query = conditions.apply::<BoardFilter, _>(filter.apply(self.select(Boards::find())?))?;
    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by_schema::<BoardFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoard>, SumidaError> {
    Ok(
      self
        .select(Boards::find_by_id(id))?
        .into_partial_model::<GetBoard>()
        .one(self.state.db())
        .await?,
    )
  }

  async fn update_topics<C: ConnectionTrait>(
    board_id: i32,
    topics: Option<Vec<String>>,
    db: &C,
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
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

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
    // `board` 에는 `remove` 가 없다. `own = owner | org->manage_board |
    // service:board->operate_delete` 자체가 "지울 수 있다"이므로 삭제는 `own` 으로 인가한다.
    Authz::try_check(RESOURCE, id, "own", &self.subject(), &txn).await?;

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

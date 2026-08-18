pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, PaginatorTrait, QuerySelect,
  Select, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace board_topic service = board_topic`).
const RESOURCE: &str = "board_topic";

pub struct BoardTopicService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardTopicService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  fn default_query_with(query: Select<BoardTopics>) -> Select<BoardTopics> {
    query.select_only().expr(Expr::col((BoardTopics, Asterisk)))
  }

  /// 행 스코프는 `board_topic#view` 술어 하나 — 토픽은 판정을 자기 게시판에 그대로 위임한다
  /// (`board->view`).
  fn select(&self, query: Select<BoardTopics>) -> Result<Select<BoardTopics>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  pub async fn list(
    &self,
    filter: BoardTopicFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardTopicListItem>, u64), SumidaError> {
    use entity::bps::board_topics::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(BoardTopics::find())?);
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardTopicListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardTopic>, SumidaError> {
    Ok(
      self
        .select(BoardTopics::find_by_id(id))?
        .into_partial_model::<GetBoardTopic>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateBoardTopic,
  ) -> Result<InsertResult<entity::bps::board_topics::ActiveModel>, SumidaError> {
    Ok(BoardTopics::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardTopic,
  ) -> Result<entity::bps::board_topics::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardTopics::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;
    let result = BoardTopics::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}

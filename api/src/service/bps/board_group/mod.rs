pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace board_group service = board_group`).
const RESOURCE: &str = "board_group";

pub struct BoardGroupService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardGroupService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  fn default_query_with(query: Select<BoardGroups>) -> Select<BoardGroups> {
    query.select_only().expr(Expr::col((BoardGroups, Asterisk)))
  }

  /// 행 스코프는 `board_group#view` 술어 하나 — 상위 그룹의 부여(`viewer^`)와 차단(`blocked^`)이
  /// 규칙 안에 함께 들어 있다.
  fn select(&self, query: Select<BoardGroups>) -> Result<Select<BoardGroups>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `BoardGroupFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: BoardGroupFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BoardGroupListItem>, u64), SumidaError> {
    use entity::bps::board_groups::Column;

    let db = self.state.db();
    let mut query =
      conditions.apply::<BoardGroupFilter, _>(filter.apply(self.select(BoardGroups::find())?))?;
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<BoardGroupFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardGroupListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardGroup>, SumidaError> {
    Ok(
      self
        .select(BoardGroups::find_by_id(id))?
        .into_partial_model::<GetBoardGroup>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateBoardGroup,
  ) -> Result<InsertResult<entity::bps::board_groups::ActiveModel>, SumidaError> {
    Ok(BoardGroups::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardGroup,
  ) -> Result<entity::bps::board_groups::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardGroups::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    // `board_group` 에는 `remove` 가 없다. `own = owner^ | service:board_group->operate_delete`
    // 자체가 "지울 수 있다"이므로 삭제는 `own` 으로 인가한다(설계 §1.4).
    Authz::try_check(RESOURCE, id, "own", &self.subject(), &txn).await?;
    let result = BoardGroups::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}

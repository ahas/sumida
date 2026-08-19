pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{
  DeleteResult, EntityTrait, JoinType, PaginatorTrait, QuerySelect, RelationTrait, Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace access_log service = access_log`).
const RESOURCE: &str = "access_log";

pub struct AccessLogService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> AccessLogService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  fn default_query_with(query: Select<AccessLogs>) -> Select<AccessLogs> {
    let user_alias = Alias::new("user");
    let col_user_id = Expr::col((user_alias.clone(), "id"));
    let col_username = Expr::col((user_alias.clone(), "username"));
    let col_full_name = Expr::col((user_alias.clone(), "full_name"));

    query
      .select_only()
      .expr(Expr::col((entity::access_logs::Entity, Asterisk)))
      .expr_as(col_user_id, "user_id")
      .expr_as(col_username, "user_username")
      .expr_as(col_full_name, "user_full_name")
      .join_as(JoinType::LeftJoin, entity::access_logs::Relation::User.def(), user_alias.clone())
  }

  /// 행 스코프는 `access_log#view` 술어 하나로 붙는다 — filter/COUNT/ORDER BY/LIMIT 이 한
  /// 문장에 그대로 남는다.
  fn select(&self, query: Select<AccessLogs>) -> Result<Select<AccessLogs>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `AccessLogFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: AccessLogFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<AccessLogListItem>, u64), SumidaError> {
    use entity::access_logs::Column;

    let db = self.state.db();

    let mut query =
      conditions.apply::<AccessLogFilter, _>(filter.apply(self.select(AccessLogs::find())?))?;
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<AccessLogFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<AccessLogListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetAccessLog>, SumidaError> {
    Ok(
      self
        .select(AccessLogs::find_by_id(id))?
        .into_model::<GetAccessLog>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    let result = AccessLogs::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

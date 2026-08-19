pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  PaginatorTrait,
  QuerySelect,
  Select,
  Set,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace organization service = organization`).
const RESOURCE: &str = "organization";

pub struct OrganizationService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> OrganizationService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 행 스코프는 `organization#view` 술어 하나 — 소속 구성원(`member`)과 상위 조직의
  /// 관리자(`manager^`)가 규칙 한 줄에 함께 들어 있다.
  fn select(&self, query: Select<Organizations>) -> Result<Select<Organizations>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `OrganizationFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: OrganizationFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<OrganizationListItem>, u64), SumidaError> {
    use entity::organizations::Column;

    let db = self.state.db();
    let mut query =
      conditions.apply::<OrganizationFilter, _>(filter.apply(self.select(Organizations::find())?))?;
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by_schema::<OrganizationFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<OrganizationListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetOrganization>, SumidaError> {
    Ok(
      self
        .select(Organizations::find_by_id(id))?
        .into_model::<GetOrganization>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateOrganization,
  ) -> Result<InsertResult<entity::organizations::ActiveModel>, SumidaError> {
    Ok(Organizations::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateOrganization,
  ) -> Result<entity::organizations::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Organizations::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    let result = Organizations::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

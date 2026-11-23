pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ConnectionTrait,
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
use crate::authz::write::{SubjectKey, TupleFilter};
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

  /// 조직을 어느 부모 아래 두는 것은 그 부모의 관리자에게 이 조직을 맡기는 일이고, 부모에서
  /// 떼어 내는 것은 그 관리자에게서 빼앗는 일이다. 그래서 어느 쪽이든 그 부모를 편집할 수 있는
  /// 사람만 한다(`organization:<부모>#edit`).
  async fn may_place_under<C: ConnectionTrait>(
    &self,
    parent_id: Option<i32>,
    db: &C,
  ) -> Result<(), SumidaError> {
    match parent_id {
      Some(parent_id) => Authz::try_check(RESOURCE, parent_id, "edit", &self.subject(), db).await,
      None => Ok(()),
    }
  }

  pub async fn create(
    &self,
    data: CreateOrganization,
  ) -> Result<InsertResult<entity::organizations::ActiveModel>, SumidaError> {
    let db = self.state.db();

    // id 는 서버가 정한다. 지워진 조직의 id 를 다시 고르면 그 id 앞으로 남은 부여를 물려받는다.
    if data.id.is_some() && !Authz::is_superuser(&self.subject(), db).await? {
      return Err(SumidaError::NoAccessPermission);
    }
    let parent_id: Option<i32> = data.parent_id.clone().into();
    self.may_place_under(parent_id, db).await?;

    Ok(Organizations::insert(data.into_active_model()).exec(db).await?)
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

    // 부모를 바꾸면 위쪽 관리자의 도달이 바뀐다. 떠나는 부모와 새 부모 둘 다에 대해 물어야, 하위
    // 조직 관리자가 자기 조직을 상위 관리에서 빼내거나 남의 트리로 옮기지 못한다.
    if let Set(parent_id) = &model.parent_id {
      let Some(current) = Organizations::find_by_id(id).one(&txn).await? else {
        return Err(SumidaError::NoData("Organization".to_owned(), id.to_string()));
      };
      if current.parent_id != *parent_id {
        self.may_place_under(current.parent_id, &txn).await?;
        self.may_place_under(*parent_id, &txn).await?;
      }
    }

    let result = Organizations::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    // 조직이 사라지면 그 구성원 집합 앞으로 남은 부여와 그 조직에 걸린 부여도 함께 사라진다.
    // 남겨 두면 같은 id 로 다시 생긴 조직이 그것을 물려받는다.
    Authz::revoke_all_of(&SubjectKey::userset(RESOURCE, &id.to_string(), "member"), &txn).await?;
    Authz::revoke_unchecked(TupleFilter::object(RESOURCE, &id.to_string()), &txn).await?;

    let result = Organizations::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, Select,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::subject::SubjectSet;
use crate::authz::write::SubjectKey;
use crate::authz::Authz;
use crate::service::grants::{from_service_grants, to_service_grants};
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace role service = role`).
const RESOURCE: &str = "role";

pub struct RoleService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> RoleService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 역할이 부여를 **받는** 쪽으로 등장할 때의 주체 키.
  ///
  /// 종전의 `role:<id>` 문자열이 사실 뜻하던 것 — "이 역할의 모든 구성원" — 을 그대로 쓴
  /// 것이다. `role:<id>` 라는 단말 주체로 적으면 아무에게도 닿지 않는다.
  fn holder(id: i32) -> SubjectKey {
    SubjectKey::userset("role", &id.to_string(), "member")
  }

  /// 역할은 전역 설정이다 — 만든 사람도 조직도 없으므로 `role#view` 는 명시적 `admin` 부여나
  /// `service:role->operate_read` 로만 열린다.
  fn select(&self, query: Select<Roles>) -> Result<Select<Roles>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  pub async fn list(
    &self,
    filter: RoleFilter,
    pagination: Pagination,
  ) -> Result<(Vec<RoleListItem>, u64), SumidaError> {
    use entity::roles::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(Roles::find())?);
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::Name,
      "createdAt" => Column::CreatedAt,
      "updatedAt" => Column::UpdatedAt,
      _ => Column::Id,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<RoleListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetRole>, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "view", &self.subject(), &txn).await?;

    let Some(mut role) = Roles::find_by_id(id).into_model::<GetRole>().one(&txn).await? else {
      return Ok(None);
    };

    // 이 역할이 보유한 라우트 게이트. 화면이 늘 읽어 온 모양 그대로다(§9.4).
    let held = SubjectSet::from_parts(vec![Self::holder(id).key()], Vec::new());
    role.access_policies = from_service_grants(Authz::service_grants(&held, &txn).await?);
    let role = Some(role);
    txn.commit().await?;

    Ok(role)
  }

  pub async fn create(
    &self,
    data: CreateRole,
  ) -> Result<InsertResult<entity::roles::ActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;
    // 권한 검사는 `replace_service_grants` 안에서 네임스페이스마다
    // `service:<namespace>#grant` 로 이뤄진다. 종전의 `assert_policy_within_authority` 는
    // 와일드카드만 거절했으므로, 자기가 갖지도 않은 구체적 권한을 역할에 달아 주는 것은
    // 그대로 통과했다.
    let grants = to_service_grants(&data.access_policies)?;
    let result = Roles::insert(data.into_active_model()).exec(&txn).await?;
    Authz::replace_service_grants(
      &Self::holder(result.last_insert_id),
      &grants,
      &self.subject(),
      &txn,
    )
    .await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(&self, id: i32, data: UpdateRole) -> Result<entity::roles::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    if let Some(user_aps) = data.access_policies.clone() {
      let grants = to_service_grants(&user_aps)?;
      Authz::replace_service_grants(&Self::holder(id), &grants, &self.subject(), &txn).await?;
    }

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = Roles::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    // 역할이 사라지면 그 역할 앞으로 남은 부여도 전부 사라진다. 객체마다 `#grant` 를 다시
    // 묻지 않는 것은 의도된 것이다 — 그러면 최상위 관리자 말고는 역할을 지울 수 없다(§1.6).
    Authz::revoke_all_of(&Self::holder(id), &txn).await?;
    let result = Roles::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

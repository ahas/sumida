pub mod dto;

use chrono::{Duration, Utc};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  DbErr,
  DeleteResult,
  EntityTrait,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  Select,
  TransactionTrait,
};
use uuid::Uuid;

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace user_invitation service = user_invitation`).
const RESOURCE: &str = "user_invitation";

pub struct UserInvitationService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> UserInvitationService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 행 스코프는 `user_invitation#view` 술어 하나 — 초대받은 사람을 볼 수 있는 사람이 곧 그
  /// 초대를 볼 수 있는 사람이다(`invitee->view`).
  fn select(&self, query: Select<UserInvitations>) -> Result<Select<UserInvitations>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  pub async fn upsert<C>(user_id: i32, uuid: Uuid, db: &C) -> Result<(), DbErr>
  where
    C: ConnectionTrait,
  {
    let active_model = entity::user_invitations::ActiveModel {
      user_id: Set(user_id),
      uuid: Set(uuid),
      expires_at: Set(Utc::now() + Duration::days(30)),
      ..Default::default()
    };

    let exists = UserInvitations::find_by_id(uuid).count(db).await? > 0;

    if exists {
      UserInvitations::update(active_model).exec(db).await?;
    } else {
      UserInvitations::insert(active_model).exec(db).await?;
    }

    Ok(())
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `UserInvitationFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: UserInvitationFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<UserInvitationListItem>, u64), SumidaError> {
    use entity::user_invitations::Column;

    let db = self.state.db();
    let mut query =
      conditions.apply::<UserInvitationFilter, _>(filter.apply(self.select(UserInvitations::find())?))?;
    let count = query.clone().select_only().column(Column::Uuid).count(db).await?;

    query = pagination.order_by_schema::<UserInvitationFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<UserInvitationListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, uuid: Uuid) -> Result<Option<GetUserInvitation>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let item = self
      .select(UserInvitations::find_by_id(uuid))?
      .filter(entity::user_invitations::Column::ExpiresAt.gt(Utc::now()))
      .into_model::<GetUserInvitation>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn create(&self, user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Set(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at),
      ..Default::default()
    };

    UserInvitations::insert(active_model).exec(&txn).await?;
    let result = UserInvitations::find_by_id(uuid)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));

    txn.commit().await?;

    result
  }

  pub async fn update(&self, user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Unchanged(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at),
      ..Default::default()
    };

    UserInvitations::update(active_model).exec(&txn).await?;

    let result = UserInvitations::find_by_id(uuid)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));

    txn.commit().await?;

    result
  }

  pub async fn delete(&self, uuid: Uuid) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, uuid, "remove", &self.subject(), &txn).await?;

    let result = UserInvitations::delete_by_id(uuid).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

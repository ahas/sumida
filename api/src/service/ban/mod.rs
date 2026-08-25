pub mod dto;

use chrono::Utc;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::sea_query::{Alias, Asterisk, Expr, SimpleExpr};
use sea_orm::{
  ColumnTrait,
  Condition,
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  JoinType,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  QueryTrait,
  RelationTrait,
  Select,
  TransactionTrait,
  UpdateResult,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace ban service = ban`).
const RESOURCE: &str = "ban";

pub struct BanService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> BanService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  pub fn default_query_with(query: Select<Bans>) -> Select<Bans> {
    let user_alias = Alias::new("u");
    let created_by_alias = Alias::new("c");

    query
      .select_only()
      .expr(Expr::col((Bans, Asterisk)))
      .join_as(JoinType::LeftJoin, BansRelation::User.def(), user_alias.clone())
      .join_as(JoinType::LeftJoin, BansRelation::CreatedUser.def(), created_by_alias.clone())
  }

  /// 행 스코프는 `ban#view` 술어 하나로 붙는다 — 대상 사용자를 볼 수 있는 사람과 처분을 남긴
  /// 본인이 규칙 한 줄에 함께 들어 있다.
  fn select(&self, query: Select<Bans>) -> Result<Select<Bans>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// 유효한 처분의 조건 — 해제되지 않았고, 만료되지도 않았다. `is_banned` 플래그와 해제가 같은
  /// 조건을 읽으므로 `lifted_at` 이 찍힌 행은 곧바로 플래그에서 빠진다.
  fn active_at(now: DateTimeUtc) -> Condition {
    Condition::all() //
      .add(BansColumn::LiftedAt.is_null())
      .add(Condition::any().add(BansColumn::ExpiresAt.is_null()).add(BansColumn::ExpiresAt.gt(now)))
  }

  pub fn exists_expr() -> SimpleExpr {
    Expr::exists(
      Bans::find()
        .select_only()
        .expr(Expr::val(1))
        .filter(Expr::col(BansColumn::UserId).eq(Expr::col((Users, entity::users::Column::Id))))
        .filter(Self::active_at(Utc::now()))
        .limit(1)
        .into_query(),
    )
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `BanFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: BanFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BanListItem>, u64), SumidaError> {
    let txn = self.state.db().begin().await?;

    let mut query = conditions.apply::<BanFilter, _>(filter.apply(self.select(Bans::find())?))?;

    let count = query.clone().select_only().column(BansColumn::Id).count(&txn).await?;

    query = pagination.order_by_schema::<BanFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BanListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBan>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let item = self
      .select(Bans::find_by_id(id))?
      .select_only()
      .expr(Expr::col((Bans, Asterisk)))
      .join_as(JoinType::LeftJoin, BansRelation::User.def(), Alias::new("u"))
      .join_as(JoinType::LeftJoin, BansRelation::CreatedUser.def(), Alias::new("b"))
      .into_partial_model::<GetBan>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn create(&self, data: CreateBan) -> Result<InsertResult<BansActiveModel>, SumidaError> {
    let active_model = data.into_active_model();
    let result = Bans::insert(active_model).exec(self.state.db()).await?;

    Ok(result)
  }

  pub async fn create_by_user_id(
    &self,
    data: BanSettings,
    user_id: i32,
  ) -> Result<InsertResult<BansActiveModel>, SumidaError> {
    let mut active_model = data.into_active_model();
    active_model.user_id = Set(Some(user_id));
    let result = Bans::insert(active_model).exec(self.state.db()).await?;

    Ok(result)
  }

  /// 사용자에게 걸려 있는 유효한 처분을 모두 해제한다. 처분 이력 자체가 기록이므로 행은 지우지
  /// 않고 `lifted_at` 만 찍는다. 해제할 처분이 없으면 아무것도 바꾸지 않고 성공으로 답한다.
  pub async fn lift_by_user_id(&self, user_id: i32) -> Result<UpdateResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    let now = Utc::now();

    let ids: Vec<i32> = Bans::find()
      .select_only()
      .column(BansColumn::Id)
      .filter(BansColumn::UserId.eq(user_id))
      .filter(Self::active_at(now))
      .into_tuple()
      .all(&txn)
      .await?;

    if ids.is_empty() {
      txn.commit().await?;
      return Ok(UpdateResult { rows_affected: 0 });
    }

    // 해제도 쓰기라 이웃한 update/delete 와 같은 행 스코프를 묻는다 — 처분 한 건마다 `ban#edit`.
    for id in &ids {
      Authz::try_check(RESOURCE, *id, "edit", &self.subject(), &txn).await?;
    }

    let result = Bans::update_many()
      .col_expr(BansColumn::LiftedAt, Expr::value(now))
      .filter(BansColumn::Id.is_in(ids))
      .exec(&txn)
      .await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(&self, id: i32, data: UpdateBan) -> Result<BansModel, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "edit", &self.subject(), &txn).await?;

    let mut active_model = data.into_active_model();
    active_model.id = Set(id);

    let result = Bans::update(active_model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, id, "remove", &self.subject(), &txn).await?;

    let result = Bans::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

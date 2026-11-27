pub mod dto;
pub mod mail_form_context;

use chrono::Utc;
use entity::prelude::*;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::{Alias, Asterisk, Expr};
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
  RelationTrait,
  Select,
  TransactionTrait,
  Unchanged,
};
use sea_orm_crud::Optional;

use self::dto::*;
#[cfg(feature = "mail-form")]
use super::mail_form::MailFormSendOptions;
use super::prelude::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::authz::write::{SubjectKey, TupleFilter};
use crate::service::auth::session::AuthSessionService;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;
use crate::util::user::hash_password;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace user service = sumida_user`).
///
/// 정의 이름이 네임스페이스와 다른 유일한 자리다. `user` 는 주체 타입이고 `sumida_user` 는
/// "그 사람에게 무언가를 할 수 있는가"라는 객체 쪽이다.
const RESOURCE: &str = "sumida_user";

pub struct UserService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> UserService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  pub(crate) fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 호출자가 플랫폼 관리자인가. 세션의 `is_admin` 이 아니라 규칙(`platform:root#superuser`)에
  /// 묻는다 — 보안 API 키는 소유자의 관리자 플래그를 물려받지 않아야 하는데, 세션 필드는 키
  /// 소유자의 것으로 채워져 있다.
  async fn is_superuser<C: sea_orm::ConnectionTrait>(&self, db: &C) -> Result<bool, SumidaError> {
    Ok(Authz::is_superuser(&self.subject(), db).await?)
  }

  /// Refuses `user`, an administrator, to a caller who is not a platform administrator.
  async fn ensure_reachable<C: sea_orm::ConnectionTrait>(
    &self,
    user: &entity::users::Model,
    db: &C,
  ) -> Result<(), SumidaError> {
    ensure_reachable(&self.subject(), user, db).await
  }

  pub(crate) fn default_query_with(query: Select<Users>) -> Select<Users> {
    let org_alias = Alias::new("org");
    let role_alias = Alias::new("role");

    query
      .select_only()
      .expr(Expr::col((entity::users::Entity, Asterisk)))
      .expr_as(BanService::<S>::exists_expr(), "is_banned")
      .expr_as(Expr::col((org_alias.clone(), "id")), "org_id")
      .expr_as(Expr::col((org_alias.clone(), "name")), "org_name")
      .expr_as(Expr::col((role_alias.clone(), "id")), "role_id")
      .expr_as(Expr::col((role_alias.clone(), "name")), "role_name")
      .join_as(JoinType::LeftJoin, entity::users::Relation::Organization.def(), org_alias.clone())
      .join_as(JoinType::LeftJoin, entity::users::Relation::Role.def(), role_alias.clone())
  }

  /// 행 스코프는 `sumida_user#view` 술어 하나 — 상위 조직 관리자의 도달(`org->manage_user`)이
  /// 목록과 단건 판정에서 같은 규칙으로 나온다. 둘을 따로 구현하면 하나는 통과하고 다른 하나는
  /// 막는 어긋남이 구조적으로 가능해진다.
  pub(crate) fn select(&self, query: Select<Users>) -> Result<Select<Users>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `UserFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: UserFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<UserListItem>, u64), SumidaError> {
    use entity::users::Column;

    let db = self.state.db();
    let mut query = conditions.apply::<UserFilter, _>(filter.apply(self.select(Users::find())?))?;
    let count = query.clone().select_only().column(Column::Username).count(db).await?;

    query = pagination.order_by_schema::<UserFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<UserListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, user_id: i32) -> Result<Option<GetUser>, SumidaError> {
    Ok(
      self
        .select(Users::find_by_id(user_id))?
        .into_model::<GetUser>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateUser,
  ) -> Result<InsertResult<entity::users::ActiveModel>, SumidaError> {
    let db = self.state.db();
    let password = data.password.clone();
    let role_id = data.role_id;

    let mut active_model = data.into_active_model();
    active_model.password =
      Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    // A non-admin cannot control privilege-escalation fields (same policy as update()).
    //   - is_admin: always false → blocks creating a backdoor admin via {isAdmin:true}
    //   - organization_id: the operator's own organization → blocks planting users into other organizations
    //   - role_id: a role is global, and its grants reach everyone in it, so the caller may hand out
    //     only the role it holds itself or one it may grant (`role:<id>#grant`)
    //   - id: chosen by the server. A deleted account's id may still be named by grants somebody
    //     forgot, and an account opened under that id would hold them.
    if !self.is_superuser(db).await? {
      if matches!(active_model.id, Set(_)) {
        return Err(SumidaError::NoAccessPermission);
      }
      if role_id != self.session.profile.role.id || self.session.profile.api_key.is_some() {
        Authz::try_check("role", role_id, "grant", &self.subject(), db).await?;
      }

      active_model.is_admin = Set(false);
      active_model.organization_id = Set(self.session.profile.organization.id);
    }

    Ok(Users::insert(active_model).exec(db).await?)
  }

  pub async fn update(&self, user_id: i32, data: UpdateUser) -> Result<entity::users::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Three-way authorization branch:
    //   (1) is_admin = true                          → full update including security fields
    //   (2) `sumida_user:<id>#edit`                  → limited to users the caller manages, which
    //                                                  the rule resolves through `org->manage_user`
    //                                                  (조상 조직의 부여가 하위 조직 사용자에게
    //                                                  닿는다). Security fields
    //                                                  (is_admin/role_id/organization_id/is_enabled) locked
    //   (3) otherwise                                 → blocked (NoAccessPermission)
    //
    // Changing one's own profile goes through the /me route (UpdateMe DTO). This route is the
    // operator-privilege channel for updating other people.
    let allow_security_fields = if self.is_superuser(&txn).await? {
      true
    } else {
      Authz::try_check(RESOURCE, user_id, "edit", &self.subject(), &txn).await?;
      false
    };
    let Some(before) = Users::find_by_id(user_id).one(&txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    self.ensure_reachable(&before, &txn).await?;

    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(user_id);

    if let Some(password) = password {
      model.password = Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    if !allow_security_fields {
      // Strip fields that could cause privilege escalation. To promote to admin or change a role,
      // an operator must act under an is_admin=true account.
      model.is_admin = NotSet;
      model.role_id = NotSet;
      model.organization_id = NotSet;
      model.is_enabled = NotSet;
    }

    // A new password, or a change to what the account may do — its administrator flag, role,
    // organization or whether it is enabled — voids every session and token established before it.
    // A session carries the role, organization and flag it was established with, so one left
    // standing would go on acting with what was just taken away.
    let disowned = matches!(model.password, Set(_))
      || matches!(model.is_admin, Set(v) if v != before.is_admin)
      || matches!(model.is_enabled, Set(v) if v != before.is_enabled)
      || matches!(model.role_id, Set(v) if v != before.role_id)
      || matches!(model.organization_id, Set(v) if v != before.organization_id);
    if disowned {
      model.credentials_reset_at = Set(Some(Utc::now()));
    }

    let result = Users::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, user_id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    Authz::try_check(RESOURCE, user_id, "remove", &self.subject(), &txn).await?;
    if let Some(user) = Users::find_by_id(user_id).one(&txn).await? {
      self.ensure_reachable(&user, &txn).await?;
    }

    // 계정이 사라지면 그 계정 앞으로 남은 부여도, 그 계정의 키 앞으로 남은 부여도, 그 계정을
    // 대상으로 한 부여도 함께 사라진다. `relation_tuples` 에는 사용자로 가는 외래키가 없어서
    // 남겨 두면 같은 id 로 다시 열린 계정이 그것을 전부 물려받는다.
    let keys: Vec<String> = ApiKeys::find()
      .select_only()
      .column(entity::api_keys::Column::Key)
      .filter(entity::api_keys::Column::UserId.eq(user_id))
      .into_tuple()
      .all(&txn)
      .await?;
    for key in &keys {
      Authz::revoke_all_of(&SubjectKey::apikey(key), &txn).await?;
    }
    Authz::revoke_all_of(&SubjectKey::user(user_id), &txn).await?;
    Authz::revoke_unchecked(TupleFilter::object(RESOURCE, &user_id.to_string()), &txn).await?;

    let result = Users::delete_by_id(user_id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn invite(&self, user_id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;

    // 초대는 그 사람에게 하는 일이라 `sumida_user:<id>#edit` 를 묻는다. 그리고 초대를 받아들이면
    // 계정이 켜지므로, 꺼진 계정을 초대하는 것은 `is_enabled` 를 바꾸는 일 — update() 에서 플랫폼
    // 관리자만 바꿀 수 있는 보안 필드다.
    Authz::try_check(RESOURCE, user_id, "edit", &self.subject(), &txn).await?;

    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    // Accepting an invitation sets the account's password, so inviting an administrator is taking
    // that account over unless the inviter is one too.
    self.ensure_reachable(&user, &txn).await?;
    if !user.is_enabled && !self.is_superuser(&txn).await? {
      return Err(SumidaError::NoAccessPermission);
    }
    // 이메일 없음 검증은 mail-form 여부와 무관하게 유지한다(초대는 이메일 전제).
    // `receiver` 는 mail-form 발송에서만 읽힌다.
    #[cfg_attr(not(feature = "mail-form"), allow(unused_variables))]
    let Some(receiver) = user.email else {
      return Err(SumidaError::NoUserEmail(user_id));
    };

    let uuid = uuid::Uuid::new_v4();

    UserInvitationService::<S>::upsert(user_id, uuid, &txn).await?;

    // 초대 이메일 발송은 mail-form 기능에 의존한다. 꺼지면 초대 레코드(UUID)는 그대로 만들되
    // 메일만 생략한다(소비자가 초대 링크를 다른 채널로 전달 가능).
    #[cfg(feature = "mail-form")]
    {
      let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
        username: opts.username.clone(),
        password: opts.password.clone(),
        auth:     opts.auth.clone(),
        server:   opts.server.clone(),
        port:     opts.resolved_port(),
        tls:      opts.tls,
      });

      MailFormService::<S>::send(
        "invitation",
        receiver,
        mail_form_context::InvitationContext {
          user_id,
          uuid: uuid.to_string(),
          full_name: user.full_name.unwrap_or_else(String::new),
        },
        opts,
        &txn,
      )
      .await?;
    }

    txn.commit().await?;

    Ok(())
  }

  pub async fn activate(&mut self, activation: UserActivation) -> Result<(), SumidaError> {
    let user_id = activation.user_id;
    let password = &activation.password;
    let uuid = &activation.uuid;
    let db = self.state.db();
    let txn = db.begin().await?;

    let Some(user) = Users::find_by_id(user_id).one(db).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    // The invitation uuid must have been issued for that user_id.
    // Verifying only the uuid without matching user_id would let one reset the password of any account (including admins).
    let Some(_) = UserInvitations::find_by_id(*uuid)
      .filter(entity::user_invitations::Column::UserId.eq(user_id))
      .filter(entity::user_invitations::Column::ExpiresAt.gt(Utc::now()))
      .one(db)
      .await?
    else {
      return Err(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));
    };

    txn.commit().await?;

    let txn = db.begin().await?;

    let username = user.username.clone();
    let mut active_model = user.into_active_model();
    active_model.is_enabled = Set(true);
    active_model.password =
      Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);
    // The password is set anew, so whatever was established under the old one is void.
    active_model.credentials_reset_at = Set(Some(Utc::now()));

    Users::update(active_model).exec(&txn).await?;
    UserInvitations::delete_by_id(*uuid).exec(&txn).await?;

    txn.commit().await?;

    let mut auth_session_service = AuthSessionService::new(self.state.clone(), self.session.clone());
    auth_session_service
      .login(LoginSession {
        username,
        password: password.to_string(),
        ..Default::default()
      })
      .await?;
    self.session = auth_session_service.session;

    Ok(())
  }

  pub async fn change_password(&mut self, data: ChangePassword) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(user) = Users::find()
      .filter(entity::users::Column::Username.eq(&data.username))
      .one(&txn)
      .await?
    else {
      txn.rollback().await?;
      return Err(SumidaError::InvalidAccount);
    };

    let user_verification_service =
      UserVerificationService::<S>::new(self.state.clone(), self.session.clone());
    let is_verified =
      user_verification_service.verify("password-change", user.id, &data.code, &txn).await?;
    self.session = user_verification_service.session;

    if !is_verified {
      txn.rollback().await?;
      return Err(SumidaError::VerificationFailed);
    }

    let password = hash_password(&data.password).map_err(|e| SumidaError::Unsupported(e.to_string()))?;

    // The row is addressed by its key: a single-row update filters on the primary key and on
    // nothing else, so the key is what has to be in the model.
    //
    // Recording the moment alongside the new password is what makes the change reach the
    // credentials already handed out: a token or a session established up to it is refused from
    // here on, so whoever the password was changed because of is put out with it.
    let active_model = entity::users::ActiveModel {
      id: Unchanged(user.id),
      password: Set(password),
      credentials_reset_at: Set(Some(Utc::now())),
      ..Default::default()
    };

    Users::update(active_model).exec(&txn).await?;

    // Whoever proved the account theirs with the code is let back in: the bans in force on the account,
    // and on the address the change came from, are lifted with it.
    Bans::update_many()
      .col_expr(entity::bans::Column::LiftedAt, Expr::value(Utc::now()))
      .filter(entity::bans::Column::LiftedAt.is_null())
      .filter(
        Condition::any().add(entity::bans::Column::UserId.eq(user.id)).add(
          Condition::all()
            .add(entity::bans::Column::UserId.is_null())
            .add(entity::bans::Column::IpAddress.eq(self.session.env.ip_address.clone())),
        ),
      )
      .exec(&txn)
      .await?;

    txn.commit().await?;

    Ok(())
  }

  pub async fn impersonate(&mut self, user_id: i32) -> Result<(), SumidaError> {
    // Only platform administrators may impersonate another user, and the answer comes from the rule:
    // a secured key carries its owner's `is_admin` in the session but not the owner's authority.
    // It also takes a cookie session to hold the result, so a request that authenticated with a
    // header credential — which never mints one — has nothing to impersonate into.
    let db = self.state.db();
    if self.session.is_request_local().await || !self.is_superuser(db).await? {
      return Err(SumidaError::OnlyAdminAllowed);
    }

    let settings = &settings::get().session.cookie;
    let Some(user) = Users::find_by_id(user_id).one(db).await? else {
      return Err(SumidaError::InvalidAccount);
    };

    self
      .session
      .authorize(user_id, &user.username, Some(settings.expiry.default_expiry()?), db)
      .await?;

    Ok(())
  }
}

/// Refuses `user` to `subject` when `user` is an administrator and `subject` is not a platform
/// administrator.
///
/// Whoever may edit an account can set its password, its email and its enabled state, ban it or
/// remove it — each of which takes the account over or shuts it out. A grant over an organization's
/// accounts, or the user namespace's operator tier, is meant for the accounts it reaches, never for an
/// administrator who happens to sit among them.
pub(crate) async fn ensure_reachable<C: sea_orm::ConnectionTrait>(
  subject: &SubjectSet,
  user: &entity::users::Model,
  db: &C,
) -> Result<(), SumidaError> {
  if user.is_admin && !Authz::is_superuser(subject, db).await? {
    return Err(SumidaError::NoAccessPermission);
  }
  Ok(())
}

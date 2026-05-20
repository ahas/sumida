pub mod dto;
pub mod mail_form_context;

use chrono::Utc;
use entity::prelude::*;
use infra::settings;
use sea_orm::ActiveValue::*;
use sea_orm::sea_query::{Alias, Asterisk, Expr};
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, JoinType,
  PaginatorTrait, QueryFilter, QuerySelect, RelationTrait, Select, TransactionTrait, Unchanged,
};
use sea_orm_crud::Optional;

use self::dto::*;
use super::mail_form::MailFormSendOptions;
use super::prelude::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::service::auth::session::AuthSessionService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::org::organization_ancestors_subquery;
use crate::util::pagination::Pagination;
use crate::util::user::hash_password;

pub struct UserService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> UserService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: UserFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserListItem>, u64), SumidaError> {
    use entity::users::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(Users::find()));
    let count = query.clone().select_only().column(Column::Username).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "username" => Column::Username,
      "name" => Column::FullName,
      "email" => Column::Email,
      "createdAt" => Column::CreatedAt,
      "updatedAt" => Column::UpdatedAt,
      _ => Column::Username,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, user_id: i32) -> Result<Option<GetUser>, SumidaError> {
    Ok(
      self
        .select(Users::find_by_id(user_id))
        .into_model::<GetUser>()
        .one(self.state.db())
        .await?,
    )
  }

  pub async fn create(
    &self,
    data: CreateUser,
  ) -> Result<InsertResult<entity::users::ActiveModel>, SumidaError> {
    let password = data.password.clone();

    let mut active_model = data.into_active_model();
    active_model.password =
      Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    Ok(Users::insert(active_model).exec(self.state.db()).await?)
  }

  pub async fn update(&self, user_id: i32, data: UpdateUser) -> Result<entity::users::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    // 권한 3단 분기:
    //   (1) is_admin = true                          → 보안 필드 포함 전체 update
    //   (2) namespace="org:user", action=update      → 자기 조직 + 후손 조직 user에 한정.
    //                                                  보안 필드(is_admin/role_id/organization_id/is_enabled) 잠금
    //   (3) 그 외                                     → 차단 (NoAccessPermission)
    //
    // 자기 자신 프로필 변경은 /me 경로(UpdateMe DTO)를 사용해야 한다. 이 경로는
    // 타인을 update할 때 쓰는 운영자 권한 채널.
    let allow_security_fields = if self.session.profile.is_admin {
      true
    } else {
      let start_org_col = Expr::col((entity::users::Entity, entity::users::Column::OrganizationId))
        .into_simple_expr();
      ap::ns("org:user")
        .who(&self.session)
        .join(Users, entity::users::Column::Id)
        .object_in_subquery(organization_ancestors_subquery(start_org_col))
        .action("update")
        .try_access(Users::find_by_id(user_id), &txn)
        .await?;
      false
    };

    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(user_id);

    if let Some(password) = password {
      model.password = Set(hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    if !allow_security_fields {
      // 권한 상승을 일으킬 수 있는 필드는 잘라낸다. 운영자가 admin 승격/role 변경을 하려면
      // is_admin=true 계정으로 동작해야 한다.
      model.is_admin = NotSet;
      model.role_id = NotSet;
      model.organization_id = NotSet;
      model.is_enabled = NotSet;
    }

    let result = Users::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, user_id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", Users::find_by_id(user_id), &txn).await?;

    let result = Users::delete_by_id(user_id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn invite(&self, user_id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    let Some(receiver) = user.email else {
      return Err(SumidaError::NoUserEmail(user_id));
    };

    let uuid = uuid::Uuid::new_v4();

    UserInvitationService::<S>::upsert(user_id, uuid, &txn).await?;

    let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
      username: opts.username.clone(),
      password: opts.password.clone(),
      auth: opts.auth.clone(),
      server: opts.server.clone(),
      port: opts.resolved_port(),
      tls: opts.tls,
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
    let Some(_) = UserInvitations::find_by_id(*uuid)
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

    let active_model = entity::users::ActiveModel {
      username: Unchanged(data.username),
      password: Set(password),
      ..Default::default()
    };

    Users::update(active_model).exec(&txn).await?;

    txn.commit().await?;

    Ok(())
  }

  pub async fn impersonate(&mut self, user_id: i32) -> Result<(), SumidaError> {
    let db = self.state.db();
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

impl<S: SumidaState> ApiService<ApCondition, Users> for UserService<S> {
  fn default_query_with(query: Select<Users>) -> Select<Users> {
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

  fn ap(&self) -> ApCondition {
    use entity::users::Column;

    ap::ns("user").who(&self.session).join(Users, Column::Id)
  }
}

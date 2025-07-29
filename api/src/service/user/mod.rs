pub mod dto;
pub mod mail_form_context;

use argon2::Argon2;
use argon2::password_hash::PasswordHasher;
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use chrono::Utc;
use infra::session::UserContext;
use infra::{db, settings};
use entity::prelude::*;
use error::SumidaError;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{ActiveValue::*, JoinType, Select};
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QueryFilter,
  QuerySelect, RelationTrait, TransactionTrait, Unchanged, sea_query::Expr,
};
use sea_orm_crud::Optional;

use crate::service::ApiService;
use crate::service::auth::session::AuthSessionService;
use crate::util::ap::{Ap, ApGuard};
use crate::util::crud::Pagination;

use self::dto::*;

use super::mail_form::MailFormSendOptions;
use super::prelude::*;

pub struct UserService {
  user_ctx: UserContext,
}

impl UserService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

  pub fn hash_password<S>(password: S) -> argon2::password_hash::Result<String>
  where
    S: AsRef<str>,
  {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    Ok(argon2.hash_password(password.as_ref().as_bytes(), &salt)?.to_string())
  }

  pub async fn list(
    &self,
    filter: UserFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserListItem>, u64), SumidaError> {
    use entity::users::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(Users::find()));
    let count = query.clone().select_only().column(Column::Username).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "name" => Column::FullName,
      "email" => Column::Email,
      "createdAt" => Column::CreatedAt,
      "updatedAt" => Column::UpdatedAt,
      _ => Column::Username,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, user_id: i32) -> Result<Option<GetUser>, SumidaError> {
    Ok(self.select(Users::find_by_id(user_id)).into_model::<GetUser>().one(db::get_connection()).await?)
  }

  pub async fn create(
    &self,
    data: CreateUser,
  ) -> Result<InsertResult<entity::users::ActiveModel>, SumidaError> {
    let password = data.password.clone();

    let mut active_model = data.into_active_model();
    active_model.password =
      Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    Ok(Users::insert(active_model).exec(db::get_connection()).await?)
  }

  pub async fn update(&self, user_id: i32, data: UpdateUser) -> Result<entity::users::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("update").check(Users::find_by_id(user_id), &txn).await?;

    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.id = Unchanged(user_id);

    if let Some(password) = password {
      model.password =
        Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    let result = Users::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, user_id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("delete").check(Users::find_by_id(user_id), &txn).await?;

    let result = Users::delete_by_id(user_id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn invite(&self, user_id: i32) -> Result<(), SumidaError> {
    let txn = db::get_connection().begin().await?;

    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    let Some(receiver) = user.email else {
      return Err(SumidaError::NoUserEmail(user_id));
    };

    let uuid = uuid::Uuid::new_v4();

    UserInvitationService::upsert(user_id, uuid, &txn).await?;

    let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
      username: opts.username.clone(),
      password: opts.password.clone(),
      auth: opts.auth.clone(),
      server: opts.server.clone(),
    });

    MailFormService::send(
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
    let db = db::get_connection();
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
      Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    Users::update(active_model).exec(&txn).await?;
    UserInvitations::delete_by_id(*uuid).exec(&txn).await?;

    txn.commit().await?;

    let mut auth_session_service = AuthSessionService::new(self.user_ctx.clone());
    auth_session_service
      .login(LoginSession {
        username,
        password: password.to_string(),
        ..Default::default()
      })
      .await?;
    self.user_ctx = auth_session_service.user_ctx;

    Ok(())
  }

  pub async fn change_password(&mut self, data: ChangePassword) -> Result<(), SumidaError> {
    let txn = db::get_connection().begin().await?;

    let Some(user) =
      Users::find().filter(entity::users::Column::Username.eq(&data.username)).one(&txn).await?
    else {
      txn.rollback().await?;
      return Err(SumidaError::InvalidAccount);
    };

    let user_verification_service = UserVerificationService::new(self.user_ctx.clone());
    let is_verified =
      user_verification_service.verify("password-change", user.id, &data.code, &txn).await?;
    self.user_ctx = user_verification_service.user_ctx;

    if !is_verified {
      txn.rollback().await?;
      return Err(SumidaError::VerificationFailed);
    }

    let password =
      Self::hash_password(&data.password).map_err(|e| SumidaError::Unsupported(e.to_string()))?;

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
    let db = db::get_connection();
    let settings = settings::get();
    let Some(user) = Users::find_by_id(user_id).one(db).await? else {
      return Err(SumidaError::InvalidAccount);
    };

    self
      .user_ctx
      .authorize(user_id, &user.username, Some(settings.session.expiry.default_expiry()?))
      .await?;

    Ok(())
  }

  pub async fn get_vapid(&self) -> Result<GetVapid, SumidaError> {
    let settings = &infra::settings::get().web_push;
    let audience = &settings.audience;
    let exp = &settings.expiration;
    let sub = &settings.subject;

    Ok(GetVapid::new(audience, exp, sub).await)
  }
}

impl ApiService<Ap, Users> for UserService {
  fn select(&self, query: Select<Users>) -> Select<Users> {
    self.ap().action("read").apply({
      let org_alias = Alias::new("org");
      let role_alias = Alias::new("role");

      query
        .select_only()
        .expr(Expr::col((entity::users::Entity, Asterisk)))
        .expr_as(UserBlockService::exists_expr(), "is_blocked")
        .expr_as(Expr::col((org_alias.clone(), "id")), "org_id")
        .expr_as(Expr::col((org_alias.clone(), "name")), "org_name")
        .expr_as(Expr::col((role_alias.clone(), "id")), "role_id")
        .expr_as(Expr::col((role_alias.clone(), "name")), "role_name")
        .join_as(JoinType::LeftJoin, entity::users::Relation::Organization.def(), org_alias.clone())
        .join_as(JoinType::LeftJoin, entity::users::Relation::Role.def(), role_alias.clone())
    })
  }

  fn ap(&self) -> Ap {
    use entity::users::Column;

    Ap::new("user") //
      .owner(&self.user_ctx)
      .object(Expr::col((Users, Column::Id)).cast_as("varchar"))
  }
}

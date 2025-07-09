pub mod dto;
pub mod mail_form_context;

#[cfg(test)]
pub mod tests;

use argon2::password_hash::PasswordHasher;
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use chrono::Utc;
use core::session::UserContext;
use core::{db, settings};
use entity::prelude::*;
use entity::user_logs::{SignInCode, SignInMethod};
use error::SumidaError;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{ActiveValue::*, JoinType, Select};
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QueryFilter,
  QuerySelect, RelationTrait, TransactionTrait, Unchanged,
};
use sea_orm_crud::Optional;

use crate::util::crud::Pagination;

use self::dto::*;

use super::mail_form::MailFormSendOptions;
use super::prelude::*;

pub struct UserService;

impl UserService {
  pub fn select(query: Select<entity::users::Entity>) -> Select<entity::users::Entity> {
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
    filter: UserFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserListItem>, u64), SumidaError> {
    use entity::users::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(Self::select(Users::find()));
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

  pub async fn get(user_id: i32) -> Result<Option<GetUser>, SumidaError> {
    Ok(Self::select(Users::find_by_id(user_id)).into_model::<GetUser>().one(db::get_connection()).await?)
  }

  pub async fn create(data: CreateUser) -> Result<InsertResult<entity::users::ActiveModel>, SumidaError> {
    let password = data.password.clone();

    let mut active_model = data.into_active_model();
    active_model.password =
      Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    Ok(Users::insert(active_model).exec(db::get_connection()).await?)
  }

  pub async fn update(username: String, data: UpdateUser) -> Result<entity::users::Model, SumidaError> {
    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.username = Set(username);

    if let Some(password) = password {
      model.password =
        Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    Ok(Users::update(model).exec(db::get_connection()).await?)
  }

  pub async fn delete(user_id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(Users::delete_by_id(user_id).exec(db::get_connection()).await?)
  }

  pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let argon2 = Argon2::default();
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
      return false;
    };
    let result = argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok();

    result
  }

  pub async fn update_my_info(
    user_ctx: &mut UserContext,
    data: UpdateUser,
  ) -> Result<entity::users::Model, SumidaError> {
    let username = &user_ctx.profile.username;
    let Optional(password) = data.password.clone();

    let mut model: entity::users::ActiveModel = data.into_active_model();
    model.username = Set(username.clone());

    if let Some(ref password) = password {
      model.password =
        Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?)
    }

    model.role_id = NotSet;
    model.organization_id = NotSet;
    model.is_admin = NotSet;
    model.is_enabled = NotSet;

    user_ctx.identify().await?;

    Ok(Users::update(model).exec(db::get_connection()).await?)
  }

  pub async fn sign_in(user_ctx: &mut UserContext, data: SignInUser) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    // 1) Return error if the user does not exist
    let Some(user) = Users::find()
      .filter(entity::users::Column::Username.eq(&data.username))
      .filter(entity::users::Column::IsEnabled.eq(true))
      .one(&txn)
      .await?
    else {
      user_ctx.write_log(None, SignInMethod::Password, SignInCode::AccountNotFound).await?;
      return Err(SumidaError::InvalidAccount);
    };

    // 2) Check the user is blocked
    if user_ctx.check_blocked(Some(user.id), &txn).await? {
      user_ctx.write_log(Some(user.id), SignInMethod::Password, SignInCode::AccountBlocked).await?;
      return Err(SumidaError::AccountBlocked);
    }

    // 3) Return error if the user is inactive
    if !user.is_enabled {
      user_ctx.write_log(Some(user.id), SignInMethod::Password, SignInCode::AccountInactive).await?;
      return Err(SumidaError::InactiveAccount);
    }

    // 4) Check the user password
    let result = if Self::verify_password(&data.password, &user.password) {
      user_ctx.write_log(Some(user.id), SignInMethod::Password, SignInCode::Success).await?;

      let expiry = if let Some(remember_me) = data.remember_me {
        let settings = settings::get();

        Some(if remember_me {
          settings.session.expiry.remember_me_expiry()?
        } else {
          settings.session.expiry.default_expiry()?
        })
      } else {
        None
      };

      user_ctx.authorize(user.id, &data.username, expiry).await?;

      Ok(())
    } else {
      user_ctx.write_log(Some(user.id), SignInMethod::Password, SignInCode::PasswordMismatch).await?;
      let blocked = user_ctx.block_if_exceeded_limit(Some(user.id), &txn).await?;

      Err(if blocked { SumidaError::AccountBlocked } else { SumidaError::InvalidAccount })
    };

    txn.commit().await?;

    result
  }

  pub async fn invite(_user_ctx: &UserContext, user_id: i32) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

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

  pub async fn activate(user_ctx: &mut UserContext, activation: UserActivation) -> Result<(), SumidaError> {
    let user_id = activation.user_id;
    let password = &activation.password;
    let uuid = &activation.uuid;
    let db_conn = db::get_connection();

    let Some(user) = Users::find_by_id(user_id).one(db_conn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    let Some(_) = UserInvitations::find_by_id(*uuid)
      .filter(entity::user_invitations::Column::ExpiresAt.gt(Utc::now()))
      .one(db_conn)
      .await?
    else {
      return Err(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));
    };

    let txn = db_conn.begin().await?;

    let username = user.username.clone();
    let mut active_model = user.into_active_model();
    active_model.is_enabled = Set(true);
    active_model.password =
      Set(Self::hash_password(password).map_err(|e| SumidaError::Unsupported(e.to_string()))?);

    Users::update(active_model).exec(&txn).await?;
    UserInvitations::delete_by_id(*uuid).exec(&txn).await?;

    txn.commit().await?;

    Self::sign_in(
      user_ctx,
      SignInUser {
        username,
        password: password.to_string(),
        ..Default::default()
      },
    )
    .await?;

    Ok(())
  }

  pub async fn change_password(data: ChangePassword) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;
    let Some(user) =
      Users::find().filter(entity::users::Column::Username.eq(&data.username)).one(&txn).await?
    else {
      txn.rollback().await?;
      return Err(SumidaError::InvalidAccount);
    };

    let is_verified = UserVerificationService::verify("password-change", user.id, &data.code, &txn).await?;

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

  pub async fn impersonate(user_ctx: &mut UserContext, user_id: i32) -> Result<(), SumidaError> {
    let db_conn = db::get_connection();
    let settings = settings::get();
    let Some(user) = Users::find_by_id(user_id).one(db_conn).await? else {
      return Err(SumidaError::InvalidAccount);
    };

    user_ctx.authorize(user_id, &user.username, Some(settings.session.expiry.default_expiry()?)).await?;

    Ok(())
  }

  pub async fn get_vapid(_user_ctx: &UserContext) -> Result<GetVapid, SumidaError> {
    let settings = &core::settings::get().web_push;
    let audience = &settings.audience;
    let exp = &settings.expiration;
    let sub = &settings.subject;

    Ok(GetVapid::new(audience, exp, sub).await)
  }
}

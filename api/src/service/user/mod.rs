pub mod dto;
pub mod mail_form_context;
pub mod session;

#[cfg(test)]
pub mod tests;

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use chrono::Utc;
use sea_orm::ActiveValue::*;
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, TransactionTrait, Unchanged,
};
use sea_orm_crud::Optional;
use session::UserContext;

use crate::core::error::SumidaError;
use crate::core::{db, settings};
use crate::entity::prelude::*;
use crate::util::crud::{search, Pagination};

use self::dto::*;

pub async fn list(
  filter: UserFilter,
  pagination: Pagination,
) -> Result<(Vec<UserListItem>, u64), SumidaError> {
  use crate::entity::users::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = Users::find()
    .apply_if(filter.username, |q, v| search(q, Column::Username, v))
    .apply_if(filter.name, |q, v| search(q, Column::FullName, v))
    .apply_if(filter.organization, |q, v| q.filter(Column::Organization.eq(v)))
    .apply_if(filter.role, |q, v| q.filter(Column::Role.eq(v)));
  let count = query.clone().select_only().column(Column::Username).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "name" => Column::FullName,
    "email" => Column::Email,
    "createdAt" => Column::CreatedAt,
    "updatedAt" => Column::UpdatedAt,
    _ => Column::Username,
  });
  let query = pagination.paginate(query);
  let items = query.into_model::<UserListItem>().all(&txn).await?;

  txn.commit().await?;

  Ok((items, count))
}

pub async fn get<S: AsRef<str>>(username: S) -> Result<Option<GetUser>, SumidaError> {
  Ok(Users::find_by_id(username.as_ref()).into_model::<GetUser>().one(db::get_connection()).await?)
}

pub async fn create(
  data: CreateUser,
) -> Result<InsertResult<crate::entity::users::ActiveModel>, SumidaError> {
  let password = data.password.clone();

  let mut active_model = data.into_active_model();
  active_model.password = Set(Users::hash_password(password).unwrap());

  Ok(Users::insert(active_model).exec(db::get_connection()).await?)
}

pub async fn update(
  username: String,
  data: UpdateUser,
) -> Result<crate::entity::users::Model, SumidaError> {
  let Optional(password) = data.password.clone();

  let mut model: crate::entity::users::ActiveModel = data.into_active_model();
  model.username = Set(username);

  if let Some(password) = password {
    model.password = Set(Users::hash_password(password).unwrap())
  }

  Ok(Users::update(model).exec(db::get_connection()).await?)
}

pub async fn delete(username: String) -> Result<DeleteResult, SumidaError> {
  Ok(Users::delete_by_id(username).exec(db::get_connection()).await?)
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
  user_ctx: &UserContext,
  data: UpdateUser,
) -> Result<crate::entity::users::Model, SumidaError> {
  let username = &user_ctx.profile.username;
  let Optional(password) = data.password.clone();

  let mut model: crate::entity::users::ActiveModel = data.into_active_model();
  model.username = Set(username.clone());

  if let Some(ref password) = password {
    model.password = Set(Users::hash_password(password).unwrap())
  }

  model.role = NotSet;
  model.organization = NotSet;
  model.is_admin = NotSet;
  model.is_enabled = NotSet;

  Ok(Users::update(model).exec(db::get_connection()).await?)
}

pub async fn sign_in(user_ctx: &mut UserContext, data: SignInUser) -> Result<(), SumidaError> {
  let db_conn = db::get_connection();
  let user = Users::find_by_id(&data.username)
    .filter(crate::entity::users::Column::IsEnabled.eq(true))
    .one(db_conn)
    .await?
    .ok_or_else(|| SumidaError::InvalidAccount)?;
  let is_verified = verify_password(&data.password, &user.password);

  if !is_verified {
    // UserSignInLogs::write(data.username, method, code, user_env, db)
    return Err(SumidaError::InvalidAccount);
  }

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

  user_ctx.sign_in(&data.username, expiry).await?;

  Ok(())
}

pub async fn invite(_user_ctx: &UserContext, username: String) -> Result<(), SumidaError> {
  let db_conn = db::get_connection();

  let Some(user) = Users::find_by_id(&username).one(db_conn).await? else {
    return Err(SumidaError::InvalidAccount);
  };
  let Some(receiver) = user.email else {
    return Err(SumidaError::NoUserEmail(username));
  };

  let uuid = uuid::Uuid::new_v4();

  UserInvitations::create(&username, uuid).await?;

  MailForms::send(
    "invitation",
    receiver,
    mail_form_context::InvitationContext {
      username,
      uuid: uuid.to_string(),
      full_name: user.full_name.unwrap_or_else(String::new),
    },
  )
  .await?;

  Ok(())
}

pub async fn activate(user_ctx: &mut UserContext, activation: UserActivation) -> Result<(), SumidaError> {
  let username = &activation.username;
  let password = &activation.password;
  let uuid = &activation.uuid;
  let db_conn = db::get_connection();

  let Some(user) = Users::find_by_id(username).one(db_conn).await? else {
    return Err(SumidaError::InvalidAccount);
  };
  let Some(_) = UserInvitations::find_by_id(*uuid)
    .filter(crate::entity::user_invitations::Column::ExpiredAt.gt(Utc::now()))
    .one(db_conn)
    .await?
  else {
    return Err(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));
  };

  let txn = db_conn.begin().await?;

  let mut active_model = user.into_active_model();
  active_model.is_enabled = Set(true);
  active_model.password = Set(Users::hash_password(password).unwrap());

  Users::update(active_model).exec(&txn).await?;
  UserInvitations::delete_by_id(*uuid).exec(&txn).await?;

  txn.commit().await?;

  sign_in(
    user_ctx,
    SignInUser {
      username: username.to_string(),
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

  let is_verified = UserVerifications::verify("password-change", &data.username, &data.code, &txn).await?;

  if !is_verified {
    txn.rollback().await?;
    return Err(SumidaError::VerificationFailed);
  }

  let password = Users::hash_password(&data.password).unwrap();

  let active_model = crate::entity::users::ActiveModel {
    username: Unchanged(data.username),
    password: Set(password),
    ..Default::default()
  };

  Users::update(active_model).exec(&txn).await?;

  txn.commit().await?;

  Ok(())
}

pub async fn impersonate<S>(user_ctx: &mut UserContext, username: S) -> Result<(), SumidaError>
where
  S: AsRef<str>,
{
  let db_conn = db::get_connection();
  let settings = settings::get();
  let username = username.as_ref();
  let user = Users::find_by_id(username).one(db_conn).await?;

  if let Some(_) = user {
    user_ctx.sign_in(username, Some(settings.session.expiry.default_expiry()?)).await?;

    return Ok(());
  }

  Err(SumidaError::InvalidAccount)
}

pub async fn get_vapid(_user_ctx: &UserContext) -> Result<GetVapid, SumidaError> {
  let settings = &crate::core::settings::get().web_push;
  let audience = &settings.audience;
  let exp = &settings.expiration;
  let sub = &settings.subject;

  Ok(GetVapid::new(audience, exp, sub).await)
}

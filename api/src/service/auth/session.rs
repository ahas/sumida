use argon2::{Argon2, PasswordHash, PasswordVerifier};
use entity::{
  prelude::*,
  user_logs::{LoginCode, LoginMethod},
};
use error::SumidaError;
use infra::settings;
use infra::{db, session::UserSession};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use crate::service::auth::dto::LoginSession;

pub struct AuthSessionService {
  pub session: UserSession,
}

impl AuthSessionService {
  pub fn new(session: UserSession) -> Self {
    Self { session }
  }

  pub fn verify_password(&self, password: &str, password_hash: &str) -> bool {
    let argon2 = Argon2::default();
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
      return false;
    };

    argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok()
  }

  pub async fn login(&mut self, data: LoginSession) -> Result<(), SumidaError> {
    let db = db::get_connection();
    let txn = db.begin().await?;

    // 1) Return error if the user does not exist
    let Some(user) = Users::find()
      .filter(entity::users::Column::Username.eq(&data.username))
      .filter(entity::users::Column::IsEnabled.eq(true))
      .one(&txn)
      .await?
    else {
      self.session.write_log(None, LoginMethod::Password, LoginCode::AccountNotFound).await?;
      return Err(SumidaError::InvalidAccount);
    };

    // 2) Check the user is blocked
    if self.session.check_blocked(Some(user.id), &txn).await? {
      self.session.write_log(Some(user.id), LoginMethod::Password, LoginCode::AccountBlocked).await?;
      return Err(SumidaError::AccountBlocked);
    }

    // 3) Return error if the user is inactive
    if !user.is_enabled {
      self.session.write_log(Some(user.id), LoginMethod::Password, LoginCode::AccountInactive).await?;
      return Err(SumidaError::InactiveAccount);
    }

    // 4) Check the user password
    let result = if self.verify_password(&data.password, &user.password) {
      self.session.write_log(Some(user.id), LoginMethod::Password, LoginCode::Success).await?;

      let expiry = if let Some(remember_me) = data.remember_me {
        let settings = &settings::get().session.cookie;

        Some(if remember_me {
          settings.expiry.remember_me_expiry()?
        } else {
          settings.expiry.default_expiry()?
        })
      } else {
        None
      };

      self.session.authorize(user.id, &data.username, expiry).await?;

      Ok(())
    } else {
      self.session.write_log(Some(user.id), LoginMethod::Password, LoginCode::PasswordMismatch).await?;
      let blocked = self.session.block_if_exceeded_limit(Some(user.id), &txn).await?;

      Err(if blocked { SumidaError::AccountBlocked } else { SumidaError::InvalidAccount })
    };

    txn.commit().await?;

    result
  }

  pub async fn logout(&mut self) -> Result<(), SumidaError> {
    self.session.logout().await?;
    Ok(())
  }
}

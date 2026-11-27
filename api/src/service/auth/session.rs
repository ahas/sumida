use entity::access_logs::{AccessMethod, AccessResultCode};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::SumidaState;
use crate::service::auth::dto::LoginSession;

pub struct AuthSessionService<S: SumidaState> {
  pub state:   S,
  pub session: UserSession,
}

impl<S: SumidaState> AuthSessionService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn login(&mut self, data: LoginSession) -> Result<(), SumidaError> {
    let db = self.state.db();

    // 1) Return error if the user does not exist
    let Some(user) = Users::find()
      .filter(entity::users::Column::Username.eq(&data.username))
      .filter(entity::users::Column::IsEnabled.eq(true))
      .one(db)
      .await?
    else {
      // Prevent timing-based user enumeration: spend the same argon2 cost even when the account does not exist.
      super::helper::dummy_verify_password(&data.password);
      self
        .session
        .write_log(None, AccessMethod::Password, AccessResultCode::AccountNotFound, db)
        .await?;
      return Err(SumidaError::InvalidAccount);
    };

    // 2) Check the user is banned
    if self.session.check_banned(Some(user.id), db).await? {
      self
        .session
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::AccountBanned, db)
        .await?;
      return Err(SumidaError::AccountBanned);
    }

    // 3) Return error if the user is inactive
    if !user.is_enabled {
      self
        .session
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::AccountInactive, db)
        .await?;
      return Err(SumidaError::InactiveAccount);
    }

    // 4) Check the user password
    if super::helper::verify_password(&data.password, &user.password) {
      super::helper::rehash_if_needed(db, user.id, &user.password, &data.password).await;
      self
        .session
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::Success, db)
        .await?;

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

      self.session.authorize(user.id, &data.username, expiry, db).await?;

      Ok(())
    } else {
      self
        .session
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::PasswordMismatch, db)
        .await?;
      let banned = self.session.ban_if_exceeded_limit(Some(user.id), db).await?;

      Err(if banned { SumidaError::AccountBanned } else { SumidaError::InvalidAccount })
    }
  }

  pub async fn logout(&mut self) -> Result<(), SumidaError> {
    self.session.logout().await?;
    Ok(())
  }
}

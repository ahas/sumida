use entity::prelude::*;
use entity::user_logs::{LoginCode, LoginMethod};
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
      self
        .session
        .write_log(None, LoginMethod::Password, LoginCode::AccountNotFound, db)
        .await?;
      return Err(SumidaError::InvalidAccount);
    };

    // 2) Check the user is blocked
    if self.session.check_blocked(Some(user.id), db).await? {
      self
        .session
        .write_log(Some(user.id), LoginMethod::Password, LoginCode::AccountBlocked, db)
        .await?;
      return Err(SumidaError::AccountBlocked);
    }

    // 3) Return error if the user is inactive
    if !user.is_enabled {
      self
        .session
        .write_log(Some(user.id), LoginMethod::Password, LoginCode::AccountInactive, db)
        .await?;
      return Err(SumidaError::InactiveAccount);
    }

    // 4) Check the user password
    let result = if super::helper::verify_password(&data.password, &user.password) {
      self
        .session
        .write_log(Some(user.id), LoginMethod::Password, LoginCode::Success, db)
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
        .write_log(Some(user.id), LoginMethod::Password, LoginCode::PasswordMismatch, db)
        .await?;
      let blocked = self.session.block_if_exceeded_limit(Some(user.id), db).await?;

      Err(if blocked { SumidaError::AccountBlocked } else { SumidaError::InvalidAccount })
    };

    result
  }

  pub async fn logout(&mut self) -> Result<(), SumidaError> {
    self.session.logout().await?;
    Ok(())
  }
}

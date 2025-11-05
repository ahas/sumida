use chrono::Utc;
use entity::access_logs::{LoginCode, LoginMethod};
use entity::prelude::*;
use error::SumidaError;
use infra::jwt::{JwtTokenStore, RefreshTokenData};
use infra::session::UserSession;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::helper::{self, TokenPair};
use crate::SumidaState;
use crate::service::auth::dto::LoginSession;

pub struct AuthTokenService<S: SumidaState, J: JwtTokenStore> {
  pub state:     S,
  pub session:   UserSession,
  pub jwt_store: J,
}

impl<S: SumidaState, J: JwtTokenStore> AuthTokenService<S, J> {
  pub fn new(state: S, session: UserSession, jwt_store: J) -> Self {
    Self {
      state,
      session,
      jwt_store,
    }
  }

  pub async fn login(&mut self, data: LoginSession) -> Result<TokenPair, SumidaError> {
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
    let result = if helper::verify_password(&data.password, &user.password) {
      self
        .session
        .write_log(Some(user.id), LoginMethod::Password, LoginCode::Success, db)
        .await?;

      // Generate JWT token pair
      let remember_me = data.remember_me.unwrap_or(false);
      let token_pair = helper::generate_token_pair(user.id, &user.username, remember_me).await?;

      // Store refresh token
      let refresh_claims = helper::verify_jwt_token(&token_pair.refresh_token).await?;
      let refresh_data = RefreshTokenData {
        user_id:    user.id,
        username:   user.username.clone(),
        jti:        refresh_claims.jti.clone(),
        expires_at: chrono::DateTime::from_timestamp(refresh_claims.exp, 0).unwrap_or_default(),
        created_at: Utc::now(),
      };

      self.jwt_store.store_refresh_token(&refresh_claims.jti, refresh_data).await?;

      Ok(token_pair)
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

  pub async fn logout(&mut self, refresh_token: Option<String>) -> Result<(), SumidaError>
  where
    J: infra::jwt::JwtTokenStore,
  {
    // If refresh token provided, blacklist it
    if let Some(token) = refresh_token {
      if let Ok(claims) = helper::verify_jwt_token(&token).await {
        self
          .jwt_store
          .blacklist_token(&claims.jti, chrono::DateTime::from_timestamp(claims.exp, 0).unwrap_or_default())
          .await?;
        self.jwt_store.delete_refresh_token(&claims.jti).await?;
      }
    }

    self.session.logout().await?;
    Ok(())
  }

  pub async fn refresh_token(&mut self, refresh_token: String) -> Result<TokenPair, SumidaError>
  where
    J: infra::jwt::JwtTokenStore,
  {
    // Verify refresh token
    let claims = helper::verify_jwt_token(&refresh_token).await?;

    // Check if refresh token type
    if claims.token_type != "refresh" {
      return Err(SumidaError::InvalidJwt);
    }

    // Check if token is blacklisted
    if self.jwt_store.is_token_blacklisted(&claims.jti).await? {
      return Err(SumidaError::InvalidJwt);
    }

    // Get stored refresh token data
    let refresh_data =
      self.jwt_store.get_refresh_token(&claims.jti).await?.ok_or(SumidaError::InvalidJwt)?;

    // Verify user still exists and is active
    let db = self.state.db();
    let user = Users::find_by_id(refresh_data.user_id)
      .one(db)
      .await?
      .ok_or(SumidaError::InvalidAccount)?;

    if !user.is_enabled {
      return Err(SumidaError::InactiveAccount);
    }

    // Check if user is blocked
    if self.session.check_blocked(Some(user.id), db).await? {
      return Err(SumidaError::AccountBlocked);
    }

    // Generate new token pair
    let new_token_pair = helper::generate_token_pair(user.id, &user.username, false).await?;

    // Store new refresh token and invalidate old one
    let new_refresh_claims = helper::verify_jwt_token(&new_token_pair.refresh_token).await?;
    let new_refresh_data = RefreshTokenData {
      user_id:    user.id,
      username:   user.username.clone(),
      jti:        new_refresh_claims.jti.clone(),
      expires_at: chrono::DateTime::from_timestamp(new_refresh_claims.exp, 0).unwrap_or_default(),
      created_at: Utc::now(),
    };

    self.jwt_store.store_refresh_token(&new_refresh_claims.jti, new_refresh_data).await?;
    self.jwt_store.delete_refresh_token(&claims.jti).await?;

    Ok(new_token_pair)
  }
}

pub async fn create_jwt_store<S: SumidaState>(
  state: &S,
) -> Result<impl infra::jwt::JwtTokenStore, SumidaError> {
  #[cfg(feature = "session-store-redis")]
  {
    use infra::jwt::RedisJwtStore;
    let redis_url = state.settings().session.redis.url.as_deref().unwrap_or("redis://localhost:6379/0");
    RedisJwtStore::new(redis_url).await
  }

  #[cfg(all(feature = "session-store-aws-dynamodb", not(feature = "session-store-redis")))]
  {
    use infra::jwt::DynamoJwtStore;
    let dynamodb_client = state.aws_client().dynamodb_client().clone();
    let table_name = state.settings().session.dynamodb.table_name.clone();
    Ok(DynamoJwtStore::new(dynamodb_client, table_name))
  }

  #[cfg(not(any(feature = "session-store-redis", feature = "session-store-aws-dynamodb")))]
  {
    Err(SumidaError::Unsupported("No JWT store implementation available".to_string()))
  }
}

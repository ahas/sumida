//! JWT access/refresh token lifecycle (the `/auth/token` endpoints).
//!
//! - **login**: issues an access + refresh pair and persists the refresh token (keyed by its `jti`)
//!   so it can be rotated and revoked.
//! - **refresh**: validates the refresh token (must be `token_type = "refresh"`, not blacklisted, and
//!   present in the store), re-checks the account is enabled / not banned, issues a NEW pair, and
//!   **rotates** — stores the new refresh `jti` and deletes the old one, so a replayed old refresh fails.
//! - **logout**: blacklists the refresh token *and* the current access token (by `jti`, with a TTL
//!   until expiry) for immediate invalidation, then clears the session.
//!
//! Token-type separation: access tokens authenticate requests (`verify_access_token` rejects a
//! refresh token); refresh tokens are only accepted by `refresh`. The `jti` blacklist makes the
//! otherwise-stateless access token immediately revocable (checked in `layer::user_guard`).

use chrono::Utc;
use entity::access_logs::{AccessMethod, AccessResultCode};
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

  /// Authenticate `data` and issue an access + refresh token pair; the refresh token is persisted
  /// (by `jti`) so it can later be rotated by [`Self::refresh_token`] or revoked by [`Self::logout`].
  pub async fn login(&mut self, data: LoginSession) -> Result<TokenPair, SumidaError> {
    let db = self.state.db();

    // 1) Return error if the user does not exist
    let Some(user) = Users::find()
      .filter(entity::users::Column::Username.eq(&data.username))
      .filter(entity::users::Column::IsEnabled.eq(true))
      .one(db)
      .await?
    else {
      // Prevent timing-based user enumeration: spend the same argon2 cost even when the account does not exist.
      helper::dummy_verify_password(&data.password);
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
    if helper::verify_password(&data.password, &user.password) {
      self
        .session
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::Success, db)
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
        .write_log(Some(user.id), AccessMethod::Password, AccessResultCode::PasswordMismatch, db)
        .await?;
      let banned = self.session.ban_if_exceeded_limit(Some(user.id), db).await?;

      Err(if banned { SumidaError::AccountBanned } else { SumidaError::InvalidAccount })
    }
  }

  /// Revoke a session: blacklist the refresh token (and delete it from the store) and the current
  /// access token by `jti` for immediate invalidation, then clear the session. Each blacklist entry
  /// carries a TTL only until that token's own expiry.
  pub async fn logout(
    &mut self,
    refresh_token: Option<String>,
    access_token: Option<String>,
  ) -> Result<(), SumidaError>
  where
    J: infra::jwt::JwtTokenStore,
  {
    // If a refresh token is present, blacklist it and delete it from the store.
    if let Some(token) = refresh_token
      && let Ok(claims) = helper::verify_jwt_token(&token).await
    {
      self
        .jwt_store
        .blacklist_token(&claims.jti, chrono::DateTime::from_timestamp(claims.exp, 0).unwrap_or_default())
        .await?;
      self.jwt_store.delete_refresh_token(&claims.jti).await?;
    }

    // Blacklist the access token too for immediate invalidation (verify_jwt looks up the jti).
    // Kept with a TTL only until the expiry time.
    if let Some(token) = access_token
      && let Ok(claims) = helper::verify_access_token(&token).await
    {
      self
        .jwt_store
        .blacklist_token(&claims.jti, chrono::DateTime::from_timestamp(claims.exp, 0).unwrap_or_default())
        .await?;
    }

    self.session.logout().await?;
    Ok(())
  }

  /// Validate and **rotate** a refresh token: reject it unless it is a non-blacklisted `refresh`
  /// token still present in the store for an enabled, non-banned account, then issue a new pair.
  /// The presented token is taken out of the store before anything else is decided, so it is spent
  /// even when the account turns out to be refused, and it can buy a pair only once.
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

    // Spend the stored refresh token. Taking it is one step, so of several requests presenting the
    // same token at once exactly one gets it and buys a pair; the rest are refused here.
    let refresh_data =
      self.jwt_store.take_refresh_token(&claims.jti).await?.ok_or(SumidaError::InvalidJwt)?;

    // Verify the account may still be let in — it exists, it is enabled, and no ban catches it.
    let db = self.state.db();
    let user = self.session.admitted_account(refresh_data.user_id, db).await?;

    // A token the account has since disowned. The pair this one belongs to was handed out before
    // the credentials were reset, so it cannot buy a new pair.
    if infra::session::credentials_reset_since(&user, helper::issued_at(&claims)) {
      return Err(SumidaError::InvalidJwt);
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

    Ok(new_token_pair)
  }
}

/// The concrete `JwtTokenStore` type, determined by the enabled cargo feature.
/// (It is a native async trait, so `dyn` is not possible; we handle it via a concrete type alias.)
#[cfg(feature = "session-store-redis")]
pub type JwtStore = infra::jwt::RedisJwtStore;
#[cfg(all(feature = "session-store-aws-dynamodb", not(feature = "session-store-redis")))]
pub type JwtStore = infra::jwt::DynamoJwtStore;

/// Creates the appropriate `JwtTokenStore` implementation based on the enabled cargo feature
/// (`session-store-redis` or `session-store-aws-dynamodb`). A compile error if neither is set.
///
/// Priority: redis > dynamodb (uses redis when both are enabled).
#[cfg(feature = "session-store-redis")]
pub async fn create_jwt_store() -> Result<JwtStore, SumidaError> {
  use infra::jwt::RedisJwtStore;

  let redis_url = infra::settings::get()
    .session
    .redis
    .as_ref()
    .and_then(|x| x.url.clone())
    .unwrap_or_else(|| "redis://localhost:6379/0".to_string());

  RedisJwtStore::new(&redis_url).await
}

#[cfg(all(feature = "session-store-aws-dynamodb", not(feature = "session-store-redis")))]
pub async fn create_jwt_store() -> Result<JwtStore, SumidaError> {
  use aws_sdk_dynamodb::Client as DynamoClient;
  use infra::jwt::DynamoJwtStore;

  let settings = infra::settings::get();
  let table_name = settings
    .session
    .dynamodb
    .as_ref()
    .map(|x| x.table.clone())
    .ok_or_else(|| SumidaError::Unsupported("session.dynamodb.table not configured".to_string()))?;

  let dynamodb_client = DynamoClient::new(infra::aws::get_sdk_config());
  Ok(DynamoJwtStore::new(dynamodb_client, table_name))
}

/// A process-wide shared JWT token store (connects only once on first use).
/// Used for blacklist lookups on the verify path (`user_guard`).
#[cfg(any(feature = "session-store-redis", feature = "session-store-aws-dynamodb"))]
static SHARED_JWT_STORE: tokio::sync::OnceCell<JwtStore> = tokio::sync::OnceCell::const_new();

#[cfg(any(feature = "session-store-redis", feature = "session-store-aws-dynamodb"))]
pub async fn shared_jwt_store() -> Result<&'static JwtStore, SumidaError> {
  SHARED_JWT_STORE.get_or_try_init(create_jwt_store).await
}

#[cfg(not(any(feature = "session-store-redis", feature = "session-store-aws-dynamodb")))]
compile_error!(
  "JWT token endpoints require one of `session-store-redis` or `session-store-aws-dynamodb` features"
);

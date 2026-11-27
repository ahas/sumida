use argon2::{Argon2, PasswordHash, PasswordVerifier};
use cfg_if::cfg_if;
use chrono::Utc;
use error::SumidaError;
use infra::settings;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn verify_password(plain_password: &str, hashed_password: &str) -> bool {
  let argon2 = Argon2::default();
  let Ok(parsed_hash) = PasswordHash::new(hashed_password) else {
    return false;
  };

  argon2.verify_password(plain_password.as_bytes(), &parsed_hash).is_ok()
}

/// Whether a stored hash was made with other parameters than new hashes use.
///
/// Verifying a hash costs what its own parameters say, so an account whose hash is heavier or lighter
/// than the default answers a failed login in a different time than a missing account, whose dummy
/// check costs the default; the difference tells which accounts exist. A hash that does not parse is
/// left to the password check, which refuses it.
pub fn needs_rehash(hashed_password: &str) -> bool {
  let Ok(parsed) = PasswordHash::new(hashed_password) else {
    return false;
  };
  let Ok(params) = argon2::Params::try_from(&parsed) else {
    return true;
  };
  let default = argon2::Params::default();

  parsed.algorithm != argon2::Algorithm::Argon2id.ident()
    || params.m_cost() != default.m_cost()
    || params.t_cost() != default.t_cost()
    || params.p_cost() != default.p_cost()
}

/// After a password was verified, stores it again under the default parameters when its hash was
/// made with others, so every account comes to cost what a missing one costs. Only the hash changes:
/// the account's credentials are the same, so no session is revoked. A failure is logged and the
/// sign-in goes on — the old hash still verifies.
pub async fn rehash_if_needed<C: sea_orm::ConnectionTrait>(
  db: &C,
  user_id: i32,
  stored_hash: &str,
  plain_password: &str,
) {
  use sea_orm::sea_query::Expr;
  use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

  if !needs_rehash(stored_hash) {
    return;
  }
  let Ok(rehashed) = crate::util::user::hash_password(plain_password) else {
    tracing::warn!("user {user_id}: the password could not be hashed again");
    return;
  };
  if let Err(err) = entity::prelude::Users::update_many()
    .col_expr(entity::users::Column::Password, Expr::value(rehashed))
    .filter(entity::users::Column::Id.eq(user_id))
    .exec(db)
    .await
  {
    tracing::warn!("user {user_id}: the rehashed password was not stored: {err}");
  }
}

/// Spends the same argon2 verification cost even for non-existent accounts, preventing
/// timing-based user enumeration that would leak account existence via login response time differences.
pub fn dummy_verify_password(plain_password: &str) {
  static DUMMY_PASSWORD_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();

  let hash = DUMMY_PASSWORD_HASH
    .get_or_init(|| crate::util::user::hash_password("sumida-timing-equalizer").unwrap_or_default());

  let _ = verify_password(plain_password, hash);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JwtClaims {
  pub sub:        String, // Subject (user_id)
  pub username:   String,
  pub iat:        i64,    // Issued at
  pub exp:        i64,    // Expiration
  pub jti:        String, // JWT ID (for blacklisting)
  pub token_type: String, // "access" or "refresh"
}

/// What the token route hands out.
///
/// **These keys are not renamed.** Unlike the rest of this API's bodies, they go out exactly as
/// they are written here.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenPair {
  /// Sent as `Authorization: Bearer <token>` to authenticate a request.
  pub access_token:  String,
  /// Traded at the rotation route for the next pair. It buys exactly one, and is destroyed as the
  /// next one is stored.
  pub refresh_token: String,
  /// Always `Bearer`.
  pub token_type:    String,
  /// How many seconds the access token has left. The refresh token outlives it, by how much is a
  /// deployment setting.
  pub expires_in:    i64,
}

pub async fn get_jwt_secret() -> Result<String, SumidaError> {
  let secret = get_jwt_secret_raw().await?;

  // HS256 key strength: shorter than 256 bits (32 bytes) is vulnerable to key brute-forcing.
  if secret.len() < 32 {
    return Err(SumidaError::Unsupported("JWT secret must be at least 32 bytes (256 bits)".to_string()));
  }

  Ok(secret)
}

async fn get_jwt_secret_raw() -> Result<String, SumidaError> {
  #[cfg(not(any(feature = "jwt-secret-store-static", feature = "jwt-secret-store-aws-ssm")))]
  compile_error!("Either `jwt-secret-store-static` or `jwt-secret-store-aws-ssm` feature must be enabled");

  let settings = settings::get();

  cfg_if! {
    if #[cfg(feature = "jwt-secret-store-static")] {
      settings
        .jwt
        .as_ref()
        .and_then(|x| x.secret.clone())
        .ok_or(SumidaError::NoJwtSecret)
    } else if #[cfg(feature = "jwt-secret-store-aws-ssm")] {
      use infra::aws;
      let ssm_client = aws_sdk_ssm::Client::new(aws::get_sdk_config());
      let ssm_path = settings
        .jwt
        .as_ref()
        .and_then(|x| x.ssm_path.clone())
        .unwrap_or_else(|| format!("/sumida/{}/jwt-secret", settings.server.stage));
      let parameter = ssm_client
        .get_parameter()
        .name(ssm_path)
        .with_decryption(true)
        .send()
        .await
        .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

      parameter
        .parameter
        .and_then(|param| param.value)
        .ok_or(SumidaError::NoJwtSecret)
    }
  }
}

pub async fn generate_access_token(user_id: i32, username: &str) -> Result<String, SumidaError> {
  let settings = settings::get();
  let jwt_settings = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?;

  let now = Utc::now();
  let expiry = jwt_settings.expiry.access_token_expiry()?;
  let exp = match expiry {
    tower_sessions::Expiry::AtDateTime(dt) => dt.unix_timestamp(),
    _ => return Err(SumidaError::InvalidJwtExpiry),
  };

  let claims = JwtClaims {
    sub: user_id.to_string(),
    username: username.to_owned(),
    iat: now.timestamp(),
    exp,
    jti: Uuid::new_v4().to_string(),
    token_type: "access".to_owned(),
  };

  let secret = get_jwt_secret().await?;
  let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref()))
    .map_err(|_| SumidaError::JwtGenerationFailed)?;

  Ok(token)
}

pub async fn generate_refresh_token(
  user_id: i32,
  username: &str,
  remember_me: bool,
) -> Result<String, SumidaError> {
  let settings = settings::get();
  let jwt_settings = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?;

  let now = Utc::now();
  let expiry = if remember_me {
    jwt_settings.expiry.remember_me_expiry()?
  } else {
    jwt_settings.expiry.refresh_token_expiry()?
  };

  let exp = match expiry {
    tower_sessions::Expiry::AtDateTime(dt) => dt.unix_timestamp(),
    _ => return Err(SumidaError::InvalidJwtExpiry),
  };

  let claims = JwtClaims {
    sub: user_id.to_string(),
    username: username.to_owned(),
    iat: now.timestamp(),
    exp,
    jti: Uuid::new_v4().to_string(),
    token_type: "refresh".to_owned(),
  };

  let secret = get_jwt_secret().await?;
  let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref()))
    .map_err(|_| SumidaError::JwtGenerationFailed)?;

  Ok(token)
}

pub async fn verify_jwt_token(token: &str) -> Result<JwtClaims, SumidaError> {
  let secret = get_jwt_secret().await?;
  let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
  validation.set_required_spec_claims(&["exp", "sub", "iat", "jti"]);

  let token = decode::<JwtClaims>(token, &DecodingKey::from_secret(secret.as_ref()), &validation)
    .map_err(|_| SumidaError::InvalidJwt)?;

  Ok(token.claims)
}

/// The moment a token says it was issued.
///
/// A claim carries whole seconds, so this is the second the token was minted in rather than the
/// instant.
pub fn issued_at(claims: &JwtClaims) -> sea_orm::prelude::DateTimeUtc {
  chrono::DateTime::from_timestamp(claims.iat, 0).unwrap_or_default()
}

/// Access-token-only verification. Allows only `token_type == "access"`.
pub async fn verify_access_token(token: &str) -> Result<JwtClaims, SumidaError> {
  let claims = verify_jwt_token(token).await?;
  if claims.token_type != "access" {
    return Err(SumidaError::InvalidJwt);
  }
  Ok(claims)
}

pub async fn generate_token_pair(
  user_id: i32,
  username: &str,
  remember_me: bool,
) -> Result<TokenPair, SumidaError> {
  let access_token = generate_access_token(user_id, username).await?;
  let refresh_token = generate_refresh_token(user_id, username, remember_me).await?;

  let settings = settings::get();
  let jwt_settings = settings.jwt.as_ref().ok_or(SumidaError::NoJwtSecret)?;
  let access_expiry = jwt_settings.expiry.access_token_expiry()?;

  let expires_in = match access_expiry {
    tower_sessions::Expiry::AtDateTime(dt) => dt.unix_timestamp() - Utc::now().timestamp(),
    _ => 3600, // 1 hour fallback
  };

  Ok(TokenPair {
    access_token,
    refresh_token,
    token_type: "Bearer".to_owned(),
    expires_in,
  })
}

#[cfg(test)]
mod rehash_tests {
  use argon2::password_hash::rand_core::OsRng;
  use argon2::password_hash::{PasswordHasher, SaltString};
  use argon2::{Algorithm, Argon2, Params, Version};

  use super::needs_rehash;

  fn hash_with(algorithm: Algorithm, params: Params) -> String {
    Argon2::new(algorithm, Version::V0x13, params)
      .hash_password(b"secret", &SaltString::generate(&mut OsRng))
      .unwrap()
      .to_string()
  }

  #[test]
  fn a_default_hash_stays() {
    assert!(!needs_rehash(&crate::util::user::hash_password("secret").unwrap()));
  }

  #[test]
  fn a_heavier_or_lighter_hash_is_redone() {
    assert!(needs_rehash(&hash_with(Algorithm::Argon2id, Params::new(65536, 3, 4, None).unwrap())));
    assert!(needs_rehash(&hash_with(Algorithm::Argon2d, Params::new(32, 1, 4, None).unwrap())));
  }

  #[test]
  fn a_hash_that_does_not_parse_is_left_to_the_password_check() {
    assert!(!needs_rehash("not a hash"));
  }
}

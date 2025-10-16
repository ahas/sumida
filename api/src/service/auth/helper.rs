use argon2::{Argon2, PasswordHash, PasswordVerifier};
use cfg_if::cfg_if;
use chrono::Utc;
use error::SumidaError;
use infra::settings;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn verify_password(password: &str, password_hash: &str) -> bool {
  let argon2 = Argon2::default();
  let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
    return false;
  };

  argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
  pub sub:        String, // Subject (user_id)
  pub username:   String,
  pub iat:        i64,    // Issued at
  pub exp:        i64,    // Expiration
  pub jti:        String, // JWT ID (for blacklisting)
  pub token_type: String, // "access" or "refresh"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenPair {
  pub access_token:  String,
  pub refresh_token: String,
  pub token_type:    String,
  pub expires_in:    i64,
}

pub async fn get_jwt_secret() -> Result<String, SumidaError> {
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
      let parameter = ssm_client
        .get_parameter()
        .name(format!("/sumida/{}/jwt-secret", settings.server.stage))
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

  let token =
    decode::<JwtClaims>(token, &DecodingKey::from_secret(secret.as_ref()), &Validation::default())
      .map_err(|_| SumidaError::InvalidJwt)?;

  Ok(token.claims)
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

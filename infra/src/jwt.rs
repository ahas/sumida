use async_trait::async_trait;
use cfg_if::cfg_if;
use chrono::{DateTime, Utc};
use error::SumidaError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenData {
  pub user_id: i32,
  pub username: String,
  pub jti: String,
  pub expires_at: DateTime<Utc>,
  pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
  pub jti: String,
  pub expires_at: DateTime<Utc>,
  pub blacklisted_at: DateTime<Utc>,
}

#[async_trait]
pub trait JwtTokenStore: Send + Sync {
  fn store_refresh_token(
    &self,
    token_id: &str,
    data: RefreshTokenData,
  ) -> impl Future<Output = Result<(), SumidaError>>;
  fn get_refresh_token(
    &self,
    token_id: &str,
  ) -> impl Future<Output = Result<Option<RefreshTokenData>, SumidaError>>;
  fn delete_refresh_token(&self, token_id: &str) -> impl Future<Output = Result<(), SumidaError>>;
  fn blacklist_token(
    &self,
    jti: &str,
    expires_at: DateTime<Utc>,
  ) -> impl Future<Output = Result<(), SumidaError>>;
  fn is_token_blacklisted(&self, jti: &str) -> impl Future<Output = Result<bool, SumidaError>>;
  fn cleanup_expired(&self) -> impl Future<Output = Result<(), SumidaError>>;
}

cfg_if! {
  if #[cfg(feature = "session-store-redis")] {
    // TODO: Redis JWT store implementation
    // Currently disabled due to Fred API complexity
    // Will implement when we find a simpler Redis abstraction

    pub struct RedisJwtStore;

    impl RedisJwtStore {
      pub async fn new(redis_url: &str) -> Result<Self, SumidaError> {
        // TODO: Implement Redis connection using fred
        // - Create Redis client/pool from redis_url
        // - Connect and wait for connection
        // - Store connection handle for later use
        let _ = redis_url;
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }
    }

    impl JwtTokenStore for RedisJwtStore {
      async fn store_refresh_token(&self, token_id: &str, data: RefreshTokenData) -> Result<(), SumidaError> {
        // TODO: Implement refresh token storage
        // - Serialize RefreshTokenData to JSON
        // - Store in Redis with key "refresh_token:{token_id}"
        // - Set TTL based on data.expires_at
        let _ = (token_id, data);
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }

      async fn get_refresh_token(&self, token_id: &str) -> Result<Option<RefreshTokenData>, SumidaError> {
        // TODO: Implement refresh token retrieval
        // - Get value from Redis with key "refresh_token:{token_id}"
        // - Deserialize JSON to RefreshTokenData if exists
        // - Return None if key doesn't exist
        let _ = token_id;
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }

      async fn delete_refresh_token(&self, token_id: &str) -> Result<(), SumidaError> {
        // TODO: Implement refresh token deletion
        // - Delete key "refresh_token:{token_id}" from Redis
        let _ = token_id;
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }

      async fn blacklist_token(&self, jti: &str, expires_at: DateTime<Utc>) -> Result<(), SumidaError> {
        // TODO: Implement token blacklisting
        // - Create BlacklistEntry with jti, expires_at, and current timestamp
        // - Serialize and store in Redis with key "blacklist:{jti}"
        // - Set TTL based on expires_at
        let _ = (jti, expires_at);
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }

      async fn is_token_blacklisted(&self, jti: &str) -> Result<bool, SumidaError> {
        // TODO: Implement blacklist check
        // - Check if key "blacklist:{jti}" exists in Redis
        // - Return true if exists, false otherwise
        let _ = jti;
        Err(SumidaError::Unsupported("Redis JWT store not yet implemented".to_string()))
      }

      async fn cleanup_expired(&self) -> Result<(), SumidaError> {
        // TODO: Implement expired token cleanup (optional)
        // - Redis automatically expires keys with TTL
        // - This method can remain empty or implement manual cleanup logic
        Ok(())
      }
    }
  } else if #[cfg(feature = "session-store-aws-dynamodb")] {
    use aws_sdk_dynamodb::{Client as DynamoClient, types::AttributeValue};
    use std::collections::HashMap;

    pub struct DynamoJwtStore {
      client: DynamoClient,
      table_name: String,
    }

    impl DynamoJwtStore {
      pub fn new(client: DynamoClient, table_name: String) -> Self {
        Self { client, table_name }
      }
    }

    impl JwtTokenStore for DynamoJwtStore {
      async fn store_refresh_token(&self, token_id: &str, data: RefreshTokenData) -> Result<(), SumidaError> {
        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id)));
        item.insert("sk".to_string(), AttributeValue::S("data".to_string()));
        item.insert("user_id".to_string(), AttributeValue::N(data.user_id.to_string()));
        item.insert("username".to_string(), AttributeValue::S(data.username));
        item.insert("jti".to_string(), AttributeValue::S(data.jti));
        item.insert("expires_at".to_string(), AttributeValue::N(data.expires_at.timestamp().to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(data.created_at.timestamp().to_string()));
        item.insert("ttl".to_string(), AttributeValue::N(data.expires_at.timestamp().to_string()));

        self.client
          .put_item()
          .table_name(&self.table_name)
          .set_item(Some(item))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn get_refresh_token(&self, token_id: &str) -> Result<Option<RefreshTokenData>, SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        let result = self.client
          .get_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        match result.item {
          Some(item) => {
            let user_id = item.get("user_id")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i32>().ok())
              .ok_or(SumidaError::Unsupported("Invalid user_id".to_string()))?;

            let username = item.get("username")
              .and_then(|v| v.as_s().ok())
              .ok_or(SumidaError::Unsupported("Invalid username".to_string()))?;

            let jti = item.get("jti")
              .and_then(|v| v.as_s().ok())
              .ok_or(SumidaError::Unsupported("Invalid jti".to_string()))?;

            let expires_at_ts = item.get("expires_at")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i64>().ok())
              .ok_or(SumidaError::Unsupported("Invalid expires_at".to_string()))?;

            let created_at_ts = item.get("created_at")
              .and_then(|v| v.as_n().ok())
              .and_then(|n| n.parse::<i64>().ok())
              .ok_or(SumidaError::Unsupported("Invalid created_at".to_string()))?;

            let data = RefreshTokenData {
              user_id,
              username: username.clone(),
              jti: jti.clone(),
              expires_at: DateTime::from_timestamp(expires_at_ts, 0).unwrap_or_default(),
              created_at: DateTime::from_timestamp(created_at_ts, 0).unwrap_or_default(),
            };

            Ok(Some(data))
          }
          None => Ok(None),
        }
      }

      async fn delete_refresh_token(&self, token_id: &str) -> Result<(), SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("refresh#{}", token_id))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        self.client
          .delete_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn blacklist_token(&self, jti: &str, expires_at: DateTime<Utc>) -> Result<(), SumidaError> {
        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(format!("blacklist#{}", jti)));
        item.insert("sk".to_string(), AttributeValue::S("data".to_string()));
        item.insert("jti".to_string(), AttributeValue::S(jti.to_string()));
        item.insert("expires_at".to_string(), AttributeValue::N(expires_at.timestamp().to_string()));
        item.insert("blacklisted_at".to_string(), AttributeValue::N(Utc::now().timestamp().to_string()));
        item.insert("ttl".to_string(), AttributeValue::N(expires_at.timestamp().to_string()));

        self.client
          .put_item()
          .table_name(&self.table_name)
          .set_item(Some(item))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(())
      }

      async fn is_token_blacklisted(&self, jti: &str) -> Result<bool, SumidaError> {
        let key = HashMap::from([
          ("pk".to_string(), AttributeValue::S(format!("blacklist#{}", jti))),
          ("sk".to_string(), AttributeValue::S("data".to_string())),
        ]);

        let result = self.client
          .get_item()
          .table_name(&self.table_name)
          .set_key(Some(key))
          .send()
          .await
          .map_err(|e| SumidaError::Unsupported(e.to_string()))?;

        Ok(result.item.is_some())
      }

      async fn cleanup_expired(&self) -> Result<(), SumidaError> {
        // DynamoDB automatically expires items with TTL, so no manual cleanup needed
        Ok(())
      }
    }
  }
}

use std::sync::{Arc, Mutex};

use tower_sessions_redis_store::RedisStore;
use tower_sessions_redis_store::fred::prelude::*;
use tower_sessions_redis_store::fred::types::ConnectHandle;

use crate::settings::session::SessionRedisSettings;

#[derive(Clone, Debug)]
pub struct RedisSessionStore {
  pub pool: Pool,
  pub handle: Arc<Mutex<ConnectHandle>>,
}

impl RedisSessionStore {
  pub async fn new(settings: &SessionRedisSettings) -> Result<Self, anyhow::Error> {
    let config = Config::from_url(&settings.url.as_deref().unwrap_or("http://localhost:6379/0")).unwrap();
    let pool = Pool::new(config, None, None, None, settings.size.unwrap_or(10))?;
    let handle = Arc::new(Mutex::new(pool.connect()));

    pool.wait_for_connect().await?;

    Ok(Self { handle, pool })
  }

  pub fn store(&self) -> RedisStore<Pool> {
    RedisStore::new(self.pool.clone())
  }
}

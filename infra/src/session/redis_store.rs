use tower_sessions_redis_store::RedisStore;
use tower_sessions_redis_store::fred::prelude::*;

use crate::settings::session::SessionRedisSettings;

pub async fn new(settings: &SessionRedisSettings) -> Result<RedisStore<Pool>, anyhow::Error> {
  let config = Config::from_url(&settings.url.as_deref().unwrap_or("http://localhost:6379/0")).unwrap();
  let pool = Pool::new(config, None, None, None, settings.size.unwrap_or(6))?;
  let _ = pool.init().await?;
  pool.wait_for_connect().await?;

  Ok(RedisStore::new(pool))
}

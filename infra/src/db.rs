use sea_orm::{
  ConnectOptions, DatabaseConnection, SqlxPostgresConnector,
  sqlx::{Pool, Postgres, pool::PoolOptions},
};
use std::str::FromStr;
use tokio::sync::OnceCell;

use crate::settings;

static CONN: OnceCell<DatabaseConnection> = OnceCell::const_new();

fn parse_to_duration(value: Option<&String>) -> Option<std::time::Duration> {
  value.and_then(|x| humantime::parse_duration(x).ok())
}

fn get_pg_pool_options() -> (String, PoolOptions<Postgres>) {
  let config = &settings::get().database;
  let connect_timeout = parse_to_duration(config.connect_timeout.as_ref());
  let idle_timeout = parse_to_duration(config.idle_timeout.as_ref());
  let acquire_timeout = parse_to_duration(config.acquire_timeout.as_ref());
  let acquire_slow_threshold = parse_to_duration(config.acquire_slow_threshold.as_ref());
  let max_lifetime = parse_to_duration(config.max_lifetime.as_ref());

  // Connection Options
  let mut conn_opts = ConnectOptions::new(&config.url);

  tracing::info!("database connection established");

  if let Some(v) = config.max_connections {
    conn_opts.max_connections(v);
  }
  if let Some(v) = config.min_connections {
    conn_opts.min_connections(v);
  }
  if let Some(v) = connect_timeout {
    conn_opts.connect_timeout(v);
  }
  if let Some(v) = idle_timeout {
    conn_opts.idle_timeout(v);
  }
  if let Some(v) = acquire_timeout {
    conn_opts.acquire_timeout(v);
  }
  if let Some(v) = max_lifetime {
    conn_opts.max_lifetime(v);
  }

  let logging = config.logging.clone().unwrap_or("off".to_string());
  let is_logging_enabled = logging != "off" && logging != "false";
  conn_opts.sqlx_logging(is_logging_enabled);

  if is_logging_enabled {
    let filter = tracing::log::LevelFilter::from_str(&logging).unwrap();
    conn_opts.sqlx_logging_level(filter);
  }

  // Pool Options
  let pool_opts = conn_opts.sqlx_pool_options::<Postgres>();

  let pool_opts = if let Some(v) = acquire_slow_threshold {
    pool_opts.acquire_slow_threshold(v)
  } else {
    pool_opts
  };

  (config.url.clone(), pool_opts)
}

pub async fn establish_connection() {
  let _conn = CONN
    .get_or_init(|| async {
      let (url, pool_opts) = get_pg_pool_options();
      let pg_pool: Pool<Postgres> = pool_opts.connect(&url).await.expect("Failed to connect Postgres pool");
      SqlxPostgresConnector::from_sqlx_postgres_pool(pg_pool)
    })
    .await;
}

pub fn get_connection() -> &'static DatabaseConnection {
  CONN.get().unwrap()
}

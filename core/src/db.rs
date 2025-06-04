use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::{str::FromStr, time::Duration};
use tokio::sync::OnceCell;

use crate::core::settings;

static CONN: OnceCell<DatabaseConnection> = OnceCell::const_new();

fn get_options_from_settings() -> ConnectOptions {
  let config = &settings::get().database;

  let mut opts = ConnectOptions::new(&config.url);

  tracing::info!("database connection established");

  if let Some(v) = config.max_connections {
    opts.max_connections(v);
  }
  if let Some(v) = config.min_connections {
    opts.min_connections(v);
  }
  if let Some(v) = config.connect_timeout {
    opts.connect_timeout(Duration::from_millis(v));
  }
  if let Some(v) = config.idle_timeout {
    opts.idle_timeout(Duration::from_millis(v));
  }
  if let Some(v) = config.acquire_timeout {
    opts.acquire_timeout(Duration::from_millis(v));
  }
  if let Some(v) = config.max_lifetime {
    opts.max_lifetime(Duration::from_millis(v));
  }

  let logging = config.logging.clone().unwrap_or("off".to_string());
  let is_logging_enabled = logging != "off" && logging != "false";
  opts.sqlx_logging(is_logging_enabled);

  if is_logging_enabled {
    let filter = tracing::log::LevelFilter::from_str(&logging).unwrap();
    opts.sqlx_logging_level(filter);
  }

  opts
}

pub async fn establish_connection() {
  let _conn = CONN
    .get_or_init(|| async {
      Database::connect(get_options_from_settings()).await.expect("database connection failed")
    })
    .await;
}

pub fn get_connection() -> &'static DatabaseConnection {
  CONN.get().unwrap()
}

#[cfg(test)]
pub fn set_test_connection(conn: DatabaseConnection) {
  let _ = CONN.set(conn);
}

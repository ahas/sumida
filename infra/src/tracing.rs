use std::str::FromStr;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use super::settings;

pub fn subscribe() {
  let settings = settings::get();
  let logging = settings.server.logging.clone().unwrap_or("off".to_string());

  if logging != "off" && logging != "false" {
    let filter = tracing_subscriber::EnvFilter::from_str(&logging).unwrap();

    tracing_subscriber::registry()
      .with(filter)
      .with(tracing_subscriber::fmt::layer())
      .init();
  }
}

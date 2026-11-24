use tokio::sync::OnceCell;

use crate::db::TestDb;

static INIT: OnceCell<()> = OnceCell::const_new();

pub async fn init_once() {
  INIT
    .get_or_init(|| async {
      // `.env.e2e` fills in what the environment leaves unset; a run handed every variable by its
      // caller needs no file.
      if let Err(error) = dotenvy::from_filename(".env.e2e")
        && !error.not_found()
      {
        panic!("Failed to load .env.e2e: {error}");
      }
      crate::testbed::configure();

      let settings = infra::settings::get();

      if let Some(filter) = settings.server.logging.as_deref() {
        tracing_subscriber::fmt().with_env_filter(filter).try_init().ok();
      }

      TestDb::setup().await.expect("Failed to setup test db");
    })
    .await;
}

use anyhow::Context;
use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
use tokio::sync::OnceCell;

#[allow(dead_code)]
pub struct TestDb {
  name: String,
}

static TEST_DB: OnceCell<TestDb> = OnceCell::const_new();

impl TestDb {
  pub async fn get() -> &'static TestDb {
    TEST_DB.get_or_init(|| async { TestDb::setup().await.unwrap() }).await
  }

  pub async fn setup() -> anyhow::Result<Self> {
    let db_name = Self::find_or_create().await?;
    Self::run_migration_fresh().await?;

    Ok(Self { name: db_name })
  }

  async fn find_or_create() -> anyhow::Result<String> {
    let settings = infra::settings::get();
    let admin_url = std::env::var("DATABASE_ADMIN_URL").context("Missing env: DATABASE_ADMIN_URL")?;
    let test_url = settings.database.url.clone();

    let db_name = test_url //
      .split('/')
      .next_back()
      .ok_or_else(|| anyhow::anyhow!("Invalid DATABASE_URL: missing DB name"))?;

    // 1. Connect to admin DB
    let admin = Database::connect(&admin_url).await.context("Failed to connect to admin DB")?;

    // 2. Create test DB if it doesn't exist
    let exists = admin
      .query_one(Statement::from_string(
        DbBackend::Postgres,
        format!("SELECT 1 FROM pg_database WHERE datname = '{db_name}'"),
      ))
      .await?
      .is_some();

    if !exists {
      admin
        .execute(Statement::from_string(DbBackend::Postgres, format!("CREATE DATABASE {db_name}")))
        .await
        .context("Failed to create test database")?;

      tracing::info!("✅ Created database `{db_name}`");
    } else {
      tracing::info!("ℹ️ Database `{db_name}` already exists");
    }

    Ok(db_name.to_string())
  }

  async fn run_migration_fresh() -> anyhow::Result<()> {
    use crate::common::migration::Migrator;
    use sea_orm_cli::MigrateSubcommands;
    use sea_orm_migration::prelude::*;

    cli::run_migrate(Migrator, infra::db::get_connection(), Some(MigrateSubcommands::Fresh), false)
      .await
      .expect("Migration fresh command failed");

    tracing::info!("✅ Applied fresh migrations");

    Ok(())
  }
}

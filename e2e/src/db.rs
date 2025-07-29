use anyhow::Context;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};

pub async fn setup_test_db() -> anyhow::Result<()> {
  // dotenvy::dotenv().ok();
  dotenvy::from_filename(".env.e2e").expect("Failed to load .env.e2e");

  let settings = infra::settings::get();
  infra::db::establish_connection().await;

  let admin_url = std::env::var("DATABASE_ADMIN_URL").context("Missing env: DATABASE_ADMIN_URL")?;
  let test_url = settings.database.url.clone();

  let db_name = test_url //
    .split('/')
    .last()
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

    println!("✅ Created database `{db_name}`");
  } else {
    println!("ℹ️ Database `{db_name}` already exists");
  }

  // 3. Connect to test DB
  let db = infra::db::get_connection();

  // 4. Run `sea-orm-cli migrate fresh`
  run_migration_fresh(&db).await?;

  Ok(())
}

async fn run_migration_fresh(db: &DatabaseConnection) -> anyhow::Result<()> {
  use crate::migration::Migrator;
  use sea_orm_cli::MigrateSubcommands;
  use sea_orm_migration::prelude::*;

  cli::run_migrate(Migrator, db, Some(MigrateSubcommands::Fresh), false)
    .await
    .expect("Migration fresh command failed");

  println!("✅ Applied fresh migrations");

  Ok(())
}

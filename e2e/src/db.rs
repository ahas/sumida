use anyhow::Context;
use api::authz::Authz;
use sea_orm::prelude::*;
use sea_orm::{Database, DbBackend, Statement};

#[allow(dead_code)]
pub struct TestDb;

impl TestDb {
  pub async fn setup() -> Result<Self, anyhow::Error> {
    // The test database is made before the pool connects to it, so a server that has never held it works.
    Self::find_or_create().await?;

    let db = infra::db::connect().await;

    Self::run_migration_fresh(&db).await?;
    Self::sync_vocabulary(&db).await?;

    db.close().await?;

    Ok(Self)
  }

  async fn find_or_create() -> anyhow::Result<String> {
    let settings = infra::settings::get();
    let admin_url = std::env::var("DATABASE_ADMIN_URL").context("Missing env: DATABASE_ADMIN_URL")?;
    let test_url = settings.database.url.clone();

    // The name is the address's path, read as an address: a query after it, such as a socket's `host`, is not
    // part of it.
    let test_address = url::Url::parse(&test_url).context("Invalid DATABASE_URL")?;
    let db_name = test_address.path().trim_start_matches('/').to_owned();

    if db_name.is_empty() {
      anyhow::bail!("Invalid DATABASE_URL: missing DB name");
    }

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

  #[allow(dead_code)]
  async fn run_migration_up(db: &DatabaseConnection) -> anyhow::Result<()> {
    use sea_orm_migration::prelude::*;

    use crate::migration::Migrator;

    Migrator::up(db, None).await.expect("Migration up command failed");

    tracing::info!("✅ Applied pending migrations");

    Ok(())
  }

  /// Use this method only when you need to completely reset the database
  #[allow(dead_code)]
  pub async fn reset_database() -> anyhow::Result<()> {
    let db = infra::db::connect().await;
    Self::run_migration_fresh(&db).await?;
    Self::sync_vocabulary(&db).await?;

    db.close().await?;
    Ok(())
  }

  /// Brings `relation_definitions` up to date with the compiled rule set.
  ///
  /// The `rebac` migration seeds only the handful of relations the backfill can write, and the
  /// foreign key on `relation_tuples` refuses everything else — so without this a fixture cannot
  /// grant `service:<ns>#op_read` or `board:<id>#viewer` at all. `Authz::boot` does the same thing
  /// in a live process.
  ///
  /// It also replaces the `* | * | {*} | user:admin` policy row this fixture used to seed. That row
  /// is superseded rather than migrated: `platform:root#superuser` reaches every rule through its
  /// `flagged` branch, which is `users.is_admin` itself — the flag `TestUser::create_admin` already
  /// sets. Keeping the row would put one grant in two places.
  async fn sync_vocabulary(db: &DatabaseConnection) -> anyhow::Result<()> {
    Authz::sync_schema(db).await.map_err(|error| anyhow::anyhow!("{error}"))?;
    tracing::info!("✅ Synced the authz vocabulary projection");
    Ok(())
  }

  async fn run_migration_fresh(db: &DatabaseConnection) -> anyhow::Result<()> {
    use sea_orm_migration::prelude::*;

    use crate::migration::Migrator;

    Migrator::fresh(db).await.expect("Migration fresh command failed");

    tracing::info!("✅ Applied fresh migrations");

    Ok(())
  }
}

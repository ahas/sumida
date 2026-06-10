use sea_orm::{ConnectionTrait, FromQueryResult, Statement};
use sea_orm_migration::prelude::*;

/// Converts plaintext API keys in an existing production DB into blake3 hashes.
///
/// The code (`api_key::hash_api_key`) was changed to store/look up keys as hashes. This converts
/// both the keys already stored as plaintext and the `access_policies."user"` (`api:<plain>`) that
/// use those keys as identifiers, so existing keys keep working.
///
/// Notes:
/// - On a fresh deploy (empty table right after init) it does nothing.
/// - Rows that are already hashed (64-char lowercase hex) are skipped (protects keys created after code deploy).
/// - Hashing is one-way, so `down` is not possible.
/// - It is safer to **run this migration before deploying the code**.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(FromQueryResult)]
struct KeyRow {
  key: String,
}

/// If it looks like a blake3 hex hash (64 chars, lowercase hex), treat it as already converted.
fn looks_hashed(s: &str) -> bool {
  s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    let backend = db.get_database_backend();

    // Add the identifying prefix column if it is missing (handles existing DBs not covered by init).
    db.execute(Statement::from_string(
      backend,
      "ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS key_prefix VARCHAR",
    ))
    .await?;

    let rows = KeyRow::find_by_statement(Statement::from_string(backend, "SELECT key FROM api_keys"))
      .all(db)
      .await?;

    for row in rows {
      let plaintext = row.key;
      if looks_hashed(&plaintext) {
        continue;
      }
      let hash = blake3::hash(plaintext.as_bytes()).to_hex().to_string();
      let prefix: String = plaintext.chars().take(8).collect();

      // First convert the api:<plain> identifier in access_policies to api:<hash>.
      db.execute(Statement::from_sql_and_values(
        backend,
        r#"UPDATE access_policies SET "user" = $1 WHERE "user" = $2"#,
        [format!("api:{hash}").into(), format!("api:{plaintext}").into()],
      ))
      .await?;

      // api_keys.key (PK) plaintext → hash. Also fill in prefix (the leading part of the plaintext).
      db.execute(Statement::from_sql_and_values(
        backend,
        "UPDATE api_keys SET key = $1, key_prefix = $2 WHERE key = $3",
        [hash.into(), prefix.into(), plaintext.into()],
      ))
      .await?;
    }

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration(
      "api_key hashing is one-way and cannot be reverted".to_owned(),
    ))
  }
}

//! Generic file store lifecycle against a real database.
//!
//! The reaper decides what to delete from four columns at once — status, expiry, reference count,
//! and whether a junction row still points at the file. Unit-level reasoning covers each column,
//! but the query that reads them together is the thing that removes data, and nothing exercised it
//! against a database.
//!
//! The shared test DB is migrated fresh once per binary and other tests leave rows behind, so every
//! assertion here targets the file it created and never a global count.
//!
//! Every case here is serialized on the maintenance key. The reaper sweeps the whole table, so a
//! case that runs it beside one that is only reading the candidate list would delete that case's
//! file before it looks.

use api::service::fs::maintenance::reap_orphan_files;
use api::util::file_lifecycle;
use api::util::storage::{StaticFile, Storage, SumidaStorage};
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::fs::files::FileStatus;
use entity::prelude::*;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use uuid::Uuid;

/// The shared database, migrated once per binary.
async fn db() -> sea_orm::DatabaseConnection {
  bootstrap::init_once().await;
  infra::db::connect().await
}

/// A file row placed directly, so the test states the exact combination it means to check.
async fn place(
  db: &sea_orm::DatabaseConnection,
  status: FileStatus,
  refs: i32,
  expires_in: Option<Duration>,
) -> Uuid {
  let uuid = Uuid::new_v4();

  entity::fs::files::ActiveModel {
    uuid: Set(uuid),
    user_id: Set(None),
    original_name: Set("lifecycle.txt".into()),
    dir: Set("e2e".into()),
    name: Set(format!("{uuid}.txt")),
    uri: Set(None),
    size: Set(4),
    mime_type: Set("text/plain".into()),
    hash: Set(None),
    tags: Set(None),
    status: Set(status),
    refs: Set(refs),
    expires_at: Set(expires_in.map(|d| Utc::now() + d)),
    ..Default::default()
  }
  .insert(db)
  .await
  .expect("place file");

  uuid
}

async fn is_orphan_candidate(db: &sea_orm::DatabaseConnection, uuid: Uuid) -> bool {
  file_lifecycle::find_orphan_files(500, db)
    .await
    .expect("find orphans")
    .iter()
    .any(|f| f.uuid == uuid)
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_expired_unreferenced_temporary_file_is_a_candidate() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  assert!(
    is_orphan_candidate(&db, file).await,
    "temporary, expired and unreferenced is exactly what the reaper collects"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_grace_period_holds_the_file() {
  let db = db().await;
  // The only difference from the collected case is that the deadline has not passed. Uploading and
  // attaching are separate requests, and this window is what keeps the gap from deleting the file
  // between them.
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(1))).await;

  assert!(
    !is_orphan_candidate(&db, file).await,
    "a file whose deadline has not passed is not collected"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_reference_holds_the_file() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 1, Some(Duration::hours(-1))).await;

  assert!(
    !is_orphan_candidate(&db, file).await,
    "something still points at it, so expiry alone does not collect it"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_file_without_a_deadline_is_never_collected() {
  let db = db().await;
  // A null deadline is how a file says it is not on the clock. Reading it as "expired long ago"
  // would delete every such file on the first pass.
  let file = place(&db, FileStatus::Temporary, 0, None).await;

  assert!(!is_orphan_candidate(&db, file).await, "no deadline means no collection");
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_promoted_file_leaves_the_candidate_set() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  assert!(is_orphan_candidate(&db, file).await);

  file_lifecycle::promote_active(file, &db).await.expect("promote");

  assert!(
    !is_orphan_candidate(&db, file).await,
    "promotion is what an attach does to the file, and it takes the file off the list"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_reference_count_moves_with_add_refs() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  file_lifecycle::add_refs(file, 1, &db).await.expect("add ref");

  assert!(
    !is_orphan_candidate(&db, file).await,
    "the count the reaper reads is the one attach and detach move"
  );

  file_lifecycle::add_refs(file, -1, &db).await.expect("drop ref");

  assert!(
    is_orphan_candidate(&db, file).await,
    "dropping the last reference puts it back on the list"
  );
}

/// Writes the object the row stands for, through the same storage the reaper deletes through.
async fn store_object(dir: &str, name: &str) {
  SumidaStorage::new()
    .store(StaticFile {
      name:          name.to_owned(),
      original_name: "lifecycle.txt".into(),
      dir:           dir.to_owned(),
      mime_type:     "text/plain".into(),
      bytes:         b"e2e".to_vec(),
      extension:     Some("txt".into()),
    })
    .await
    .expect("store object");
}

async fn object_exists(path: &str) -> bool {
  SumidaStorage::new().get_file_size(path).await.is_ok()
}

// The reaper runs over every candidate in the table, so two of these at once would delete each
// other's rows out from under the assertions. The maintenance pass this belongs to is serialized on
// the same key.
#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_reaper_removes_both_the_object_and_the_row() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;
  let row = Files::find_by_id(file).one(&db).await.expect("read back").expect("row present");

  store_object(&row.dir, &row.name).await;
  assert!(object_exists(&row.get_path()).await, "the object is there to begin with");

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    !object_exists(&row.get_path()).await,
    "the stored object is gone, not just the row that named it"
  );
  assert!(
    Files::find_by_id(file).one(&db).await.expect("read back").is_none(),
    "and the row that pointed at it is gone too"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_missing_object_does_not_hold_the_row() {
  let db = db().await;
  // No object was ever written for this row. Storage deletion fails, and the reaper is meant to
  // carry on — otherwise the same row is collected on every pass and never leaves the table.
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    Files::find_by_id(file).one(&db).await.expect("read back").is_none(),
    "a storage failure does not stop the row from going"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_the_reaper_leaves_held_files_alone() {
  let db = db().await;
  // Everything the reaper reads says "keep" except that this file is temporary. Running the whole
  // pass — not just the query — is what shows the two halves agree.
  let held = place(&db, FileStatus::Temporary, 1, Some(Duration::hours(-1))).await;
  let row = Files::find_by_id(held).one(&db).await.expect("read back").expect("row present");

  store_object(&row.dir, &row.name).await;

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    Files::find_by_id(held).one(&db).await.expect("read back").is_some(),
    "a referenced file survives the pass"
  );
  assert!(object_exists(&row.get_path()).await, "and so does its object");
}

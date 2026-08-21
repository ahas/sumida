//! Generic file store lifecycle against a real database.
//!
//! The reaper decides what to delete from four columns at once — status, expiry, reference count,
//! and whether a junction row still points at the file. Unit-level reasoning covers each column,
//! but the query that reads them together is the thing that removes data, and nothing exercised it
//! against a database.
//!
//! The shared test DB is migrated fresh once per binary and other tests leave rows behind, so every
//! assertion here targets the file it created and never a global count.

use api::util::file_lifecycle;
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
    expires_at: Set(expires_in.map(|d| (Utc::now() + d).fixed_offset())),
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
async fn test_expired_unreferenced_temporary_file_is_a_candidate() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  assert!(
    is_orphan_candidate(&db, file).await,
    "temporary, expired and unreferenced is exactly what the reaper collects"
  );
}

#[tokio::test]
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
async fn test_a_reference_holds_the_file() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 1, Some(Duration::hours(-1))).await;

  assert!(
    !is_orphan_candidate(&db, file).await,
    "something still points at it, so expiry alone does not collect it"
  );
}

#[tokio::test]
async fn test_a_file_without_a_deadline_is_never_collected() {
  let db = db().await;
  // A null deadline is how a file says it is not on the clock. Reading it as "expired long ago"
  // would delete every such file on the first pass.
  let file = place(&db, FileStatus::Temporary, 0, None).await;

  assert!(
    !is_orphan_candidate(&db, file).await,
    "no deadline means no collection"
  );
}

#[tokio::test]
async fn test_promoted_file_leaves_the_candidate_set() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  assert!(is_orphan_candidate(&db, file).await);

  file_lifecycle::promote_active(file, &db)
    .await
    .expect("promote");

  assert!(
    !is_orphan_candidate(&db, file).await,
    "promotion is what an attach does to the file, and it takes the file off the list"
  );
}

#[tokio::test]
async fn test_reference_count_moves_with_add_refs() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  file_lifecycle::add_refs(file, 1, &db)
    .await
    .expect("add ref");

  assert!(
    !is_orphan_candidate(&db, file).await,
    "the count the reaper reads is the one attach and detach move"
  );

  file_lifecycle::add_refs(file, -1, &db)
    .await
    .expect("drop ref");

  assert!(
    is_orphan_candidate(&db, file).await,
    "dropping the last reference puts it back on the list"
  );
}

#[tokio::test]
async fn test_deleting_a_candidate_removes_the_row() {
  let db = db().await;
  let file = place(&db, FileStatus::Temporary, 0, Some(Duration::hours(-1))).await;

  // The reaper itself needs object storage, which this suite does not stand up. The row delete is
  // the half that touches the database, and it is the half that cascades a pending claim.
  Files::delete_by_id(file)
    .exec(&db)
    .await
    .expect("delete");

  assert!(
    Files::find_by_id(file)
      .one(&db)
      .await
      .expect("read back")
      .is_none(),
    "the row is gone"
  );
}

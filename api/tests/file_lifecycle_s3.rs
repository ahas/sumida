//! The orphan reaper against the S3 backend.
//!
//! `api/tests/file_lifecycle.rs` covers the query that picks candidates and the local filesystem
//! backend that deletes them. The backend is chosen at runtime from settings, so the S3 side is a
//! different implementation of the same trait reached by the same pass — and it is the one that
//! runs in a deployment that stores files in a bucket.
//!
//! This does not join the default suite. It needs a bucket, so it skips unless
//! `SUMIDA_TEST_S3_BUCKET` names one and the settings point the store at it:
//!
//! ```text
//! SUMIDA_TEST_S3_BUCKET=<bucket> SUMIDA_STORAGE__S3__BUCKET=<bucket> \
//!   cargo test --features e2e/system-settings,api/storage-aws-s3 \
//!     -p api --test file_lifecycle_s3
//! ```

#![cfg(feature = "storage-aws-s3")]

use api::service::fs::maintenance::reap_orphan_files;
use api::util::storage::{StaticFile, Storage, SumidaStorage};
use chrono::{Duration, Utc};
use e2e::prelude::*;
use entity::fs::files::FileStatus;
use entity::prelude::*;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use uuid::Uuid;

/// The bucket the run was given, or nothing — in which case every case here reports why it stopped
/// short rather than passing on an empty run.
fn bucket() -> Option<String> {
  std::env::var("SUMIDA_TEST_S3_BUCKET").ok()
}

async fn db() -> sea_orm::DatabaseConnection {
  bootstrap::init_once().await;
  // The S3 client is built from a config loaded once per process; without this the first call into
  // the store panics rather than reporting a missing bucket.
  infra::aws::load_sdk_config().await;
  infra::db::connect().await
}

/// A temporary, expired, unreferenced row — what the reaper collects — plus the object it names.
async fn place(db: &sea_orm::DatabaseConnection, with_object: bool) -> entity::fs::files::Model {
  let uuid = Uuid::new_v4();
  let name = format!("{uuid}.txt");

  let row = entity::fs::files::ActiveModel {
    uuid: Set(uuid),
    user_id: Set(None),
    original_name: Set("lifecycle.txt".into()),
    dir: Set("e2e".into()),
    name: Set(name.clone()),
    uri: Set(None),
    size: Set(3),
    mime_type: Set("text/plain".into()),
    hash: Set(None),
    tags: Set(None),
    status: Set(FileStatus::Temporary),
    refs: Set(0),
    expires_at: Set(Some(Utc::now() - Duration::hours(1))),
    ..Default::default()
  }
  .insert(db)
  .await
  .expect("place row");

  if with_object {
    SumidaStorage::new()
      .store(StaticFile {
        name,
        original_name: "lifecycle.txt".into(),
        dir: "e2e".into(),
        mime_type: "text/plain".into(),
        bytes: b"e2e".to_vec(),
        extension: Some("txt".into()),
      })
      .await
      .expect("store object");
  }

  row
}

async fn object_exists(path: &str) -> bool {
  SumidaStorage::new().get_file_size(path).await.is_ok()
}

// The reaper sweeps the whole table, so these serialize with each other and with the maintenance
// pass the same way the filesystem cases do.
#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_the_bucket_is_the_backend_this_run_uses() {
  let Some(name) = bucket() else {
    eprintln!("skipping: set SUMIDA_TEST_S3_BUCKET and SUMIDA_STORAGE__S3__BUCKET to run the S3 tests");
    return;
  };

  let db = db().await;
  let row = place(&db, true).await;

  // Without this the rest of the file would pass against the local filesystem and say nothing about
  // S3 at all — the store picks its backend from settings, and a missing setting falls back.
  assert_eq!(
    infra::settings::get().storage.s3.as_ref().map(|s| s.bucket.as_str()),
    Some(name.as_str()),
    "the store is pointed at the bucket this run was given"
  );
  assert!(object_exists(&row.get_path()).await, "and the object landed in it");

  Files::delete_by_id(row.uuid).exec(&db).await.expect("clean up");
  SumidaStorage::new().delete(&row.get_path()).await.ok();
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_reaper_removes_both_the_object_and_the_row() {
  if bucket().is_none() {
    eprintln!("skipping: SUMIDA_TEST_S3_BUCKET is not set");
    return;
  }

  let db = db().await;
  let row = place(&db, true).await;

  assert!(object_exists(&row.get_path()).await, "the object is there to begin with");

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    !object_exists(&row.get_path()).await,
    "the object in the bucket is gone, not just the row that named it"
  );
  assert!(
    Files::find_by_id(row.uuid).one(&db).await.expect("read back").is_none(),
    "and the row that pointed at it is gone too"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_a_missing_object_does_not_hold_the_row() {
  if bucket().is_none() {
    eprintln!("skipping: SUMIDA_TEST_S3_BUCKET is not set");
    return;
  }

  let db = db().await;
  // No object was ever uploaded for this row. S3 answers a delete of a key it does not hold with
  // success, so unlike the filesystem there is no error for the reaper to swallow here — which is
  // exactly why the case is worth stating: the row must go either way, and the two backends reach
  // that end differently.
  let row = place(&db, false).await;

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    Files::find_by_id(row.uuid).one(&db).await.expect("read back").is_none(),
    "the row goes whether or not there was an object behind it"
  );
}

#[tokio::test]
#[serial_test::serial(membership_sweep)]
async fn test_the_reaper_leaves_held_files_alone() {
  if bucket().is_none() {
    eprintln!("skipping: SUMIDA_TEST_S3_BUCKET is not set");
    return;
  }

  let db = db().await;
  let row = place(&db, true).await;

  // One reference is all it takes. Everything else about this file says "collect me".
  api::util::file_lifecycle::add_refs(row.uuid, 1, &db)
    .await
    .expect("add ref");

  reap_orphan_files(&db).await.expect("reap");

  assert!(
    Files::find_by_id(row.uuid).one(&db).await.expect("read back").is_some(),
    "a referenced file survives the pass"
  );
  assert!(object_exists(&row.get_path()).await, "and so does its object");

  Files::delete_by_id(row.uuid).exec(&db).await.expect("clean up");
  SumidaStorage::new().delete(&row.get_path()).await.ok();
}

//! File lifecycle primitives shared by the board services and the orphan reaper.
//!
//! `files` is a generic table; `board_files` is the junction tying a file to a board post.
//! `files.refs` counts **attached** references (rows with `post_id IS NOT NULL`), so `refs == 0`
//! means the file is not in use by any post and — once its grace TTL lapses — is reapable.
//!
//! Lifecycle:
//! - upload (board): a pending `board_files(post_id = NULL)` row is created and the file is put on a
//!   TTL (`status = temporary`, `expires_at = now + grace`). `refs` stays 0.
//! - claim (post create/update): the pending row's `post_id` is filled (or a new attached row is
//!   inserted for a generic file); `refs += 1`, `status = active`, `expires_at = NULL`.
//! - unclaim (detach / post delete): the attached row is removed; `refs -= 1`, and when it reaches 0
//!   the file is put back on the TTL so the reaper can collect it.

use chrono::{Duration, Utc};
use entity::fs::files::FileStatus;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

/// Grace window (hours) before an unattached temporary file becomes reapable.
pub const ORPHAN_GRACE_HOURS: i64 = 24;

/// `now + grace`, the TTL stamped on temporary/unattached files.
pub fn grace_expiry() -> DateTimeWithTimeZone {
  (Utc::now() + Duration::hours(ORPHAN_GRACE_HOURS)).fixed_offset()
}

/// Mark a file as temporary and put it on the orphan TTL.
pub async fn mark_pending<C: ConnectionTrait>(file_uuid: Uuid, db: &C) -> Result<(), SumidaError> {
  Files::update_many()
    .set(entity::fs::files::ActiveModel {
      status: Set(FileStatus::Temporary),
      expires_at: Set(Some(grace_expiry())),
      ..Default::default()
    })
    .filter(FilesColumn::Uuid.eq(file_uuid))
    .exec(db)
    .await?;
  Ok(())
}

/// Promote a file to active and clear its TTL (it is now referenced by a post).
pub async fn promote_active<C: ConnectionTrait>(file_uuid: Uuid, db: &C) -> Result<(), SumidaError> {
  Files::update_many()
    .set(entity::fs::files::ActiveModel {
      status: Set(FileStatus::Active),
      expires_at: Set(None),
      ..Default::default()
    })
    .filter(FilesColumn::Uuid.eq(file_uuid))
    .exec(db)
    .await?;
  Ok(())
}

/// Adjust the attached-reference count by `delta`.
pub async fn add_refs<C: ConnectionTrait>(file_uuid: Uuid, delta: i32, db: &C) -> Result<(), SumidaError> {
  Files::update_many()
    .col_expr(FilesColumn::Refs, Expr::col(FilesColumn::Refs).add(delta))
    .filter(FilesColumn::Uuid.eq(file_uuid))
    .exec(db)
    .await?;
  Ok(())
}

/// Register a freshly uploaded board file as a *pending claim* (not yet attached to a post).
pub async fn register_pending_board_file<C: ConnectionTrait>(
  file_uuid: Uuid,
  db: &C,
) -> Result<(), SumidaError> {
  BoardFiles::insert(entity::bps::board_files::ActiveModel {
    file_uuid: Set(file_uuid),
    post_id: Set(None),
  })
  .exec(db)
  .await?;
  mark_pending(file_uuid, db).await?;
  Ok(())
}

/// Attach `file_uuid` to `post_id`. Returns `true` when a new attachment was made.
/// Skips uuids with no backing file (e.g. stale references parsed from post content).
pub async fn claim_file<C: ConnectionTrait>(
  post_id: i32,
  file_uuid: Uuid,
  db: &C,
) -> Result<bool, SumidaError> {
  if Files::find_by_id(file_uuid).one(db).await?.is_none() {
    return Ok(false);
  }

  match BoardFiles::find_by_id(file_uuid).one(db).await? {
    Some(row) => match row.post_id {
      Some(pid) if pid == post_id => Ok(false), // already attached to this post
      Some(_) => Err(SumidaError::FileReuseNotAllowed),
      None => {
        // pending claim → attach to this post
        BoardFiles::update_many()
          .col_expr(BoardFilesColumn::PostId, Expr::value(post_id))
          .filter(BoardFilesColumn::FileUuid.eq(file_uuid))
          .exec(db)
          .await?;
        add_refs(file_uuid, 1, db).await?;
        promote_active(file_uuid, db).await?;
        Ok(true)
      }
    },
    None => {
      // generic file with no junction row yet → create an attached link
      BoardFiles::insert(entity::bps::board_files::ActiveModel {
        file_uuid: Set(file_uuid),
        post_id: Set(Some(post_id)),
      })
      .exec(db)
      .await?;
      add_refs(file_uuid, 1, db).await?;
      promote_active(file_uuid, db).await?;
      Ok(true)
    }
  }
}

/// Detach `file_uuid` from `post_id`. When the file's ref count hits 0 it is put back on the TTL.
pub async fn unclaim_file<C: ConnectionTrait>(
  post_id: i32,
  file_uuid: Uuid,
  db: &C,
) -> Result<(), SumidaError> {
  let res = BoardFiles::delete_many()
    .filter(BoardFilesColumn::FileUuid.eq(file_uuid))
    .filter(BoardFilesColumn::PostId.eq(post_id))
    .exec(db)
    .await?;

  if res.rows_affected > 0 {
    add_refs(file_uuid, -1, db).await?;
    if let Some(file) = Files::find_by_id(file_uuid).one(db).await?
      && file.refs <= 0
    {
      mark_pending(file_uuid, db).await?;
    }
  }

  Ok(())
}

/// Candidate orphan files: temporary, past their TTL, and not attached to any post (`refs <= 0`).
pub async fn find_orphan_files<C: ConnectionTrait>(
  limit: u64,
  db: &C,
) -> Result<Vec<entity::fs::files::Model>, SumidaError> {
  let now = Utc::now().fixed_offset();

  let files = Files::find()
    .filter(FilesColumn::Status.eq(FileStatus::Temporary))
    .filter(FilesColumn::ExpiresAt.is_not_null())
    .filter(FilesColumn::ExpiresAt.lt(now))
    .filter(FilesColumn::Refs.lte(0))
    .limit(limit)
    .all(db)
    .await?;

  Ok(files)
}

/// Defensive re-check used by the reaper: is the file attached to any post right now?
pub async fn is_attached<C: ConnectionTrait>(file_uuid: Uuid, db: &C) -> Result<bool, SumidaError> {
  let count = BoardFiles::find()
    .filter(BoardFilesColumn::FileUuid.eq(file_uuid))
    .filter(BoardFilesColumn::PostId.is_not_null())
    .count(db)
    .await?;
  Ok(count > 0)
}

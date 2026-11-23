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
use sea_orm::prelude::DateTimeUtc;
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set};
use uuid::Uuid;

/// `now + grace`, the TTL stamped on temporary/unattached files (grace from `storage.orphan_grace_hours`).
pub fn grace_expiry() -> DateTimeUtc {
  let hours = infra::settings::get().storage.orphan_grace_hours;
  Utc::now() + Duration::hours(hours)
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
#[cfg(feature = "board")]
pub async fn register_pending_board_file<C: ConnectionTrait>(
  file_uuid: Uuid,
  db: &C,
) -> Result<(), SumidaError> {
  BoardFiles::insert(entity::bps::board_files::ActiveModel {
    file_uuid: Set(file_uuid),
    post_id:   Set(None),
  })
  .exec(db)
  .await?;
  mark_pending(file_uuid, db).await?;
  Ok(())
}

/// Attach `file_uuid` to `post_id` on behalf of `uploader`. Returns the file when a new attachment
/// was made, `None` when there was nothing to do (no backing file — e.g. a stale reference parsed from
/// post content — or already attached to this post).
///
/// Only the user who uploaded a file may attach it, and only while it is attached to no post: an
/// attachment republishes the bytes under the post's visibility and hands them to the orphan reaper
/// once detached, so a file someone else owns is refused with [`SumidaError::NoAccessPermission`].
#[cfg(feature = "board")]
pub async fn claim_file<C: ConnectionTrait>(
  post_id: i32,
  file_uuid: Uuid,
  uploader: i32,
  db: &C,
) -> Result<Option<entity::fs::files::Model>, SumidaError> {
  let Some(file) = Files::find_by_id(file_uuid).one(db).await? else {
    return Ok(None);
  };

  let junction = BoardFiles::find_by_id(file_uuid).one(db).await?;
  match junction.as_ref().and_then(|row| row.post_id) {
    Some(pid) if pid == post_id => return Ok(None), // already attached to this post
    Some(_) => return Err(SumidaError::FileReuseNotAllowed),
    None => {}
  }
  if file.user_id != Some(uploader) {
    return Err(SumidaError::NoAccessPermission);
  }

  match junction {
    Some(_) => {
      // pending claim → attach to this post
      BoardFiles::update_many()
        .col_expr(BoardFilesColumn::PostId, Expr::value(post_id))
        .filter(BoardFilesColumn::FileUuid.eq(file_uuid))
        .filter(BoardFilesColumn::PostId.is_null())
        .exec(db)
        .await?;
    }
    None => {
      // generic file with no junction row yet → create an attached link
      BoardFiles::insert(entity::bps::board_files::ActiveModel {
        file_uuid: Set(file_uuid),
        post_id:   Set(Some(post_id)),
      })
      .exec(db)
      .await?;
    }
  }
  add_refs(file_uuid, 1, db).await?;
  promote_active(file_uuid, db).await?;
  Ok(Some(file))
}

/// Whether `uploader` uploaded the file under `file_uuid`.
#[cfg(feature = "board")]
pub async fn is_uploaded_by<C: ConnectionTrait>(
  file_uuid: Uuid,
  uploader: i32,
  db: &C,
) -> Result<bool, SumidaError> {
  Ok(
    Files::find_by_id(file_uuid)
      .one(db)
      .await?
      .is_some_and(|file| file.user_id == Some(uploader)),
  )
}

/// Detach `file_uuid` from `post_id`. When the file's ref count hits 0 it is put back on the TTL.
#[cfg(feature = "board")]
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
  let now = Utc::now();

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
/// board 기능이 꺼지면 board_files 정션이 없으므로 항상 미첨부(false)다.
#[cfg(feature = "board")]
pub async fn is_attached<C: ConnectionTrait>(file_uuid: Uuid, db: &C) -> Result<bool, SumidaError> {
  let count = BoardFiles::find()
    .filter(BoardFilesColumn::FileUuid.eq(file_uuid))
    .filter(BoardFilesColumn::PostId.is_not_null())
    .count(db)
    .await?;
  Ok(count > 0)
}

#[cfg(not(feature = "board"))]
pub async fn is_attached<C: ConnectionTrait>(_file_uuid: Uuid, _db: &C) -> Result<bool, SumidaError> {
  Ok(false)
}

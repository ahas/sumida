//! Background cleanup for the generic `files` store.
//!
//! Files are uploaded before they are attached to anything (e.g. tiptap pre-uploads images, then the
//! post is created). An upload that is never attached would otherwise linger forever.
//! [`reap_orphan_files`] removes files that are still temporary, past their TTL, and unreferenced
//! (`refs <= 0`), deleting both the storage object and the row (which cascades any pending
//! `board_files` claim).
//!
//! Invoked from the shared maintenance pass ([`crate::service::membership::maintenance::run_once`]),
//! which already holds a single-runner advisory lock, so this function is lock-free itself.

use entity::prelude::*;
use error::SumidaError;
use sea_orm::{DatabaseConnection, EntityTrait};

use crate::util::file_lifecycle;
use crate::util::storage::{Storage, SumidaStorage};

/// Max files to collect per pass.
const BATCH: u64 = 500;

/// Remove orphaned (unattached, expired, temporary) files. Returns how many were deleted.
pub async fn reap_orphan_files(db: &DatabaseConnection) -> Result<usize, SumidaError> {
  let candidates = file_lifecycle::find_orphan_files(BATCH, db).await?;
  if candidates.is_empty() {
    return Ok(0);
  }

  let storage = SumidaStorage::new();
  let mut removed = 0usize;

  for file in candidates {
    // Defensive re-check: a concurrent claim may have attached the file since it was selected.
    if file_lifecycle::is_attached(file.uuid, db).await? {
      continue;
    }

    // Best-effort storage removal; proceed with the row delete regardless so we don't loop on it.
    if let Err(e) = storage.delete(&file.get_path()).await {
      tracing::warn!("orphan reaper: storage delete failed for {}: {e}", file.uuid);
    }

    Files::delete_by_id(file.uuid).exec(db).await?;
    removed += 1;
  }

  if removed > 0 {
    tracing::debug!("orphan reaper: removed {removed} unattached file(s)");
  }

  Ok(removed)
}

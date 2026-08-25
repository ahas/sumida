//! Background maintenance for the board (bps) domain.
//!
//! Scheduled posts are published lazily nowhere else, so without this a `scheduled` post whose
//! `scheduled_at` has passed would never go live. Run from the shared maintenance pass
//! ([`crate::service::membership::maintenance::run_once`]), which holds a single-runner advisory lock.

use chrono::Utc;
use entity::bps::board_posts::BoardPostStatus;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

/// Publish scheduled posts whose `scheduled_at` has arrived. Returns how many were published.
pub async fn publish_scheduled_posts(db: &DatabaseConnection) -> Result<usize, SumidaError> {
  let now = Utc::now();

  let result = BoardPosts::update_many()
    .set(entity::bps::board_posts::ActiveModel {
      status: Set(BoardPostStatus::Published),
      published_at: Set(Some(now)),
      ..Default::default()
    })
    .filter(BoardPostsColumn::Status.eq(BoardPostStatus::Scheduled))
    .filter(BoardPostsColumn::ScheduledAt.lte(now))
    .exec(db)
    .await?;

  let published = result.rows_affected as usize;
  if published > 0 {
    tracing::debug!("board maintenance: published {published} scheduled post(s)");
  }

  Ok(published)
}

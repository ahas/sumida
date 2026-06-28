pub mod helpers {
  use entity::prelude::*;
  use error::SumidaError;
  use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
  use uuid::Uuid;

  /// Extract board_id from board_post
  pub async fn get_board_id_from_post<C>(post_id: i32, db: &C) -> Result<i32, SumidaError>
  where
    C: ConnectionTrait,
  {
    let post = BoardPosts::find_by_id(post_id)
      .one(db)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), post_id.to_string()))?;

    Ok(post.board_id)
  }

  /// Extract board_id from post that has this file attached
  pub async fn get_board_id_from_file<C>(file_uuid: Uuid, db: &C) -> Result<Option<i32>, SumidaError>
  where
    C: ConnectionTrait,
  {
    use entity::bps::board_files::Column;

    let board_file = BoardFiles::find().filter(Column::FileUuid.eq(file_uuid)).one(db).await?;

    // `post_id` is None for a pending claim (uploaded, not yet attached to a post) — no board to resolve.
    if let Some(post_id) = board_file.and_then(|bf| bf.post_id) {
      return Ok(Some(get_board_id_from_post(post_id, db).await?));
    }

    Ok(None)
  }
}

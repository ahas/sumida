pub mod helpers {
  use entity::prelude::*;
  use error::SumidaError;
  use sea_orm::{ConnectionTrait, EntityTrait};

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

  /// Extract board_id from board_file
  pub async fn get_board_id_from_file<C>(file_id: i32, db: &C) -> Result<Option<i32>, SumidaError>
  where
    C: ConnectionTrait,
  {
    let file = BoardFiles::find_by_id(file_id)
      .one(db)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), file_id.to_string()))?;

    // BoardFile can be associated with board directly, or through post/comment
    if let Some(board_id) = file.board_id {
      return Ok(Some(board_id));
    }

    if let Some(post_id) = file.post_id {
      return Ok(Some(get_board_id_from_post(post_id, db).await?));
    }

    Ok(None)
  }
}

/// Board-specific permission helper functions
pub mod helpers {
  use crate::util::ap::{self, ApGuard};
  use infra::session::UserSession;
  use entity::prelude::*;
  use error::SumidaError;
  use sea_orm::{EntityTrait, ConnectionTrait};

  /// Check if user can read board
  pub async fn can_read_board<C>(session: &UserSession, board_id: i32, db: &C) -> bool
  where
    C: sea_orm::ConnectionTrait,
  {
    if session.profile.is_admin {
      return true;
    }

    let condition = ap::ns("board").object(format!("board:{}", board_id)).action("read").who(session);

    condition.check_allowed(db).await
  }

  /// Check if user can write to board
  pub async fn can_write_board<C>(session: &UserSession, board_id: i32, db: &C) -> bool
  where
    C: sea_orm::ConnectionTrait,
  {
    if session.profile.is_admin {
      return true;
    }

    let condition = ap::ns("board").object(format!("board:{}", board_id)).action("write").who(session);

    condition.check_allowed(db).await
  }

  /// Check if user can moderate board
  pub async fn can_moderate_board<C>(session: &UserSession, board_id: i32, db: &C) -> bool
  where
    C: sea_orm::ConnectionTrait,
  {
    if session.profile.is_admin {
      return true;
    }

    let condition = ap::ns("board").object(format!("board:{}", board_id)).action("moderate").who(session);

    condition.check_allowed(db).await
  }

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

  /// Extract board_id from board_comment via post
  pub async fn get_board_id_from_comment<C>(comment_id: i32, db: &C) -> Result<i32, SumidaError>
  where
    C: ConnectionTrait,
  {
    let comment = BoardComments::find_by_id(comment_id)
      .one(db)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardComment".into(), comment_id.to_string()))?;
    
    get_board_id_from_post(comment.post_id, db).await
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
    
    if let Some(comment_id) = file.comment_id {
      return Ok(Some(get_board_id_from_comment(comment_id, db).await?));
    }
    
    Ok(None)
  }

  /// Check hierarchical read permission (board -> post -> comment/file)
  pub async fn check_hierarchical_read_permission<C>(
    session: &UserSession,
    board_id: i32,
    action: &str,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    // First check board-level permission
    if !can_read_board(session, board_id, db).await {
      return Err(SumidaError::NoAccessPermission);
    }

    // Then check specific action permission if it's not just "read"
    if action != "read" {
      let can_perform_action = match action {
        "write" => can_write_board(session, board_id, db).await,
        "moderate" => can_moderate_board(session, board_id, db).await,
        _ => can_read_board(session, board_id, db).await,
      };

      if !can_perform_action {
        return Err(SumidaError::NoAccessPermission);
      }
    }

    Ok(())
  }
}

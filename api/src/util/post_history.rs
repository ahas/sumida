use chrono::Utc;
use entity::bs::{board_posts, board_post_histories, boards};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ActiveModelBehavior, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, 
  EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};


#[derive(Debug, Clone)]
pub struct PostHistoryConfig {
  pub enable_post_history: bool,
  pub max_history_count: i32,
}

impl From<&boards::Model> for PostHistoryConfig {
  fn from(board: &boards::Model) -> Self {
    Self {
      enable_post_history: board.enable_post_history,
      max_history_count: board.max_history_count,
    }
  }
}

pub struct PostHistoryManager {
  config: PostHistoryConfig,
}

impl PostHistoryManager {
  pub fn new(config: PostHistoryConfig) -> Self {
    Self { config }
  }

  pub async fn from_board_id<C>(board_id: i32, db: &C) -> Result<Self, SumidaError>
  where
    C: ConnectionTrait,
  {
    let board = boards::Entity::find_by_id(board_id)
      .one(db)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), board_id.to_string()))?;

    Ok(Self::new(PostHistoryConfig::from(&board)))
  }


  /// Auto-save draft content
  pub async fn auto_save<C>(&self, post_id: i32, content: &str, db: &C) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    if !self.config.enable_post_history {
      return Ok(());
    }

    let mut post_model = board_posts::ActiveModel::new();
    post_model.id = Set(post_id);
    post_model.auto_save_content = Set(Some(content.to_string()));
    post_model.last_auto_save_at = Set(Some(Utc::now().fixed_offset()));

    post_model.update(db).await?;

    Ok(())
  }


  /// Update post status
  pub async fn update_post_status<C>(
    &self,
    post_id: i32,
    status: board_posts::PostStatus,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    let mut post_model = board_posts::ActiveModel::new();
    post_model.id = Set(post_id);
    post_model.status = Set(String::from(status.clone()));

    // Set published_at when status changes to published
    if status == board_posts::PostStatus::Published {
      post_model.published_at = Set(Some(Utc::now().fixed_offset()));
      // Clear auto-save when publishing
      post_model.auto_save_content = Set(None);
      post_model.last_auto_save_at = Set(None);
    }

    post_model.update(db).await?;

    Ok(())
  }

  /// Schedule post for future publication
  pub async fn schedule_post<C>(
    &self,
    post_id: i32,
    scheduled_at: chrono::DateTime<chrono::FixedOffset>,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    let mut post_model = board_posts::ActiveModel::new();
    post_model.id = Set(post_id);
    post_model.status = Set(String::from(board_posts::PostStatus::Scheduled));
    post_model.scheduled_at = Set(Some(scheduled_at));

    post_model.update(db).await?;

    Ok(())
  }


  pub fn get_config(&self) -> &PostHistoryConfig {
    &self.config
  }

  /// Validate if history operations are enabled
  pub fn is_history_enabled(&self) -> bool {
    self.config.enable_post_history
  }

  /// Save post content to history before update
  pub async fn save_history<C>(
    &self,
    post_id: i32,
    content: &str,
    modified_by: i32,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    if !self.config.enable_post_history {
      return Ok(());
    }

    // Save current content to history
    let history_model = board_post_histories::ActiveModel {
      post_id: Set(post_id),
      content: Set(content.to_string()),
      modified_by: Set(modified_by),
      modified_at: Set(Utc::now().fixed_offset()),
      ..Default::default()
    };

    BoardPostHistories::insert(history_model).exec(db).await?;

    // Cleanup old history if over limit
    self.cleanup_old_history(post_id, db).await?;

    Ok(())
  }

  /// Get post history
  pub async fn get_history<C>(
    &self,
    post_id: i32,
    db: &C,
  ) -> Result<Vec<board_post_histories::Model>, SumidaError>
  where
    C: ConnectionTrait,
  {
    let histories = BoardPostHistories::find()
      .filter(board_post_histories::Column::PostId.eq(post_id))
      .order_by_desc(board_post_histories::Column::ModifiedAt)
      .all(db)
      .await?;

    Ok(histories)
  }

  /// Get specific history version
  pub async fn get_history_version<C>(
    &self,
    post_id: i32,
    history_id: i32,
    db: &C,
  ) -> Result<Option<board_post_histories::Model>, SumidaError>
  where
    C: ConnectionTrait,
  {
    let history = BoardPostHistories::find()
      .filter(board_post_histories::Column::PostId.eq(post_id))
      .filter(board_post_histories::Column::Id.eq(history_id))
      .one(db)
      .await?;

    Ok(history)
  }

  /// Restore post to specific history version
  pub async fn restore_to_version<C>(
    &self,
    post_id: i32,
    history_id: i32,
    modified_by: i32,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait + TransactionTrait,
  {
    if !self.config.enable_post_history {
      return Err(SumidaError::BadRequest("Post history is disabled".into()));
    }

    let txn = db.begin().await?;

    // Get the history version
    let history = self.get_history_version(post_id, history_id, &txn).await?
      .ok_or_else(|| SumidaError::NoData("History".into(), history_id.to_string()))?;

    // Get current post content to save as history before restore
    let current_post = BoardPosts::find_by_id(post_id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), post_id.to_string()))?;

    // Save current content as history
    if let Some(current_content) = &current_post.content {
      self.save_history(post_id, &current_content.to_string(), modified_by, &txn).await?;
    }

    // Restore to history version
    let mut post_model = board_posts::ActiveModel::new();
    post_model.id = Set(post_id);
    post_model.content = Set(Some(
      serde_json::from_str(&history.content)
        .map_err(|e| SumidaError::BadRequest(format!("Invalid JSON in history: {}", e)))?
    ));
    post_model.updated_at = Set(Utc::now().fixed_offset());

    post_model.update(&txn).await?;

    txn.commit().await?;
    
    Ok(())
  }

  /// Update post content with history tracking
  pub async fn update_with_history<C>(
    &self,
    post_id: i32,
    new_content: &str,
    session: &UserSession,
    db: &C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait + TransactionTrait,
  {
    let txn = db.begin().await?;

    // Get current content to save as history
    if self.config.enable_post_history {
      let current_post = BoardPosts::find_by_id(post_id)
        .one(&txn)
        .await?
        .ok_or_else(|| SumidaError::NoData("BoardPost".into(), post_id.to_string()))?;

      // Save current content as history
      if let Some(current_content) = &current_post.content {
        self.save_history(post_id, &current_content.to_string(), session.profile.id, &txn).await?;
      }
    }

    // Update post with new content
    let mut post_model = board_posts::ActiveModel::new();
    post_model.id = Set(post_id);
    post_model.content = Set(Some(
      serde_json::from_str(new_content)
        .map_err(|e| SumidaError::BadRequest(format!("Invalid JSON content: {}", e)))?
    ));
    post_model.updated_at = Set(Utc::now().fixed_offset());

    post_model.update(&txn).await?;

    txn.commit().await?;

    Ok(())
  }

  /// Clean up old history beyond max count
  async fn cleanup_old_history<C>(&self, post_id: i32, db: &C) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    if self.config.max_history_count <= 0 {
      return Ok(());
    }

    // Get count of current histories
    let count = BoardPostHistories::find()
      .filter(board_post_histories::Column::PostId.eq(post_id))
      .count(db)
      .await? as i32;

    // If over limit, delete oldest entries
    if count > self.config.max_history_count {
      let excess_count = count - self.config.max_history_count;
      
      // Get IDs of oldest entries to delete
      let old_histories = BoardPostHistories::find()
        .filter(board_post_histories::Column::PostId.eq(post_id))
        .order_by_asc(board_post_histories::Column::ModifiedAt)
        .limit(excess_count as u64)
        .all(db)
        .await?;

      // Delete old entries
      for history in old_histories {
        BoardPostHistories::delete_by_id(history.id).exec(db).await?;
      }
    }

    Ok(())
  }

  /// Delete all history for a post
  pub async fn delete_post_history<C>(&self, post_id: i32, db: &C) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    BoardPostHistories::delete_many()
      .filter(board_post_histories::Column::PostId.eq(post_id))
      .exec(db)
      .await?;

    Ok(())
  }
}



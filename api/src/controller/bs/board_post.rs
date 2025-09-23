use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::util::post_history::PostHistoryManager;
use crate::{SumidaState, service_ap, user_guard};
use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use entity::bs::board_post_histories;
use error::SumidaError;
use infra::session::UserSession;

#[derive(Clone, Debug, Default)]
pub struct BoardPostController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardPostController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardPostFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    Ok(BoardPostService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardPost, SumidaError> {
    BoardPostService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardPost>,
  ) -> Result<BoardPostCreated, SumidaError> {
    let res = BoardPostService::new(state, session).create(body).await?;
    Ok(BoardPostCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardPost>,
  ) -> Result<BoardPostUpdated, SumidaError> {
    BoardPostService::new(state, session).update_with_permission(id, body).await?;
    Ok(BoardPostUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardPostDeleted, SumidaError> {
    BoardPostService::new(state, session).delete(id).await?;
    Ok(BoardPostDeleted)
  }

  // Auto-save functionality
  async fn auto_save(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<AutoSavePost>,
  ) -> Result<AutoSaveResult, SumidaError> {
    // Verify access to the post first
    let post = BoardPostService::new(state.clone(), session.clone())
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    // Auto-save content
    let history_manager = PostHistoryManager::from_board_id(post.board_id, state.db()).await?;
    history_manager.auto_save(id, &body.content, state.db()).await?;

    Ok(AutoSaveResult { success: true })
  }

  // Publish post
  async fn publish(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<PublishResult, SumidaError> {
    // Verify access to the post first
    let post = BoardPostService::new(state.clone(), session.clone())
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    // Publish post
    let history_manager = PostHistoryManager::from_board_id(post.board_id, state.db()).await?;
    history_manager
      .update_post_status(id, entity::bs::board_posts::PostStatus::Published, state.db())
      .await?;

    Ok(PublishResult { success: true })
  }

  // Schedule post
  async fn schedule(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<SchedulePost>,
  ) -> Result<ScheduleResult, SumidaError> {
    // Verify access to the post first
    let post = BoardPostService::new(state.clone(), session.clone())
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    // Schedule post
    let history_manager = PostHistoryManager::from_board_id(post.board_id, state.db()).await?;
    history_manager.schedule_post(id, body.scheduled_at, state.db()).await?;

    Ok(ScheduleResult { success: true })
  }

  // Get post history
  async fn get_history(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<Json<Vec<board_post_histories::Model>>, SumidaError> {
    // Verify access to the post first
    let post = BoardPostService::new(state.clone(), session.clone())
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    let history_manager = PostHistoryManager::from_board_id(post.board_id, state.db()).await?;
    let histories = history_manager.get_history(id, state.db()).await?;

    Ok(Json(histories))
  }

  // Restore post to specific version
  async fn restore_version(
    session: UserSession,
    State(state): State<S>,
    Path((id, history_id)): Path<(i32, i32)>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    // Verify access to the post first
    let post = BoardPostService::new(state.clone(), session.clone())
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    let history_manager = PostHistoryManager::from_board_id(post.board_id, state.db()).await?;
    history_manager.restore_to_version(id, history_id, session.profile.id, state.db()).await?;

    Ok(Json(serde_json::json!({ "success": true })))
  }
}

impl<S> ApiController<S> for BoardPostController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    // Read operations - basic authentication required
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_post", read));

    // Write operations - board-level permissions checked in handlers
    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board_post", create));

    // Update operations - ownership/moderate permissions checked in handlers
    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board_post", update));

    // Delete operations - ownership/moderate permissions checked in handlers
    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_post", delete));

    // Post management operations - ownership/moderate permissions checked in handlers
    let management = Router::new()//
      .route("/{id}/auto-save", routing::post(Self::auto_save))
      .route("/{id}/publish", routing::post(Self::publish))
      .route("/{id}/schedule", routing::post(Self::schedule))
      // TODO: Implement history endpoints
      // .route("/{id}/history", routing::get(Self::get_history))
      // .route("/{id}/history/{history_id}/restore", routing::post(Self::restore_version))
      .route_layer(service_ap!(state, "board_post", update));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .merge(management)
      .route_layer(user_guard!(state))
  }
}

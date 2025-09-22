use crate::controller::ApiController;
use crate::service::bs::board_file::BoardFileService;
use crate::service::bs::board_file::dto::*;
use crate::util::pagination::{List, Pagination};
use crate::util::file_validation::{FileValidator, FileInfo};
use crate::layer::board_ap::helpers as board_helpers;
use crate::{SumidaState, service_ap, user_guard};
use sea_orm::EntityTrait;
use axum::{
  Json, Router,
  body::Body,
  extract::{Path, Query, State},
  http::{StatusCode, header},
  response::Response,
  routing,
};
use error::SumidaError;
use infra::session::UserSession;
use std::io::SeekFrom;
use tokio::fs::File;
use tokio::io::AsyncSeekExt;
use tokio_util::io::ReaderStream;

#[derive(Clone, Debug, Default)]
pub struct BoardFileController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardFileController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardFileFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardFileListItem>, SumidaError> {
    Ok(BoardFileService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardFile, SumidaError> {
    BoardFileService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardFile>,
  ) -> Result<BoardFileCreated, SumidaError> {
    // Validate file if board_id is provided
    if let Some(board_id) = body.board_id.0 {
      let validator = FileValidator::from_board_id(board_id, state.db()).await?;
      
      let file_info = FileInfo::new(
        body.original_filename.clone(),
        body.file_size,
        body.mime_type.clone(),
      );
      
      validator.validate_file(&file_info)?;
    }
    
    let res = BoardFileService::new(state, session).create(body).await?;
    Ok(BoardFileCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardFile>,
  ) -> Result<BoardFileUpdated, SumidaError> {
    BoardFileService::new(state, session).update(id, body).await?;
    Ok(BoardFileUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardFileDeleted, SumidaError> {
    BoardFileService::new(state, session).delete(id).await?;
    Ok(BoardFileDeleted)
  }

  async fn get_stats(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardFileFilter>,
  ) -> Result<Json<BoardFileStats>, SumidaError> {
    let stats = BoardFileService::new(state, session).get_stats(filter).await?;
    Ok(Json(stats))
  }

  async fn download(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<Response<Body>, SumidaError> {
    let service = BoardFileService::new(state.clone(), session.clone());
    
    // Get file info and check board-specific permissions
    let file = service
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), id.to_string()))?;
    
    // Check board visibility if file is attached to a board
    if let Some(board_id) = file.board_id {
      use entity::bs::boards;
      
      let _board = boards::Entity::find_by_id(board_id)
        .one(state.db())
        .await?
        .ok_or_else(|| SumidaError::NoData("Board".into(), board_id.to_string()))?;
      
      // Use board_helpers to check if user can read the board
      if !board_helpers::can_read_board(&session, board_id, state.db()).await {
        return Err(SumidaError::NoAccessPermission);
      }
    }
    
    let download_info = service
      .get_download_info(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), id.to_string()))?;

    // Check if file exists
    let mut file = File::open(&download_info.file_path)
      .await
      .map_err(|_| SumidaError::NoData("File".into(), download_info.file_path.clone()))?;

    // Get file size
    let file_size = file
      .seek(SeekFrom::End(0))
      .await
      .map_err(|_| SumidaError::InternalServerError("Failed to get file size".into()))?;

    file
      .seek(SeekFrom::Start(0))
      .await
      .map_err(|_| SumidaError::InternalServerError("Failed to reset file position".into()))?;

    // Create file stream
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    // Create response with appropriate headers
    let response = Response::builder()
      .status(StatusCode::OK)
      .header(header::CONTENT_TYPE, download_info.content_type)
      .header(header::CONTENT_LENGTH, file_size.to_string())
      .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", download_info.filename))
      .body(body)
      .map_err(|_| SumidaError::InternalServerError("Failed to create response".into()))?;

    Ok(response)
  }

  async fn mark_completed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<MarkCompleted>,
  ) -> Result<BoardFileUpdated, SumidaError> {
    BoardFileService::new(state, session).mark_as_completed(id, body.file_url, body.file_hash).await?;
    Ok(BoardFileUpdated)
  }

  async fn mark_failed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardFileUpdated, SumidaError> {
    BoardFileService::new(state, session).mark_as_failed(id).await?;
    Ok(BoardFileUpdated)
  }

  async fn cleanup_expired(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<CleanupResult>, SumidaError> {
    let deleted_count = BoardFileService::new(state, session).cleanup_expired().await?;
    Ok(Json(CleanupResult { deleted_count }))
  }

  async fn cleanup_failed(
    session: UserSession,
    State(state): State<S>,
    Query(params): Query<CleanupFailedParams>,
  ) -> Result<Json<CleanupResult>, SumidaError> {
    let older_than_hours = params.older_than_hours.unwrap_or(24);
    let deleted_count = BoardFileService::new(state, session).cleanup_failed(older_than_hours).await?;
    Ok(Json(CleanupResult { deleted_count }))
  }
}


impl<S> ApiController<S> for BoardFileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route("/stats", routing::get(Self::get_stats))
      .route("/{id}/download", routing::get(Self::download))
      .route_layer(service_ap!(state, "board_files", read));

    let create = Router::new()
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board_files", create));

    let update = Router::new()
      .route("/{id}", routing::patch(Self::update))
      .route("/{id}/complete", routing::post(Self::mark_completed))
      .route("/{id}/fail", routing::post(Self::mark_failed))
      .route_layer(service_ap!(state, "board_files", update));

    let delete = Router::new()
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_files", delete));

    let admin = Router::new()
      .route("/cleanup/expired", routing::post(Self::cleanup_expired))
      .route("/cleanup/failed", routing::post(Self::cleanup_failed))
      .route_layer(service_ap!(state, "board_files", admin));

    Router::new()
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .merge(admin)
      .route_layer(user_guard!(state))
  }
}


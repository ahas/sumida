use axum::{
  Json, Router,
  body::Body,
  extract::{FromRequest, Multipart, Path, Query, Request, State},
  http::{StatusCode, header},
  response::Response,
  routing,
};
use error::SumidaError;
use infra::session::UserSession;
use sea_orm_crud::Optional;
use tokio_util::io::ReaderStream;

use crate::controller::ApiController;
use crate::service::bs::board_file::BoardFileService;
use crate::service::bs::board_file::dto::*;
use crate::util::file_validation::{FileInfo, FileValidator};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Debug, Default)]
struct MultipartFormData {
  metadata: Option<CreateBoardFile>,
  file_data: Option<(String, Vec<u8>, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct BoardFileController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardFileController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }

  async fn parse_multipart_form(multipart: &mut Multipart) -> Result<MultipartFormData, SumidaError> {
    let mut form_data = MultipartFormData::default();

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|e| SumidaError::BadRequest(format!("Multipart error: {}", e)))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
        "metadata" => {
          let text =
            field.text().await.map_err(|e| SumidaError::BadRequest(format!("Invalid metadata: {}", e)))?;
          form_data.metadata = Some(
            serde_json::from_str(&text)
              .map_err(|e| SumidaError::BadRequest(format!("Invalid metadata JSON: {}", e)))?,
          );
        }
        "file" => {
          let filename = field.file_name().unwrap_or("unknown").to_string();
          let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
          let data = field
            .bytes()
            .await
            .map_err(|e| SumidaError::BadRequest(format!("Failed to read file: {}", e)))?;

          form_data.file_data = Some((filename, data.to_vec(), content_type));
        }
        _ => {
          // Skip unknown fields
        }
      }
    }

    Ok(form_data)
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
    request: Request,
  ) -> Result<BoardFileCreated, SumidaError> {
    use crate::util::storage::create_storage;
    use std::path::Path;
    use uuid::Uuid;

    // Detect content type
    let content_type =
      request.headers().get(header::CONTENT_TYPE).and_then(|ct| ct.to_str().ok()).unwrap_or("");

    let (board_id, post_id, file_data, metadata_only) = if content_type.starts_with("multipart/form-data") {
      // Handle multipart upload
      let mut multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid multipart data".into()))?;

      let form_data = Self::parse_multipart_form(&mut multipart).await?;
      let metadata =
        form_data.metadata.ok_or_else(|| SumidaError::BadRequest("metadata is required".into()))?;
      let board_id =
        metadata.board_id.0.ok_or_else(|| SumidaError::BadRequest("board_id is required".into()))?;
      (board_id, metadata.post_id.0, form_data.file_data, false)
    } else {
      // Handle JSON metadata
      let json = Json::<CreateBoardFile>::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid JSON data".into()))?;
      let body = json.0;
      let board_id =
        body.board_id.0.ok_or_else(|| SumidaError::BadRequest("board_id is required".into()))?;
      (board_id, body.post_id.0, None, true)
    };

    let create_dto = if let Some((original_filename, file_bytes, mime_type)) = file_data {
      // File upload case
      // Validate file
      let validator = FileValidator::from_board_id(board_id, state.db()).await?;
      let file_info = FileInfo::new(original_filename.clone(), file_bytes.len() as i64, mime_type.clone());
      validator.validate_file(&file_info)?;

      // Generate unique filename and store file
      let file_extension =
        Path::new(&original_filename).extension().and_then(|ext| ext.to_str()).unwrap_or("");

      let unique_filename = format!("{}.{}", Uuid::new_v4(), file_extension);

      // Store file using storage abstraction
      let storage = create_storage();
      let file_path = storage.store(&file_bytes, &unique_filename).await?;
      let file_url = storage.get_url(&file_path);

      CreateBoardFile {
        board_id: Optional(Some(board_id)),
        post_id: Optional(post_id),
        user_id: Optional(Some(session.profile.id)),
        original_filename,
        file_path,
        file_url: Optional(Some(file_url)),
        file_size: file_bytes.len() as i64,
        mime_type,
        file_hash: Optional(None), // Could add hash calculation later
        upload_status: "completed".to_string(),
        is_temporary: Optional(Some(false)),
        expires_at: Optional(None),
      }
    } else if metadata_only {
      return Err(SumidaError::BadRequest("Metadata-only creation not supported via this endpoint".into()));
    } else {
      return Err(SumidaError::BadRequest("file is required".into()));
    };

    let res = BoardFileService::new(state, session).create(create_dto).await?;
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
    let download_info = service
      .get_download_info(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), id.to_string()))?;

    // Use storage abstraction for download
    use crate::util::storage::create_storage;

    let storage = create_storage();
    let file_stream = storage.download(&download_info.file_path).await?;
    let file_size = storage.get_file_size(&download_info.file_path).await?;

    // Create file stream
    let stream = ReaderStream::new(file_stream);
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

    let create = Router::new().route("/", routing::post(Self::create)).route_layer(service_ap!(
      state,
      "board_files",
      create
    ));

    let update = Router::new()
      .route("/{id}", routing::patch(Self::update))
      .route("/{id}/complete", routing::post(Self::mark_completed))
      .route("/{id}/fail", routing::post(Self::mark_failed))
      .route_layer(service_ap!(state, "board_files", update));

    let delete = Router::new().route("/{id}", routing::delete(Self::delete)).route_layer(service_ap!(
      state,
      "board_files",
      delete
    ));

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

use axum::body::Body;
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::header;
use axum::response::Response;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;

use crate::controller::ApiController;
use crate::service::fs::dto::*;
use crate::service::fs::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::util::storage::{Storage, SumidaStorage};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct FileController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> FileController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn parse_multipart_form(multipart: &mut Multipart) -> Result<UploadFile, SumidaError> {
    let mut file_data: Option<UploadFileData> = None;
    let mut metadata: Option<CreateFile> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|e| SumidaError::BadRequest(format!("Multipart error: {}", e)))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
        "metadata" => {
          let text = field
            .text()
            .await
            .map_err(|e| SumidaError::BadRequest(format!("Invalid metadata: {}", e)))?;
          metadata = Some(
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

          file_data = Some(UploadFileData {
            original_name: filename,
            bytes:         data.to_vec(),
            mime_type:     content_type,
          });
        }
        _ => {}
      }
    }

    Ok(UploadFile {
      file_data: file_data.unwrap(),
      metadata:  metadata.unwrap(),
    })
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<FileFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<FileListItem>, SumidaError> {
    Ok(FileService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetFile, SumidaError> {
    FileService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("File".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    request: Request,
  ) -> Result<FileCreated, SumidaError> {
    let content_type = request
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|ct| ct.to_str().ok())
      .unwrap_or("");
    let form_data = if content_type.starts_with("multipart/form-data") {
      let mut multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid multipart data".into()))?;

      Self::parse_multipart_form(&mut multipart).await?
    } else {
      return Err(SumidaError::BadRequest("Only multipart/form-data is supported for file uploads".into()));
    };

    let result = FileService::new(state, session).create(form_data.file_data, form_data.metadata).await?;

    Ok(FileCreated {
      id: result.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateFile>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session).update(id, body).await?;
    Ok(FileUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<FileDeleted, SumidaError> {
    FileService::new(state, session).delete(id).await?;

    Ok(FileDeleted)
  }

  async fn get_stats(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<FileFilter>,
  ) -> Result<Json<FileStats>, SumidaError> {
    let stats = FileService::new(state, session).get_stats(filter).await?;
    Ok(Json(stats))
  }

  async fn download(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<Response<Body>, SumidaError> {
    let service = FileService::new(state.clone(), session.clone());
    let download_info = service
      .get_download_info(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("File".into(), id.to_string()))?;

    let storage = SumidaStorage::new();
    let response = storage
      .download(&download_info.name, &download_info.path, &download_info.content_type)
      .await?;

    Ok(response)
  }

  async fn mark_completed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<MarkFileCompleted>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session).mark_as_completed(id, body.uri, body.hash).await?;
    Ok(FileUpdated)
  }

  async fn mark_failed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session).mark_as_failed(id).await?;

    Ok(FileUpdated)
  }

  async fn cleanup_expired(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<CleanupResult>, SumidaError> {
    let deleted_count = FileService::new(state, session).cleanup_expired().await?;

    Ok(Json(CleanupResult { deleted_count }))
  }

  async fn cleanup_failed(
    session: UserSession,
    State(state): State<S>,
    Query(params): Query<CleanupFailedParams>,
  ) -> Result<Json<CleanupResult>, SumidaError> {
    let older_than_hours = params.older_than_hours.unwrap_or(24);
    let deleted_count = FileService::new(state, session).cleanup_failed(older_than_hours).await?;

    Ok(Json(CleanupResult { deleted_count }))
  }
}

impl<S> ApiController<S> for FileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route("/stats", routing::get(Self::get_stats))
      .route("/{id}/download", routing::get(Self::download))
      .route_layer(service_ap!(state, "file", read));

    let route_create = Router::new()
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "file", create));

    let route_update = Router::new()
      .route("/{id}", routing::patch(Self::update))
      .route("/{id}/complete", routing::post(Self::mark_completed))
      .route("/{id}/fail", routing::post(Self::mark_failed))
      .route_layer(service_ap!(state, "file", update));

    let route_delete = Router::new()
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "file", delete));

    let route_admin = Router::new()
      .route("/cleanup/expired", routing::post(Self::cleanup_expired))
      .route("/cleanup/failed", routing::post(Self::cleanup_failed))
      .route_layer(service_ap!(state, "file", admin));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .merge(route_admin)
      .route_layer(user_guard!(state))
  }
}

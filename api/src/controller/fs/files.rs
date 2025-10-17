use axum::body::Body;
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use sea_orm_crud::{Nullable, Optional};
use tokio_util::io::ReaderStream;

use crate::controller::ApiController;
use crate::service::fs::dto::*;
use crate::service::fs::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Debug, Default)]
struct MultipartFormData {
  metadata:  Option<FileUploadMeta>,
  file_data: Option<(String, Vec<u8>, String)>,
}

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
          let text = field
            .text()
            .await
            .map_err(|e| SumidaError::BadRequest(format!("Invalid metadata: {}", e)))?;
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
        _ => {}
      }
    }

    Ok(form_data)
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
    Path(key): Path<String>,
  ) -> Result<GetFile, SumidaError> {
    FileService::new(state, session)
      .get(&key)
      .await?
      .ok_or_else(|| SumidaError::NoData("File".to_owned(), key.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    request: Request,
  ) -> Result<FileCreated, SumidaError> {
    use std::path::Path;

    use uuid::Uuid;

    use crate::util::storage::create_storage;

    let content_type = request
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|ct| ct.to_str().ok())
      .unwrap_or("");

    let (file_data, metadata) = if content_type.starts_with("multipart/form-data") {
      let mut multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid multipart data".into()))?;

      let form_data = Self::parse_multipart_form(&mut multipart).await?;

      let file_data =
        form_data.file_data.ok_or_else(|| SumidaError::BadRequest("file is required".into()))?;

      (Some(file_data), form_data.metadata)
    } else {
      return Err(SumidaError::BadRequest("Only multipart/form-data is supported for file uploads".into()));
    };

    let (dto, metadata) = if let Some((original_filename, file_bytes, mime_type)) = file_data {
      let file_extension =
        Path::new(&original_filename).extension().and_then(|ext| ext.to_str()).unwrap_or("");

      let unique_filename = format!("{}.{}", Uuid::new_v4(), file_extension);

      let storage = create_storage();
      let file_path = storage.store(&file_bytes, &unique_filename).await?;
      let file_url = storage.get_url(&file_path);

      (
        CreateFile {
          path: file_path,
          uri: Optional(Some(file_url)),
          size: file_bytes.len() as i64,
          mime_type,
          hash: Optional(None),
          status: "completed".to_string(),
          is_temporary: Optional(metadata.as_ref().and_then(|m| m.is_temporary)),
          expires_at: match metadata.as_ref().and_then(|m| m.expires_at) {
            Some(v) => Nullable::Value(v),
            None => Nullable::Null,
          },
        },
        FileMetadata {
          original: original_filename,
        },
      )
    } else {
      return Err(SumidaError::BadRequest("file is required".into()));
    };

    let result = FileService::new(state, session).create(create_dto).await?;

    Ok(FileCreated {
      id: result.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(key): Path<String>,
    Json(body): Json<UpdateFile>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session).update(&key, body).await?;
    Ok(FileUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(key): Path<String>,
  ) -> Result<FileDeleted, SumidaError> {
    FileService::new(state, session).delete(&key).await?;
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
    Path(id): Path<String>,
  ) -> Result<Response<Body>, SumidaError> {
    let service = FileService::new(state.clone(), session.clone());
    let download_info = service
      .get_download_info(&id)
      .await?
      .ok_or_else(|| SumidaError::NoData("File".into(), id.to_string()))?;

    use crate::util::storage::create_storage;

    let storage = create_storage();
    let file_stream = storage.download(&download_info.file_path).await?;
    let file_size = storage.get_file_size(&download_info.file_path).await?;

    let stream = ReaderStream::new(file_stream);
    let body = Body::from_stream(stream);

    let response = Response::builder()
      .status(StatusCode::OK)
      .header(header::CONTENT_TYPE, download_info.content_type)
      .header(header::CONTENT_LENGTH, file_size.to_string())
      .header(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", download_info.filename),
      )
      .body(body)
      .map_err(|_| SumidaError::InternalServerError("Failed to create response".into()))?;

    Ok(response)
  }

  async fn mark_completed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<String>,
    Json(body): Json<MarkFileCompleted>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session)
      .mark_as_completed(&id, body.file_url, body.file_hash)
      .await?;
    Ok(FileUpdated)
  }

  async fn mark_failed(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<String>,
  ) -> Result<FileUpdated, SumidaError> {
    FileService::new(state, session).mark_as_failed(&id).await?;
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

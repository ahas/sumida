use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::response::Response;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use uuid::Uuid;

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
use crate::service::fs::dto::*;
use crate::service::fs::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

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

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<FileFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<FileListItem>, SumidaError> {
    Ok(FileService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<GetFile, SumidaError> {
    FileService::new(state, session)
      .get(uuid)
      .await?
      .ok_or_else(|| SumidaError::NoData("File".to_owned(), uuid.to_string()))
  }

  async fn upload(
    session: UserSession,
    State(state): State<S>,
    request: Request,
  ) -> Result<FileUploaded, SumidaError> {
    let file_upload_data = UploadFile::from_request(state.clone(), request).await?;
    let result = FileService::new(state, session).upload(file_upload_data).await?;

    Ok(result)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
    Json(body): Json<DeleteFile>,
  ) -> Result<FileDeleted, SumidaError> {
    FileService::new(state, session).delete(uuid, body).await?;

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
    Path(uuid): Path<Uuid>,
  ) -> Result<Response<Body>, SumidaError> {
    FileService::new(state.clone(), session.clone()).download(uuid).await
  }

  async fn inline(
    session: UserSession,
    State(state): State<S>,
    Path(uuid): Path<Uuid>,
  ) -> Result<Response<Body>, SumidaError> {
    FileService::new(state, session).inline(uuid).await
  }
}

impl<S> ApiController<S> for FileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "file")
      .read(
        Router::new()
          .route("/", routing::get(Self::list))
          .route("/{uuid}", routing::get(Self::get))
          .route("/stats", routing::get(Self::get_stats))
          .route("/{uuid}/download", routing::get(Self::download)),
      )
      .descriptor::<FileFilter>()
      .create(Router::new().route("/", routing::post(Self::upload)))
      .delete(Router::new().route("/{uuid}", routing::delete(Self::delete)))
      // Inline serving authorizes per-row (post visibility for board files, `file:read` otherwise),
      // so it takes the session guard only — not the `file:read` service gate.
      .ungated(Router::new().route("/{uuid}/inline", routing::get(Self::inline)))
      .build()
  }
}

use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::response::Response;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;

use crate::controller::ApiController;
use crate::service::fs::dto::*;
use crate::service::fs::prelude::*;
use crate::util::pagination::{List, Pagination};
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
    Path(id): Path<i32>,
    Json(body): Json<DeleteFile>,
  ) -> Result<FileDeleted, SumidaError> {
    FileService::new(state, session).delete(id, body).await?;

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
    Ok(FileService::new(state.clone(), session.clone()).download(id).await?)
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
      .route("/", routing::post(Self::upload))
      .route_layer(service_ap!(state, "file", create));

    // let route_update = Router::new() //
    //   .route_layer(service_ap!(state, "file", update));

    let route_delete = Router::new()
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "file", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      // .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

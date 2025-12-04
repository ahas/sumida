use axum::extract::{Path, Query, Request, State};
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use uuid::Uuid;

use crate::controller::ApiController;
use crate::service::bps::board_file::BoardFileService;
use crate::service::bps::board_file::dto::*;
use crate::service::fs::dto::{FileUploadTrait, FileUploaded};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

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
    Path((post_id, file_uuid)): Path<(i32, Uuid)>,
  ) -> Result<GetBoardFile, SumidaError> {
    BoardFileService::new(state, session)
      .get(post_id, file_uuid)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), format!("{post_id}-{file_uuid}")))
  }

  async fn upload(
    session: UserSession,
    State(state): State<S>,
    request: Request,
  ) -> Result<FileUploaded, SumidaError> {
    let file_upload_data = UploadBoardFile::from_request(state.clone(), request).await?;
    let result = BoardFileService::new(state, session).upload(file_upload_data).await?;

    Ok(result)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<DeleteBoardFile>,
  ) -> Result<BoardFileDeleted, SumidaError> {
    BoardFileService::new(state, session).delete(body).await?;

    Ok(BoardFileDeleted)
  }
}

impl<S> ApiController<S> for BoardFileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/file", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_file", read));

    let create = Router::new() //
      .route("/", routing::post(Self::upload))
      .route_layer(service_ap!(state, "board_file", create));

    let delete = Router::new() //
      .route("/", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_file", delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

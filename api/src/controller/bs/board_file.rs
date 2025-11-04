use axum::extract::{Path, Query, Request, State};
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;

use crate::controller::ApiController;
use crate::service::bs::board_file::BoardFileService;
use crate::service::bs::board_file::dto::*;
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
    Path((post_id, file_id)): Path<(i32, i32)>,
  ) -> Result<GetBoardFile, SumidaError> {
    BoardFileService::new(state, session)
      .get(post_id, file_id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardFile".into(), format!("{}-{}", post_id, file_id)))
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

  // async fn get_files_for_post(
  //   session: UserSession,
  //   State(state): State<S>,
  //   Path(post_id): Path<i32>,
  // ) -> Result<Json<Vec<BoardFileListItem>>, SumidaError> {
  //   let files = BoardFileService::new(state, session).get_files_for_post(post_id).await?;
  //   Ok(Json(files))
  // }
  //
  // async fn get_posts_for_file(
  //   session: UserSession,
  //   State(state): State<S>,
  //   Path(file_id): Path<i32>,
  // ) -> Result<Json<Vec<i32>>, SumidaError> {
  //   let post_ids = BoardFileService::new(state, session).get_posts_for_file(file_id).await?;
  //   Ok(Json(post_ids))
  // }
}

impl<S> ApiController<S> for BoardFileController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/", routing::get(Self::get))
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

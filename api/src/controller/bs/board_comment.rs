use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{service_ap, user_guard};
use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use error::SumidaError;
use std::marker::PhantomData;

#[derive(Clone, Debug, Default)]
pub struct BoardCommentController<S>(PhantomData<S>);

impl<S> BoardCommentController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    State(_): State<S>,
    Query(filter): Query<BoardCommentFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardCommentListItem>, SumidaError> {
    Ok(BoardCommentService::list(filter, pagination).await?.into())
  }

  async fn get(State(_): State<S>, Path(id): Path<i32>) -> Result<GetBoardComment, SumidaError> {
    BoardCommentService::get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardComment".into(), id.to_string()))
  }

  async fn create(
    State(_): State<S>,
    Json(body): Json<CreateBoardComment>,
  ) -> Result<BoardCommentCreated, SumidaError> {
    let res = BoardCommentService::create(body).await?;
    Ok(BoardCommentCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    State(_): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardComment>,
  ) -> Result<BoardCommentUpdated, SumidaError> {
    BoardCommentService::update(id, body).await?;
    Ok(BoardCommentUpdated)
  }

  async fn delete(State(_): State<S>, Path(id): Path<i32>) -> Result<BoardCommentDeleted, SumidaError> {
    BoardCommentService::delete(id).await?;
    Ok(BoardCommentDeleted)
  }
}

impl<S> ApiController<S> for BoardCommentController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!("board_comment", read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!("board_comment", create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!("board_comment", update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!("board_comment", delete));

    Router::new().merge(read).merge(create).merge(update).merge(delete).route_layer(user_guard!(state))
  }
}

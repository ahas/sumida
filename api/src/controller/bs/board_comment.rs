use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};
use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use error::SumidaError;
use infra::session::UserSession;
use std::marker::PhantomData;

#[derive(Clone, Debug, Default)]
pub struct BoardCommentController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> BoardCommentController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardCommentFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardCommentListItem>, SumidaError> {
    Ok(BoardCommentService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardComment, SumidaError> {
    BoardCommentService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardComment".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardComment>,
  ) -> Result<BoardCommentCreated, SumidaError> {
    let res = BoardCommentService::new(state, session).create(body).await?;
    Ok(BoardCommentCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardComment>,
  ) -> Result<BoardCommentUpdated, SumidaError> {
    BoardCommentService::new(state, session).update(id, body).await?;
    Ok(BoardCommentUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardCommentDeleted, SumidaError> {
    BoardCommentService::new(state, session).delete(id).await?;
    Ok(BoardCommentDeleted)
  }
}

impl<S: SumidaState> ApiController<S> for BoardCommentController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_comment", read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board_comment", create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board_comment", update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_comment", delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

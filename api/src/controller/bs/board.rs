use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{ability_guard, user_guard};
use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use core::session::UserContext;
use error::SumidaError;
use std::marker::PhantomData;

pub struct BoardController<S>(PhantomData<S>);

impl<S> BoardController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    user_ctx: UserContext,
    State(_): State<S>,
    Query(filter): Query<BoardFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardListItem>, SumidaError> {
    Ok(BoardService::list(&user_ctx, filter, pagination).await?.into())
  }

  async fn get(State(_): State<S>, Path(id): Path<i32>) -> Result<GetBoard, SumidaError> {
    match BoardService::get(id).await? {
      Some(item) => Ok(item),
      None => Err(SumidaError::NoData("Board".into(), id.to_string())),
    }
  }

  async fn create(State(_): State<S>, Json(body): Json<CreateBoard>) -> Result<BoardCreated, SumidaError> {
    let res = BoardService::create(body).await?;
    Ok(BoardCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    State(_): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoard>,
  ) -> Result<BoardUpdated, SumidaError> {
    BoardService::update(id, body).await?;
    Ok(BoardUpdated)
  }

  async fn delete(State(_): State<S>, Path(id): Path<i32>) -> Result<BoardDeleted, SumidaError> {
    BoardService::delete(id).await?;
    Ok(BoardDeleted)
  }
}

impl<S> ApiController<S> for BoardController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(ability_guard!(board, read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(ability_guard!(board, create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(ability_guard!(board, update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(ability_guard!(board, delete));

    Router::new().merge(read).merge(create).merge(update).merge(delete).route_layer(user_guard!(state))
  }
}

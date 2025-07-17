use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};
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

  async fn get(
    user_ctx: UserContext,
    State(_): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoard, SumidaError> {
    match BoardService::get(&user_ctx, id).await? {
      Some(item) => Ok(item),
      None => Err(SumidaError::NoData("Board".into(), id.to_string())),
    }
  }

  async fn create(
    user_ctx: UserContext,
    State(_): State<S>,
    Json(body): Json<CreateBoard>,
  ) -> Result<BoardCreated, SumidaError> {
    let res = BoardService::create(&user_ctx, body).await?;
    Ok(BoardCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoard>,
  ) -> Result<BoardUpdated, SumidaError> {
    BoardService::update(&user_ctx, id, body).await?;
    Ok(BoardUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardDeleted, SumidaError> {
    BoardService::delete(&user_ctx, id).await?;
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
      .route_layer(service_ap!(board, read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(board, create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(board, update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(board, delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

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
use infra::session::UserSession;
use std::marker::PhantomData;

#[derive(Clone, Debug, Default)]
pub struct BoardController<S>(PhantomData<S>);

impl<S> BoardController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(_): State<S>,
    Query(filter): Query<BoardFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardListItem>, SumidaError> {
    Ok(BoardService::new(session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(_): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoard, SumidaError> {
    BoardService::new(session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(_): State<S>,
    Json(body): Json<CreateBoard>,
  ) -> Result<BoardCreated, SumidaError> {
    Ok(BoardCreated {
      id: BoardService::new(session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(_): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoard>,
  ) -> Result<BoardUpdated, SumidaError> {
    BoardService::new(session).update(id, body).await?;
    Ok(BoardUpdated)
  }

  async fn delete(
    session: UserSession,
    State(_): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardDeleted, SumidaError> {
    BoardService::new(session).delete(id).await?;
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
      .route_layer(service_ap!("board", read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!("board", create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!("board", update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!("board", delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

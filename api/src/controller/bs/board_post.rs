use crate::controller::ApiController;
use crate::service::bs::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};
use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};
use error::SumidaError;

pub struct BoardPostController<S>(std::marker::PhantomData<S>);

impl<S> BoardPostController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }

  async fn list(
    State(_): State<S>,
    Query(filter): Query<BoardPostFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    Ok(BoardPostService::list(filter, pagination).await?.into())
  }

  async fn get(State(_): State<S>, Path(id): Path<i32>) -> Result<GetBoardPost, SumidaError> {
    match BoardPostService::get(id).await? {
      Some(item) => Ok(item),
      None => Err(SumidaError::NoData("BoardPost".into(), id.to_string())),
    }
  }

  async fn create(
    State(_): State<S>,
    Json(body): Json<CreateBoardPost>,
  ) -> Result<BoardPostCreated, SumidaError> {
    let res = BoardPostService::create(body).await?;
    Ok(BoardPostCreated {
      id: res.last_insert_id,
    })
  }

  async fn update(
    State(_): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardPost>,
  ) -> Result<BoardPostUpdated, SumidaError> {
    BoardPostService::update(id, body).await?;
    Ok(BoardPostUpdated)
  }

  async fn delete(State(_): State<S>, Path(id): Path<i32>) -> Result<BoardPostDeleted, SumidaError> {
    BoardPostService::delete(id).await?;
    Ok(BoardPostDeleted)
  }
}

impl<S> ApiController<S> for BoardPostController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(board_post, read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(board_post, create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(board_post, update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(board_post, delete));

    Router::new().merge(read).merge(create).merge(update).merge(delete).route_layer(user_guard!(state))
  }
}

use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::{user_guard, service_ap};
use crate::util::crud::{List, Pagination};
use crate::{controller::ApiController};

pub struct BoardCategoryController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> BoardCategoryController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    State(_state): State<S>,
    Query(filter): Query<BoardCategoryFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardCategoryListItem>, SumidaError> {
    Ok(BoardCategoryService::list(filter, pagination).await?.into())
  }

  async fn get(State(_state): State<S>, Path(id): Path<i32>) -> Result<GetBoardCategory, SumidaError> {
    match BoardCategoryService::get(id).await? {
      Some(cat) => Ok(cat),
      None => Err(SumidaError::NoData("BoardCategory".to_owned(), id.to_string())),
    }
  }

  async fn create(
    State(_state): State<S>,
    Json(body): Json<CreateBoardCategory>,
  ) -> Result<BoardCategoryCreated, SumidaError> {
    Ok(BoardCategoryCreated {
      id: BoardCategoryService::create(body).await?.last_insert_id,
    })
  }

  async fn update(
    State(_state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardCategory>,
  ) -> Result<BoardCategoryUpdated, SumidaError> {
    BoardCategoryService::update(id, body).await?;
    Ok(BoardCategoryUpdated)
  }

  async fn delete(
    State(_state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardCategoryDeleted, SumidaError> {
    BoardCategoryService::delete(id).await?;
    Ok(BoardCategoryDeleted)
  }
}

impl<S> ApiController<S> for BoardCategoryController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(board_category, read));

    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(board_category, create));

    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(board_category, update));

    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(board_category, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

use axum::{
  Json, Router,
  extract::{Path, Query, State},
  routing,
};

use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, controller::ApiController};
use crate::{service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct BoardCategoryController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardCategoryController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardCategoryFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardCategoryListItem>, SumidaError> {
    Ok(BoardCategoryService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardCategory, SumidaError> {
    BoardCategoryService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardCategory".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardCategory>,
  ) -> Result<BoardCategoryCreated, SumidaError> {
    Ok(BoardCategoryCreated {
      id: BoardCategoryService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardCategory>,
  ) -> Result<BoardCategoryUpdated, SumidaError> {
    BoardCategoryService::new(state, session).update(id, body).await?;
    Ok(BoardCategoryUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardCategoryDeleted, SumidaError> {
    BoardCategoryService::new(state, session).delete(id).await?;
    Ok(BoardCategoryDeleted)
  }
}

impl<S> ApiController<S> for BoardCategoryController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_category", read));

    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board_category", create));

    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board_category", update));

    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_category", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

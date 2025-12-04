use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;

use crate::controller::ApiController;
use crate::service::bps::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct BoardPostController<S: SumidaState>(std::marker::PhantomData<S>);

impl<S: SumidaState> BoardPostController<S> {
  pub fn new() -> Self {
    Self(std::marker::PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardPostFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    Ok(BoardPostService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardPost, SumidaError> {
    BoardPostService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))
  }

  async fn view(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<ViewBoardPost>,
  ) -> Result<GetBoardPost, SumidaError> {
    BoardPostService::new(state, session)
      .view(id, body)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardPost>,
  ) -> Result<BoardPostCreated, SumidaError> {
    let res = BoardPostService::new(state, session).create(body).await?;
    Ok(BoardPostCreated {
      id: res.last_insert_id,
    })
  }

  // async fn create_draft(
  //   session: UserSession,
  //   State(state): State<S>,
  //   Json(body): Json<CreateBoardDraft>,
  // ) -> Result<BoardPostCreated, SumidaError> {
  //   let res = BoardPostService::new(state, session).create_draft(body).await?;
  //   Ok(BoardPostCreated {
  //     id: res.last_insert_id,
  //   })
  // }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardPost>,
  ) -> Result<BoardPostUpdated, SumidaError> {
    BoardPostService::new(state, session).update(id, body).await?;
    Ok(BoardPostUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardPostDeleted, SumidaError> {
    BoardPostService::new(state, session).delete(id).await?;
    Ok(BoardPostDeleted)
  }
}

impl<S> ApiController<S> for BoardPostController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    // Read operations - basic authentication required
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route("/{id}/view", routing::post(Self::view))
      .route_layer(service_ap!(state, "board_post", read));

    // Write operations - board-level permissions checked in handlers
    let create = Router::new() //
      .route("/", routing::post(Self::create))
      // .route("/draft", routing::post(Self::create_draft))
      .route_layer(service_ap!(state, "board_post", create));

    // Update operations - ownership/moderate permissions checked in handlers
    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board_post", update));

    // Delete operations - ownership/moderate permissions checked in handlers
    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_post", delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

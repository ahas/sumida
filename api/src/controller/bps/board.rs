use std::marker::PhantomData;

use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use entity::bps::board_posts::BoardPostKind;
use error::SumidaError;
use infra::session::UserSession;

use crate::controller::ApiController;
use crate::service::bps::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct BoardController<S: SumidaState>(PhantomData<S>);

impl<S: SumidaState> BoardController<S> {
  pub fn new() -> Self {
    Self(PhantomData)
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardListItem>, SumidaError> {
    Ok(BoardService::new(state, session).list(filter, pagination).await?.into())
  }

  async fn get_posts(
    session: UserSession,
    State(state): State<S>,
    Path(board_id): Path<i32>,
    Query(mut filter): Query<BoardPostFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    // The route is `/boards/{id}/posts`; scope to that board regardless of any query `boardId`.
    filter.board_id = Some(board_id);

    let service = BoardPostService::new(state, session);

    let post_list = match filter.kind.as_ref().unwrap_or(&BoardPostKind::Article) {
      BoardPostKind::Comment => service.get_comments(filter, pagination).await?,
      BoardPostKind::Reply => service.get_replies(filter, pagination).await?,
      BoardPostKind::Notice => service.get_notices(filter, pagination).await?,
      _ => service.get_articles(filter, pagination).await?,
    };

    Ok(post_list)
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoard, SumidaError> {
    BoardService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoard>,
  ) -> Result<BoardCreated, SumidaError> {
    Ok(BoardCreated {
      id: BoardService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoard>,
  ) -> Result<BoardUpdated, SumidaError> {
    BoardService::new(state, session).update(id, body).await?;
    Ok(BoardUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardDeleted, SumidaError> {
    BoardService::new(state, session).delete(id).await?;
    Ok(BoardDeleted)
  }
}

impl<S> ApiController<S> for BoardController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    let read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route("/{id}/posts", routing::get(Self::get_posts))
      .route_layer(service_ap!(state, "board", read));

    let create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board", create));

    let update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board", update));

    let delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board", delete));

    Router::new() //
      .merge(read)
      .merge(create)
      .merge(update)
      .merge(delete)
      .route_layer(user_guard!(state))
  }
}

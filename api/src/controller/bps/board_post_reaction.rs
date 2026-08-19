use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_router};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct BoardPostReactionController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardPostReactionController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardPostReactionFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardPostReactionListItem>, SumidaError> {
    Ok(
      BoardPostReactionService::new(state, session)
        .list(filter, conditions, pagination)
        .await?
        .into(),
    )
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardPostReaction, SumidaError> {
    BoardPostReactionService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPostReaction".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardPostReaction>,
  ) -> Result<BoardPostReactionCreated, SumidaError> {
    Ok(BoardPostReactionCreated {
      id: BoardPostReactionService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardPostReaction>,
  ) -> Result<BoardPostReactionUpdated, SumidaError> {
    BoardPostReactionService::new(state, session).update(id, body).await?;
    Ok(BoardPostReactionUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardPostReactionDeleted, SumidaError> {
    BoardPostReactionService::new(state, session).delete(id).await?;
    Ok(BoardPostReactionDeleted)
  }
}

impl<S> ApiController<S> for BoardPostReactionController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new() //
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "board_post_reaction", read));
    // 이 목록이 무엇을 받는지 알려 주는 서술. 목록과 같은 읽기 게이트 뒤에 있고, 아래 최상위
    // `user_guard!` 를 함께 받는다.
    let route_filters =
      filter_descriptor_router::<S, BoardPostReactionFilter>(state, "board_post_reaction");
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "board_post_reaction", create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "board_post_reaction", update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "board_post_reaction", delete));

    Router::new() //
      .merge(route_read)
      .merge(route_filters)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

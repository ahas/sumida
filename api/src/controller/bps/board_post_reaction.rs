use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

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
    ControllerRouter::new(state, "board_post_reaction")
      .read(
        Router::new() //
          .route("/", routing::get(Self::list))
          .route("/{id}", routing::get(Self::get)),
      )
      .descriptor::<BoardPostReactionFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      .update(Router::new().route("/{id}", routing::patch(Self::update)))
      .delete(Router::new().route("/{id}", routing::delete(Self::delete)))
      .build()
  }
}

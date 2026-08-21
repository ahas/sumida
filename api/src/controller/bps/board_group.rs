use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

#[derive(Clone, Debug, Default)]
pub struct BoardGroupController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardGroupController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardGroupFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardGroupListItem>, SumidaError> {
    Ok(
      BoardGroupService::new(state, session)
        .list(filter, conditions, pagination)
        .await?
        .into(),
    )
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardGroup, SumidaError> {
    BoardGroupService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardGroup".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardGroup>,
  ) -> Result<BoardGroupCreated, SumidaError> {
    Ok(BoardGroupCreated {
      id: BoardGroupService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardGroup>,
  ) -> Result<BoardGroupUpdated, SumidaError> {
    BoardGroupService::new(state, session).update(id, body).await?;
    Ok(BoardGroupUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardGroupDeleted, SumidaError> {
    BoardGroupService::new(state, session).delete(id).await?;
    Ok(BoardGroupDeleted)
  }
}

impl<S> ApiController<S> for BoardGroupController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_group")
      .read(
        Router::new() //
          .route("/", routing::get(Self::list))
          .route("/{id}", routing::get(Self::get)),
      )
      .descriptor::<BoardGroupFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      .update(Router::new().route("/{id}", routing::patch(Self::update)))
      .delete(Router::new().route("/{id}", routing::delete(Self::delete)))
      .build()
  }
}

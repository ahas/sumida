use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::SumidaState;
use crate::controller::{ApiController, ControllerRouter};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

#[derive(Clone, Debug, Default)]
pub struct BoardTopicController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BoardTopicController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BoardTopicFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BoardTopicListItem>, SumidaError> {
    Ok(
      BoardTopicService::new(state, session)
        .list(filter, conditions, pagination)
        .await?
        .into(),
    )
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBoardTopic, SumidaError> {
    BoardTopicService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardTopic".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBoardTopic>,
  ) -> Result<BoardTopicCreated, SumidaError> {
    Ok(BoardTopicCreated {
      id: BoardTopicService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBoardTopic>,
  ) -> Result<BoardTopicUpdated, SumidaError> {
    BoardTopicService::new(state, session).update(id, body).await?;
    Ok(BoardTopicUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BoardTopicDeleted, SumidaError> {
    BoardTopicService::new(state, session).delete(id).await?;
    Ok(BoardTopicDeleted)
  }
}

impl<S> ApiController<S> for BoardTopicController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> Router<S> {
    ControllerRouter::new(state, "board_topic")
      .read(
        Router::new() //
          .route("/", routing::get(Self::list))
          .route("/{id}", routing::get(Self::get)),
      )
      .descriptor::<BoardTopicFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      .update(Router::new().route("/{id}", routing::patch(Self::update)))
      .delete(Router::new().route("/{id}", routing::delete(Self::delete)))
      .build()
  }
}

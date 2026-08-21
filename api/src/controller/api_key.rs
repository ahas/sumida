use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

#[derive(Clone, Debug, Default)]
pub struct ApiKeyController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> ApiKeyController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<ApiKeyFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<ApiKeyListItem>, SumidaError> {
    Ok(ApiKeyService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(key): Path<String>,
  ) -> Result<GetApiKey, SumidaError> {
    ApiKeyService::new(state, session)
      .get(&key)
      .await?
      .ok_or_else(|| SumidaError::NoData("ApiKey".to_owned(), key.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateApiKey>,
  ) -> Result<ApiKeyCreated, SumidaError> {
    // The plaintext key returned by create() is exposed only in this response (DB stores only the hash).
    Ok(ApiKeyCreated {
      key: ApiKeyService::new(state, session).create(body).await?,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(key): Path<String>,
    Json(body): Json<UpdateApiKey>,
  ) -> Result<ApiKeyUpdated, SumidaError> {
    ApiKeyService::new(state, session).update(&key, body).await?;
    Ok(ApiKeyUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(key): Path<String>,
  ) -> Result<ApiKeyDeleted, SumidaError> {
    ApiKeyService::new(state, session).delete(&key).await?;
    Ok(ApiKeyDeleted)
  }
}

impl<S> ApiController<S> for ApiKeyController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "api_key")
      .read(
        Router::new() //
          .route("/", routing::get(Self::list))
          .route("/{id}", routing::get(Self::get)),
      )
      .descriptor::<ApiKeyFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      .update(Router::new().route("/{id}", routing::patch(Self::update)))
      .delete(Router::new().route("/{id}", routing::delete(Self::delete)))
      .build()
  }
}

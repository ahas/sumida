use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

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
    Query(pagination): Query<Pagination>,
  ) -> Result<List<ApiKeyListItem>, SumidaError> {
    Ok(ApiKeyService::new(state, session).list(filter, pagination).await?.into())
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
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "api_key", read));
    let route_create = Router::new()
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "api_key", create));
    let route_update = Router::new()
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "api_key", update));
    let route_delete = Router::new()
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "api_key", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

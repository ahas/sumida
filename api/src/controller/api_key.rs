use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::service::prelude::*;
use crate::util::pagination::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

#[derive(Clone, Debug, Default)]
pub struct ApiKeyController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> ApiKeyController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(_state): State<S>,
    Query(filter): Query<ApiKeyFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<ApiKeyListItem>, SumidaError> {
    Ok(ApiKeyService::new(session).list(filter, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(_state): State<S>,
    Path(key): Path<String>,
  ) -> Result<GetApiKey, SumidaError> {
    ApiKeyService::new(session)
      .get(&key)
      .await?
      .ok_or_else(|| SumidaError::NoData("ApiKey".to_owned(), key.to_string()))
  }

  async fn create(
    session: UserSession,
    State(_state): State<S>,
    Json(body): Json<CreateApiKey>,
  ) -> Result<ApiKeyCreated, SumidaError> {
    Ok(ApiKeyCreated {
      key: ApiKeyService::new(session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(_state): State<S>,
    Path(key): Path<String>,
    Json(body): Json<UpdateApiKey>,
  ) -> Result<ApiKeyUpdated, SumidaError> {
    ApiKeyService::new(session).update(&key, body).await?;
    Ok(ApiKeyUpdated)
  }

  async fn delete(
    session: UserSession,
    State(_state): State<S>,
    Path(key): Path<String>,
  ) -> Result<ApiKeyDeleted, SumidaError> {
    ApiKeyService::new(session).delete(&key).await?;
    Ok(ApiKeyDeleted)
  }
}

impl<S> ApiController<S> for ApiKeyController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!("api_key", read));
    let route_create =
      Router::new().route("/", routing::post(Self::create)).route_layer(service_ap!("api_key", create));
    let route_update =
      Router::new().route("/{id}", routing::patch(Self::update)).route_layer(service_ap!("api_key", update));
    let route_delete =
      Router::new().route("/{id}", routing::delete(Self::delete)).route_layer(service_ap!("api_key", delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

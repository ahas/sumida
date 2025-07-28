use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
use crate::{service_ap, user_guard};

use super::ApiController;

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
    user_ctx: UserContext,
    State(_state): State<S>,
    Query(filter): Query<ApiKeyFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<ApiKeyListItem>, SumidaError> {
    Ok(ApiKeyService::list(&user_ctx, filter, pagination).await?.into())
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(key): Path<String>,
  ) -> Result<GetApiKey, SumidaError> {
    ApiKeyService::get(&user_ctx, &key)
      .await?
      .ok_or_else(|| SumidaError::NoData("ApiKey".to_owned(), key.to_string()))
  }

  async fn create(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<CreateApiKey>,
  ) -> Result<ApiKeyCreated, SumidaError> {
    Ok(ApiKeyCreated {
      key: ApiKeyService::create(&user_ctx, body).await?.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(key): Path<String>,
    Json(body): Json<UpdateApiKey>,
  ) -> Result<ApiKeyUpdated, SumidaError> {
    ApiKeyService::update(&user_ctx, &key, body).await?;
    Ok(ApiKeyUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(key): Path<String>,
  ) -> Result<ApiKeyDeleted, SumidaError> {
    ApiKeyService::delete(&user_ctx, &key).await?;
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
      .route_layer(service_ap!(api_key, read));
    let route_create =
      Router::new().route("/", routing::post(Self::create)).route_layer(service_ap!(api_key, create));
    let route_update =
      Router::new().route("/{id}", routing::patch(Self::update)).route_layer(service_ap!(api_key, update));
    let route_delete =
      Router::new().route("/{id}", routing::delete(Self::delete)).route_layer(service_ap!(api_key, delete));

    Router::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

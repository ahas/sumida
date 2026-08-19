use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::ApiController;
use crate::service::prelude::*;
use crate::util::filter::{ConditionQuery, filter_descriptor_router};
use crate::util::pagination::{List, Pagination};
use crate::{SumidaState, service_ap, user_guard};

#[derive(Clone, Debug, Default)]
pub struct BanController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> BanController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<BanFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<BanListItem>, SumidaError> {
    Ok(BanService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<GetBan, SumidaError> {
    BanService::new(state, session)
      .get(id)
      .await?
      .ok_or_else(|| SumidaError::NoData("Ban".to_owned(), id.to_string()))
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateBan>,
  ) -> Result<BanCreated, SumidaError> {
    Ok(BanCreated {
      id: BanService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateBan>,
  ) -> Result<BanUpdated, SumidaError> {
    BanService::new(state, session).update(id, body).await?;
    Ok(BanUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
  ) -> Result<BanDeleted, SumidaError> {
    BanService::new(state, session).delete(id).await?;
    Ok(BanDeleted)
  }
}

impl<S> ApiController<S> for BanController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{id}", routing::get(Self::get))
      .route_layer(service_ap!(state, "ban", read));
    let route_filters = filter_descriptor_router::<S, BanFilter>(state, "ban");
    let route_create = Router::new() //
      .route("/", routing::post(Self::create))
      .route_layer(service_ap!(state, "ban", create));
    let route_update = Router::new() //
      .route("/{id}", routing::patch(Self::update))
      .route_layer(service_ap!(state, "ban", update));
    let route_delete = Router::new() //
      .route("/{id}", routing::delete(Self::delete))
      .route_layer(service_ap!(state, "ban", delete));

    Router::new()
      .merge(route_read)
      .merge(route_filters)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

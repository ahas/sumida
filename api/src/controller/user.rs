use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing};
use axum_extra::extract::Query;

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

#[derive(Clone, Debug, Default)]
pub struct UserController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> UserController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    session: UserSession,
    State(state): State<S>,
    Query(filter): Query<UserFilter>,
    Query(conditions): Query<ConditionQuery>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<UserListItem>, SumidaError> {
    Ok(UserService::new(state, session).list(filter, conditions, pagination).await?.into())
  }

  async fn get(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<GetUser, SumidaError> {
    UserService::new(state, session).get(user_id).await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn create(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<CreateUser>,
  ) -> Result<UserCreated, SumidaError> {
    Ok(UserCreated {
      id: UserService::new(state, session).create(body).await?.last_insert_id,
    })
  }

  async fn update(
    session: UserSession,
    State(state): State<S>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateUser>,
  ) -> Result<UserUpdated, SumidaError> {
    UserService::new(state, session).update(id, body).await?;
    Ok(UserUpdated)
  }

  async fn delete(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<UserDeleted, SumidaError> {
    UserService::new(state, session).delete(user_id).await?;
    Ok(UserDeleted)
  }

  async fn invite(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(state, session).invite(user_id).await?;
    Ok(StatusCode::OK)
  }

  async fn ban(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
    Json(body): Json<BanSettings>,
  ) -> Result<StatusCode, SumidaError> {
    BanService::new(state, session).create_by_user_id(body, user_id).await?;
    Ok(StatusCode::OK)
  }

  async fn lift_ban(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<StatusCode, SumidaError> {
    BanService::new(state, session).lift_by_user_id(user_id).await?;
    Ok(StatusCode::OK)
  }

  async fn activate(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<UserActivation>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(state, session).activate(body).await?;
    Ok(StatusCode::ACCEPTED)
  }

  async fn change_password(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<ChangePassword>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(state, session).change_password(body).await?;
    Ok(StatusCode::OK)
  }

  async fn impersonate(
    session: UserSession,
    State(state): State<S>,
    Path(user_id): Path<i32>,
  ) -> Result<StatusCode, SumidaError> {
    UserService::new(state, session).impersonate(user_id).await?;
    Ok(StatusCode::ACCEPTED)
  }
}

impl<S> ApiController<S> for UserController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    // Standard CRUD routes requiring session/token auth + service_ap permission.
    // impersonate issues a session for an arbitrary user, so it must go through authentication,
    // but its authority is admin status rather than a namespace action — the service-layer
    // impersonate() is what checks it, so the route carries no action gate.
    let route_guarded = ControllerRouter::new(state, "user")
      .read(
        Router::new() //
          .route("/", routing::get(Self::list))
          .route("/{user_id}", routing::get(Self::get)),
      )
      .descriptor::<UserFilter>()
      .create(Router::new().route("/", routing::post(Self::create)))
      .update(
        Router::new()
          .route("/{user_id}", routing::patch(Self::update))
          .route("/{user_id}/invite", routing::post(Self::invite))
          .route("/{user_id}/ban", routing::post(Self::ban).delete(Self::lift_ban)),
      )
      .delete(Router::new().route("/{user_id}", routing::delete(Self::delete)))
      .ungated(Router::new().route("/{user_id}/impersonate", routing::post(Self::impersonate)))
      .build();

    // Pre-auth endpoints that self-verify via a body token (invitation uuid / verification code).
    let route_public = Router::new()
      .route("/change-password", routing::post(Self::change_password))
      .route("/activate", routing::post(Self::activate));

    Router::new().merge(route_guarded).merge(route_public)
  }
}

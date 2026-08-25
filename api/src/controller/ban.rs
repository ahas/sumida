use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where this controller's routes stand. The router nests here and the document appends here, so
/// the prefix is written once and the two cannot drift apart.
pub const MOUNT: &str = "/bans";

const ONE: &str = "/{id}";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "ban";

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
}

/// One page of the bans this caller may see, lifted ones and expired ones included.
///
/// A ban stays on the list after it is over; `liftedAt` and `expiresAt` are what say whether it is
/// still in force.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "ban_list",
  params(BanFilter, ConditionQuery, Pagination),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "One page of bans, and how many rows are behind it", body = List<BanListItem>),
    (
      status = 400,
      description = "A value in the query string is not of the shape it has to be (plain text), or \
                     the condition tree names a key, an operator or a size this list does not \
                     declare (error body).",
      content((String = "text/plain"), (ErrorBody = "application/json")),
    ),
    Refused,
  ),
))]
async fn list<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<BanFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<BanListItem>, SumidaError> {
  Ok(BanService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One ban, with the reason and the note the list entries leave out.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "ban_get",
  params(("id" = i32, Path, description = "The ban's identifier")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The ban", body = GetBan),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetBan, SumidaError> {
  BanService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("Ban".to_owned(), id.to_string()))
}

/// Writes a ban.
///
/// A ban names an account, an address, or both. It takes hold on the next request rather than at
/// the next sign-in, so credentials already handed out are held to it. Leaving `expiresAt` null
/// shuts the target out until the ban is lifted.
///
/// This is the route that can shut out an address rather than an account. Shutting out one account
/// is also `POST /users/{user_id}/ban`, which needs no identifier in the body.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "ban_create",
  request_body = CreateBan,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 201, description = "The identifier of the ban that was written", body = BanCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateBan>,
) -> Result<BanCreated, SumidaError> {
  Ok(BanCreated {
    id: BanService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Changes a ban.
///
/// Every property may be left out, but leaving one out does not always leave it alone: `userId`,
/// `ipAddress` and `expiresAt` are **cleared** by their absence, while `reason` and `note` keep what
/// is stored. To keep any of the three, send it back.
///
/// Setting `expiresAt` to a moment that has passed is how a ban is ended through this route;
/// `DELETE /users/{user_id}/ban` is what ends every ban on one account at once.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "ban_update",
  params(("id" = i32, Path, description = "The ban's identifier")),
  request_body = UpdateBan,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Changed. The body is an empty object and carries nothing of what changed", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn update<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
  Json(body): Json<UpdateBan>,
) -> Result<BanUpdated, SumidaError> {
  BanService::new(state, session).update(id, body).await?;
  Ok(BanUpdated)
}

/// Removes a ban outright, leaving no record that it was ever written.
///
/// Ending a ban while keeping the record of it is what `expiresAt` and lifting are for. Removing
/// what is not there is not an error. Whether an unknown identifier answers as removed or as
/// refused depends on the caller: a grant that covers the resource as a whole answers the same as
/// it would for a ban that was really there, and a grant that reaches only particular bans is
/// refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "ban_delete",
  params(("id" = i32, Path, description = "The ban's identifier")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Removed. The body is an empty object and carries no identifier", body = EmptyObject),
    Unreadable,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<BanDeleted, SumidaError> {
  BanService::new(state, session).delete(id).await?;
  Ok(BanDeleted)
}

impl<S> ApiController<S> for BanController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "ban")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<BanFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

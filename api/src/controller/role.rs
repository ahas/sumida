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
pub const MOUNT: &str = "/roles";

const ONE: &str = "/{id}";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "role";

#[derive(Clone, Debug, Default)]
pub struct RoleController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S> RoleController<S>
where
  S: SumidaState,
{
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// One page of the roles this caller may see.
///
/// The grants a role carries are not on the list entries; read one role to see them.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "role_list",
  params(RoleFilter, ConditionQuery, Pagination),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "One page of roles, and how many rows are behind it", body = List<RoleListItem>),
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
  Query(filter): Query<RoleFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<RoleListItem>, SumidaError> {
  Ok(RoleService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One role, with every grant it carries.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "role_get",
  params(("id" = i32, Path, description = "The role's identifier")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The role and its grants", body = GetRole),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetRole, SumidaError> {
  RoleService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("Role".to_owned(), id.to_string()))
}

/// Adds a role and writes the grants it carries.
///
/// `accessPolicies` is the set of route gates this role opens. Every entry names one namespace,
/// `object` is `["service"]` and `effect` is `"allow"` — those two are the only values this form
/// takes, because a grant is the only thing it can store and it is the route-gate tier only. A
/// grant on one particular row goes through `/relation-tuples` instead, which asks that row's own
/// permission. A caller may only hand out what it is itself allowed to hand out for that namespace.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "role_create",
  request_body = CreateRole,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 201, description = "The identifier of the role that was added", body = RoleCreated),
    (
      status = 400,
      description = "The body was read but a grant in it is not one this form can store — an \
                     `effect` other than `allow`, or an `object` other than `[\"service\"]`.",
      body = ErrorBody,
      content_type = "application/json",
    ),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateRole>,
) -> Result<RoleCreated, SumidaError> {
  Ok(RoleCreated {
    id: RoleService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Changes the properties that were sent, and leaves the rest.
///
/// Sending `accessPolicies` replaces the whole set rather than adding to it, so what is left out is
/// taken away. Leaving it out keeps what the role already carries.
///
/// `accessPolicies` is the set of route gates this role opens. Every entry names one namespace,
/// `object` is `["service"]` and `effect` is `"allow"` — those two are the only values this form
/// takes, because a grant is the only thing it can store and it is the route-gate tier only. A
/// grant on one particular row goes through `/relation-tuples` instead, which asks that row's own
/// permission. A caller may only hand out what it is itself allowed to hand out for that namespace.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "role_update",
  params(("id" = i32, Path, description = "The role's identifier")),
  request_body = UpdateRole,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Changed. The body is an empty object and carries nothing of what changed", body = EmptyObject),
    (
      status = 400,
      description = "The body was read but a grant in it is not one this form can store — an \
                     `effect` other than `allow`, or an `object` other than `[\"service\"]`.",
      body = ErrorBody,
      content_type = "application/json",
    ),
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
  Json(body): Json<UpdateRole>,
) -> Result<RoleUpdated, SumidaError> {
  RoleService::new(state, session).update(id, body).await?;
  Ok(RoleUpdated)
}

/// Removes a role, and with it every grant the role was carrying.
///
/// Accounts holding the role keep the role's identifier no longer; what they could do through it
/// they can no longer do. Removing what is not there is not an error. Whether an unknown identifier
/// answers as removed or as refused depends on the caller: a grant that covers the resource as a
/// whole answers the same as it would for a role that was really there, and a grant that reaches
/// only particular roles is refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "role_delete",
  params(("id" = i32, Path, description = "The role's identifier")),
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
) -> Result<RoleDeleted, SumidaError> {
  RoleService::new(state, session).delete(id).await?;
  Ok(RoleDeleted)
}

impl<S> ApiController<S> for RoleController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "role")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<RoleFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

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
pub const MOUNT: &str = "/organizations";

const ONE: &str = "/{id}";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "organization";

#[derive(Clone, Debug, Default)]
pub struct OrganizationController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> OrganizationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// One level of the tree, or a search across all of it.
///
/// Naming `parentId` reads that node's children. Naming none and searching by name answers from
/// every level, because the node someone is searching for is one they have not opened to. Naming
/// none and not searching answers with the roots, which is where reading a tree starts.
///
/// Which organizations come back is settled by the caller's grants before any of this is applied,
/// so a shorter list than expected is the answer and not a fault.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "organization_list",
  params(OrganizationFilter, ConditionQuery, Pagination),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "One page of the tree, and how many rows are behind it", body = List<OrganizationListItem>),
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
  Query(filter): Query<OrganizationFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<OrganizationListItem>, SumidaError> {
  Ok(
    OrganizationService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One organization.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "organization_get",
  params(("id" = i32, Path, description = "The organization's identifier")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The organization", body = GetOrganization),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetOrganization, SumidaError> {
  OrganizationService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("Organization".to_owned(), id.to_string()))
}

/// Adds an organization.
///
/// `parentId` is where it hangs. Leaving it null makes it a root.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "organization_create",
  request_body = CreateOrganization,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 201, description = "The identifier of the organization that was added", body = OrganizationCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateOrganization>,
) -> Result<OrganizationCreated, SumidaError> {
  Ok(OrganizationCreated {
    id: OrganizationService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Changes the properties that were sent, and leaves the rest.
///
/// Sending `parentId` moves the organization, and everything hanging under it moves with it.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "organization_update",
  params(("id" = i32, Path, description = "The organization's identifier")),
  request_body = UpdateOrganization,
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
  Json(body): Json<UpdateOrganization>,
) -> Result<OrganizationUpdated, SumidaError> {
  OrganizationService::new(state, session).update(id, body).await?;
  Ok(OrganizationUpdated)
}

/// Removes an organization.
///
/// Removing what is not there is not an error. Whether an unknown identifier answers as removed or
/// as refused depends on the caller: a grant that covers the resource as a whole answers the same
/// as it would for an organization that was really there, and a grant that reaches only particular
/// organizations is refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "organization_delete",
  params(("id" = i32, Path, description = "The organization's identifier")),
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
) -> Result<OrganizationDeleted, SumidaError> {
  OrganizationService::new(state, session).delete(id).await?;
  Ok(OrganizationDeleted)
}

impl<S> ApiController<S> for OrganizationController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "organization")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<OrganizationFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

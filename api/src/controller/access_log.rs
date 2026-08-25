use axum::extract::{Path, Query, State};
use axum::{Router, routing};

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{ACCESS_LOG_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here, so
/// the prefix is written once and the two cannot come to mean different places.
pub const MOUNT: &str = "/access-logs";

const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct AccessLogController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> AccessLogController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// Sign-in attempts, newest first.
///
/// Every attempt is recorded, a failed one included, so a row may name an account this installation
/// does not have and carry no account at all. Which keys narrow this list and which sort it are
/// answered by `GET /access-logs/filters`, not by this document — a deployment may declare its own.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = ACCESS_LOG_TAG,
  operation_id = "access_log_list",
  params(AccessLogFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of attempts, and how many there are behind it", body = List<AccessLogListItem>),
    // The one place a 400 has two shapes. A query-string value the reader cannot take is refused
    // as plain text before the handler; a condition tree outside this list's declaration reaches
    // the handler and comes back as the error layer's body.
    (
      status = 400,
      description = "A query-string value is not of the shape the reader takes (plain text), or \
                     the condition tree names a key, an operator or a size this list does not \
                     accept (error body).",
      content((String = "text/plain"), (ErrorBody = "application/json")),
    ),
    Refused,
  ),
))]
async fn list<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<AccessLogFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<AccessLogListItem>, SumidaError> {
  Ok(AccessLogService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One attempt, with the browser and platform its user agent was read as.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = ACCESS_LOG_TAG,
  operation_id = "access_log_get",
  params(("id" = i32, Path, description = "The attempt's identifier")),
  responses(
    (status = 200, description = "The attempt", body = GetAccessLog),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<GetAccessLog, SumidaError> {
  AccessLogService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("AccessLog".to_owned(), id.to_string()))
}

/// Strike one attempt from the record.
///
/// This is the trail an audit reads, and nothing else keeps a second copy of a row: what is removed
/// here does not come back.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = ACCESS_LOG_TAG,
  operation_id = "access_log_delete",
  params(("id" = i32, Path, description = "The attempt's identifier")),
  responses(
    (status = 200, description = "Removed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i32>,
) -> Result<AccessLogDeleted, SumidaError> {
  AccessLogService::new(state, session).delete(id).await?;
  Ok(AccessLogDeleted)
}

impl<S> ApiController<S> for AccessLogController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "access_log")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<AccessLogFilter>()
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

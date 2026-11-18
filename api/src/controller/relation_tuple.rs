use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{
  EmptyObject,
  ErrorBody,
  NotFound,
  RELATION_TUPLE_TAG,
  Refused,
  Unreadable,
  WrongBody,
};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/relation-tuples";

const ONE: &str = "/{id}";

/// The paragraph every operation here repeats, because a caller who reads only one of them still
/// has to know which of two refusals it got.
#[cfg(feature = "openapi")]
const TWO_GATES: &str = "Both refusals are `403` and they mean different things, so branch on \
`code` and not on the status. `E1009` is the route gate: the caller holds no `access_policy` \
capability at all and the handler never ran. `E1008` is the row check inside the handler: the \
caller may administer policy but holds no `grant` on the object this tuple names.";

/// `/relation-tuples` — the record of who holds what.
///
/// Two gates, and they answer with **different error codes**, which is the thing to hold on to when
/// reading a refusal:
///
/// * the route gate ([`ControllerRouter`]) asks `service:access_policy#can_<action>` and refuses with
///   `NoServiceAvailable` — `403`, code `1009`. It runs **before** the handler, so it says nothing
///   about the tuple being written.
/// * `Authz::write` / `Authz::revoke` ask `<resource_type>#grant` **on the object named in the
///   request** and refuse with `NoAccessPermission` — `403`, code `1008`.
///
/// Both are `403`, and conflating them is how a test passes with the row scope deleted: the gate
/// refuses first, so a caller who was never granted the service tier never reaches the part under
/// test. Assert the code, not the status.
///
/// `/access-policies` is mounted beside this and is read-only: a grant is written here, and read
/// there in the flat shape the CASL client consumes.
#[derive(Clone, Debug, Default)]
pub struct RelationTupleController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> RelationTupleController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The grants stored in this installation — operator-tier reading.
///
/// Every row is one grant: a subject holds a relation on an object. `subject` is the canonical key
/// the evaluator reads (`user:7`, `role:3#member`, `organization:100#member`, `user:*`), and the
/// vocabulary those strings are drawn from is served by `GET /authz/schema`.
///
/// This is the whole authorization record, so reading it tells the caller who may do what across
/// the installation. It stands behind the `access_policy` capability for that reason.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = RELATION_TUPLE_TAG,
  operation_id = "relation_tuple_list",
  params(RelationTupleFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of grants, and how many there are behind it", body = List<RelationTupleListItem>),
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
  Query(filter): Query<RelationTupleFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<RelationTupleListItem>, SumidaError> {
  Ok(
    RelationTupleService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One grant.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = RELATION_TUPLE_TAG,
  operation_id = "relation_tuple_get",
  params(("id" = i64, Path, description = "The grant's identifier")),
  responses(
    (status = 200, description = "The grant", body = RelationTupleListItem),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i64>,
) -> Result<RelationTupleListItem, SumidaError> {
  RelationTupleService::new(state, session)
    .get(id)
    .await?
    .ok_or_else(|| SumidaError::NoData("RelationTuple".to_owned(), id.to_string()))
}

/// Write a grant. **This is the operation that hands out authority.**
///
/// The tuple takes effect for the evaluator as soon as it is stored, and `expiresAt` is the only
/// condition one can carry — left out, the grant does not expire.
///
/// Every name in the request has to come from `GET /authz/schema`. A relation the vocabulary does
/// not declare, a subject written where an object belongs, or a relation whose `kind` is `derived`
/// is refused rather than stored: a row like that would sit in the table looking like authority and
/// match nothing, forever.
///
/// Two checks stand in front of it, and each refuses with `403` for its own reason.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = RELATION_TUPLE_TAG,
  operation_id = "relation_tuple_create",
  description = TWO_GATES,
  request_body = CreateRelationTuple,
  responses(
    (status = 201, description = "Stored. The identifier of the grant now in effect", body = RelationTupleCreated),
    (
      status = 400,
      description = "The relation, the resource type or the subject is not one the vocabulary \
                     declares, or the relation is derived and cannot be granted.",
      body = ErrorBody,
    ),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateRelationTuple>,
) -> Result<RelationTupleCreated, SumidaError> {
  Ok(RelationTupleCreated {
    id: RelationTupleService::new(state, session).create(body).await?,
  })
}

/// Take a grant back. Whatever the subject held only through this row is gone from the next check on.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = RELATION_TUPLE_TAG,
  operation_id = "relation_tuple_delete",
  description = TWO_GATES,
  params(("id" = i64, Path, description = "The grant's identifier")),
  responses(
    (status = 200, description = "Revoked. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(id): Path<i64>,
) -> Result<RelationTupleDeleted, SumidaError> {
  RelationTupleService::new(state, session).delete(id).await?;
  Ok(RelationTupleDeleted)
}

impl<S> ApiController<S> for RelationTupleController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    // The namespace is `access_policy`, not a new one: this endpoint replaces that screen, and
    // giving it a namespace of its own would silently take the capability away from everyone who
    // holds it today. `service:access_policy` resolves to the `relation_tuple` definition through
    // the inventory (`schema.sz`'s `namespace access_policy service = relation_tuple`).
    ControllerRouter::new(state, "access_policy")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<RelationTupleFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

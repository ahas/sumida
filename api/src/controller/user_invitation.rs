use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
use uuid::Uuid;

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where this controller's routes stand. The router nests here and the document appends here, so
/// the prefix is written once and the two cannot drift apart.
pub const MOUNT: &str = "/user-invitations";

const ONE: &str = "/{uuid}";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "user invitation";

#[derive(Clone, Debug, Default)]
pub struct UserInvitationController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> UserInvitationController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// One page of the invitations this caller may see.
///
/// Invitations that have run out are on this list like any other; `expiresAt` is what tells them
/// apart. Reading one by its identifier is what refuses an expired invitation.
///
/// Whose invitations a caller sees follows from whom that caller may see: the grant that shows an
/// account is the grant that shows the invitation issued to it.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invitation_list",
  params(UserInvitationFilter, ConditionQuery, Pagination),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "One page of invitations, and how many rows are behind it", body = List<UserInvitationListItem>),
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
  Query(filter): Query<UserInvitationFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserInvitationListItem>, SumidaError> {
  Ok(
    UserInvitationService::new(state, session)
      .list(filter, conditions, pagination)
      .await?
      .into(),
  )
}

/// One invitation, so long as it is still good.
///
/// An invitation that has run out answers the same way one that was never issued does. What the
/// token is worth is the point of reading it, and a token that is worth nothing is nothing to
/// report.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invitation_get",
  params(("uuid" = Uuid, Path, description = "The invitation token")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The invitation, and when it runs out", body = GetUserInvitation),
    Unreadable,
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<GetUserInvitation, SumidaError> {
  UserInvitationService::new(state, session)
    .get(uuid)
    .await?
    .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
}

/// **This route cannot be reached.** It asks for an account identifier in the path and stands at a
/// path that has no identifier in it, so every request to it — body and credentials
/// notwithstanding — is turned away before the handler runs.
///
/// The way to issue an invitation is `POST /users/{user_id}/invite`, which also mails it out.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invitation_create",
  request_body = CreateUserInvitation,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (
      status = 500,
      description = "Every request, once it is past the credential check. The body is the one line \
                     of plain text the reader leaves when a handler asks for a path parameter the \
                     route does not carry.",
      content_type = "text/plain",
      body = String,
    ),
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
  Json(_body): Json<CreateUserInvitation>,
) -> Result<UserInvitationCreated, SumidaError> {
  let invitation = UserInvitationService::new(state, session).create(user_id).await?;

  Ok(UserInvitationCreated {
    uuid:       invitation.uuid,
    expires_at: invitation.expires_at,
  })
}

/// **This route cannot be reached.** It stands at a path whose segment is an invitation token and
/// asks for that segment as an account identifier, so a token is refused as unreadable; and where a
/// number is given instead, what it then writes is addressed by a token it has just made up, which
/// nothing is stored under.
///
/// The way to replace an invitation is `POST /users/{user_id}/invite`, which issues a fresh one and
/// puts the previous one out of use.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invitation_update",
  params(("uuid" = Uuid, Path, description = "The invitation token — read as an account identifier, which is why nothing works here")),
  request_body = UpdateUserInvitation,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn update<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
  Json(_body): Json<UpdateUserInvitation>,
) -> Result<UserInvitationUpdated, SumidaError> {
  let invitation = UserInvitationService::new(state, session).update(user_id).await?;

  Ok(UserInvitationUpdated {
    uuid:       invitation.uuid,
    expires_at: invitation.expires_at,
  })
}

/// Withdraws an invitation.
///
/// The token stops working at once, and whoever holds it can no longer set a password with it.
/// Withdrawing what is not there is not an error. Whether an unknown token answers as withdrawn or
/// as refused depends on the caller: a grant that covers the resource as a whole answers the same
/// as it would for an invitation that was really there, and a grant that reaches only particular
/// invitations is refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invitation_delete",
  params(("uuid" = Uuid, Path, description = "The invitation token")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Withdrawn. The body is an empty object and carries no token", body = EmptyObject),
    Unreadable,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(uuid): Path<Uuid>,
) -> Result<UserInvitationDeleted, SumidaError> {
  UserInvitationService::new(state, session).delete(uuid).await?;
  Ok(UserInvitationDeleted)
}

impl<S> ApiController<S> for UserInvitationController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "user_invitation")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<UserInvitationFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

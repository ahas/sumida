use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Json, Router, routing};
use axum_extra::extract::Query;

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where this controller's routes stand. The router nests here and the document appends here, so
/// the prefix is written once and the two cannot drift apart.
pub const MOUNT: &str = "/users";

// The constants below are what the router's `route` and the document's `path` both read. Only the
// collection differs between the two: axum wants a leading `/` and the document wants the prefix
// itself.
const ONE: &str = "/{user_id}";
const INVITE: &str = "/{user_id}/invite";
const BAN: &str = "/{user_id}/ban";
const IMPERSONATE: &str = "/{user_id}/impersonate";
const CHANGE_PASSWORD: &str = "/change-password";
const ACTIVATE: &str = "/activate";

/// What binds this controller's operations together in the document.
#[cfg(feature = "openapi")]
const TAG: &str = "user";

/// What every route under this prefix answers when the caller may not see a row: the same thing it
/// answers when the row is not there.
#[cfg(feature = "openapi")]
const SCOPED: &str = "Which accounts a caller sees is settled by that caller's grants before any \
filter is applied, so a shorter list than expected is the answer and not a fault.";

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
}

/// Narrows and orders the accounts this caller may see, and answers one page of them.
///
/// Which keys narrow it, which keys order it and how large a page may be are answered by
/// `GET /users/filters` rather than stated here — those are a deployment's own declaration and the
/// server reads them out of it.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_list",
  params(UserFilter, ConditionQuery, Pagination),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = SCOPED, body = List<UserListItem>),
    // The only place a 400 has two shapes. A query value that cannot be read is refused by the
    // reader in plain text; a condition tree that steps outside what this list declares reaches the
    // handler and is refused with the error body.
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
  Query(filter): Query<UserFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<UserListItem>, SumidaError> {
  Ok(UserService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One account, with the organization and the role it holds.
///
/// An account this caller may not see is answered the same way one that does not exist is: with a
/// 401. Neither is told apart from credentials the server would not take in the first place.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_get",
  params(("user_id" = i32, Path, description = "The account's identifier")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The account", body = GetUser),
    Unreadable,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
) -> Result<GetUser, SumidaError> {
  UserService::new(state, session).get(user_id).await?.ok_or(SumidaError::InvalidAccount)
}

/// Opens an account.
///
/// A caller who is not an administrator cannot use this to hand out privilege: `isAdmin` is forced
/// to false and `organizationId` to the caller's own organization, whatever the body says.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_create",
  request_body = CreateUser,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 201, description = "The identifier of the account that was opened", body = UserCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateUser>,
) -> Result<UserCreated, SumidaError> {
  Ok(UserCreated {
    id: UserService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Changes an account.
///
/// Every property may be left out, but leaving one out does not always leave it alone: `email`,
/// `fullName` and `phone` are **cleared** by their absence, while everything else keeps what is
/// stored. To keep an address, send it back.
///
/// This is the channel for changing **somebody else's** account; changing one's own goes through
/// `/me`. A caller who is not an administrator may only reach accounts their grants cover, and the
/// four properties that decide privilege — `isAdmin`, `roleId`, `organizationId`, `isEnabled` — are
/// dropped from such a request rather than refusing it.
///
/// Sending `password` replaces the password. It does not void credentials already handed out; the
/// route that does that is `POST /users/change-password`.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_update",
  params(("user_id" = i32, Path, description = "The account's identifier")),
  request_body = UpdateUser,
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
  Json(body): Json<UpdateUser>,
) -> Result<UserUpdated, SumidaError> {
  UserService::new(state, session).update(id, body).await?;
  Ok(UserUpdated)
}

/// Removes an account.
///
/// Removing what is not there is not an error. Whether an unknown identifier answers as removed or
/// as refused depends on the caller: a grant that covers the resource as a whole answers the same
/// as it would for an account that was really there, and a grant that reaches only particular
/// accounts is refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_delete",
  params(("user_id" = i32, Path, description = "The account's identifier")),
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
  Path(user_id): Path<i32>,
) -> Result<UserDeleted, SumidaError> {
  UserService::new(state, session).delete(user_id).await?;
  Ok(UserDeleted)
}

/// Issues this account a fresh invitation and mails it out.
///
/// The invitation is what lets someone who cannot sign in yet set their own password, at
/// `POST /users/activate`. Issuing a new one replaces whatever invitation the account was holding,
/// so the token from an earlier mail stops working. It is good for 30 days.
///
/// The account has to have an email address on file: there is nowhere to send the invitation
/// otherwise, and that is refused rather than quietly skipped. A deployment built without mail
/// still records the invitation — the token can then be carried to the person by some other means.
///
/// The path names what to invite, so no body is read. Sending one is not an error and changes
/// nothing.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = INVITE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_invite",
  params(("user_id" = i32, Path, description = "The account to invite")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Issued. The response has no body"),
    Unreadable,
    (
      status = 500,
      description = "The account has no email address on file (`E1005`), so there is nowhere to \
                     send the invitation. Nothing about the request is retryable until an address \
                     is put on the account.",
      body = ErrorBody,
    ),
    Refused,
  ),
))]
async fn invite<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
) -> Result<StatusCode, SumidaError> {
  UserService::new(state, session).invite(user_id).await?;
  Ok(StatusCode::OK)
}

/// Shuts an account out.
///
/// It takes hold on that account's next request, not at its next sign-in: every credential already
/// handed out is held to it. Leaving `expiresAt` null shuts the account out until the ban is lifted.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = BAN,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_ban",
  params(("user_id" = i32, Path, description = "The account to shut out")),
  request_body = BanSettings,
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Shut out. The response has no body"),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn ban<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
  Json(body): Json<BanSettings>,
) -> Result<StatusCode, SumidaError> {
  BanService::new(state, session).create_by_user_id(body, user_id).await?;
  Ok(StatusCode::OK)
}

/// Lets an account back in.
///
/// Every ban on it that is still in force is lifted at once, each one asked for separately: a
/// caller who may not lift one of them lifts none. An account under no ban is not an error and
/// nothing happens.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = BAN,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_lift_ban",
  params(("user_id" = i32, Path, description = "The account to let back in")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Lifted, or there was nothing in force. The response has no body"),
    Unreadable,
    Refused,
  ),
))]
async fn lift_ban<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
) -> Result<StatusCode, SumidaError> {
  BanService::new(state, session).lift_by_user_id(user_id).await?;
  Ok(StatusCode::OK)
}

/// Takes on another account's identity for the rest of this session.
///
/// Administrators only, and the check is made here rather than by a route gate. What comes back is
/// a session that is the named account's: from the next request on, the caller is that account and
/// sees exactly what it sees. There is no route back — signing out and in again is what ends it.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = IMPERSONATE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_impersonate",
  params(("user_id" = i32, Path, description = "The account to take on")),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 202, description = "The session is now that account's. The response has no body"),
    Unreadable,
    Refused,
  ),
))]
async fn impersonate<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(user_id): Path<i32>,
) -> Result<StatusCode, SumidaError> {
  UserService::new(state, session).impersonate(user_id).await?;
  Ok(StatusCode::ACCEPTED)
}

/// Turns an invitation into a working account, and signs it in.
///
/// No credentials are needed to call this: the invitation is the credential. The `uuid` must be one
/// issued for that very `userId` and must not have run out — 30 days from when it was issued — and
/// it is spent here, so the same one cannot be used twice.
///
/// On success the account is enabled, the password is set to what was sent, and the caller is left
/// signed in as that account.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = ACTIVATE,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_activate",
  request_body = UserActivation,
  security(()),
  responses(
    (status = 202, description = "Enabled and signed in. The response has no body"),
    Unreadable,
    WrongBody,
    (
      status = 401,
      description = "There is no such account.",
      body = ErrorBody,
      content_type = "application/json",
    ),
    (
      status = 404,
      description = "The invitation is unknown, was issued for a different account, or has run out.",
      body = ErrorBody,
      content_type = "application/json",
    ),
  ),
))]
async fn activate<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<UserActivation>,
) -> Result<StatusCode, SumidaError> {
  UserService::new(state, session).activate(body).await?;
  Ok(StatusCode::ACCEPTED)
}

/// Sets a new password against a verification code.
///
/// No credentials are needed: the code is the credential. Ask for one first with
/// `POST /user-verifications` for kind `password-change`; it is six digits and good for three
/// minutes, and it is spent whether or not the password change that follows succeeds.
///
/// The change voids every credential the account already handed out — sessions, access tokens —
/// from the moment it takes effect. API keys are issued on their own terms and are not touched.
///
/// It also lets a banned account back in: the bans in force on the account, and on the address the
/// change comes from, are lifted, and the password failures before the change no longer count
/// towards a ban.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = CHANGE_PASSWORD,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "user_change_password",
  request_body = ChangePassword,
  security(()),
  responses(
    (status = 200, description = "The password is changed. The response has no body"),
    Unreadable,
    WrongBody,
    (
      status = 401,
      description = "There is no such account, or the code is wrong, spent or expired. The two are \
                     not told apart.",
      body = ErrorBody,
      content_type = "application/json",
    ),
  ),
))]
async fn change_password<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<ChangePassword>,
) -> Result<StatusCode, SumidaError> {
  UserService::new(state, session).change_password(body).await?;
  Ok(StatusCode::OK)
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
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<UserFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(
        Router::new()
          .route(ONE, routing::patch(update::<S>))
          .route(INVITE, routing::post(invite::<S>))
          .route(BAN, routing::post(ban::<S>).delete(lift_ban::<S>)),
      )
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .ungated(Router::new().route(IMPERSONATE, routing::post(impersonate::<S>)))
      .build();

    // Pre-auth endpoints that self-verify via a body token (invitation uuid / verification code).
    let route_public = Router::new()
      .route(CHANGE_PASSWORD, routing::post(change_password::<S>))
      .route(ACTIVATE, routing::post(activate::<S>));

    Router::new().merge(route_guarded).merge(route_public)
  }
}

use axum::extract::State;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde_json::json;

use crate::authz::Authz;
use crate::authz::effective::Effective;
use crate::authz::subject::SubjectSet;
use crate::controller::ApiController;
#[cfg(feature = "openapi")]
use crate::openapi::ME_TAG as TAG;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, Unauthenticated, Unreadable, WrongBody};
use crate::service::grants::{ApForUser, from_service_grants};
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

/// Where this controller's routes stand. The router nests here and the document hangs its paths
/// here, so the two cannot come to name different places.
pub const MOUNT: &str = "/me";

/// The routes below the account itself, named once for the router and the document both.
const PREFS: &str = "/prefs";
const ACCESS_POLICIES: &str = "/access-policies";
const PERMISSIONS: &str = "/permissions";

/// A change to the caller's own account, as the reader reads it.
///
/// Every key is spelled out because leaving one out is an instruction of its own rather than
/// silence, and an example that left one out would be teaching that instruction by accident.
#[cfg(feature = "openapi")]
pub fn update_example() -> serde_json::Value {
  json!({
    "username": "someone",
    "password": "the new password",
    "email": "someone@example.com",
    "fullName": "Someone",
    "phone": null,
  })
}

/// Preferences, as the reader reads them: whatever JSON the client wants to keep.
#[cfg(feature = "openapi")]
pub fn prefs_example() -> serde_json::Value {
  json!({ "theme": "dark", "listPageSize": 50 })
}

#[derive(Clone, Debug, Default)]
pub struct MeController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> MeController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The caller's own account.
///
/// Always the account behind this request; there is no way to ask about somebody else here. The
/// two timestamps `firstSeenAt` and `lastSeenAt` describe **this session** rather than the account
/// — when it was authorized and when it was last used — while `createdAt` and `updatedAt` describe
/// the account row.
///
/// `isBanned` reports whether a ban names the account. It does not report whether a ban would catch
/// the request that arrives next: a ban can also name an address or a device, and those are matched
/// against the request rather than against the account.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "get_me",
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The account behind this request", body = Me),
    Unauthenticated,
  ),
))]
async fn get_me<S: SumidaState>(session: UserSession, State(state): State<S>) -> Result<Me, SumidaError> {
  MeService::new(state, session).get_session().await?.ok_or(SumidaError::InvalidAccount)
}

/// Changes the caller's own account.
///
/// **Leaving a key out is an instruction, and not the same instruction for every field.**
/// `username` and `password` left out are left alone. `email`, `fullName` and `phone` left out are
/// **cleared** — for those three, absent and `null` say the same thing — so a request meaning to
/// change one of them must send the current value of the other two, and a request meaning to change
/// nothing else must send all three back.
///
/// `username` is ignored however it arrives: the account keeps the name it signed in with. A
/// `password` is hashed before it is stored and is never read back by any route. The account's
/// role, organization, administrator flag and enabled flag are not reachable from here — they are
/// somebody else's to change.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = "",
  context_path = MOUNT,
  tag = TAG,
  operation_id = "update_me",
  request_body(content = UpdateMe, example = update_example),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (
      status = 200,
      description = "Changed. The body carries nothing and does not echo what was written.",
      body = EmptyObject,
    ),
    Unreadable,
    WrongBody,
    Unauthenticated,
  ),
))]
async fn update_me<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<UpdateMe>,
) -> Result<MeUpdated, SumidaError> {
  MeService::new(state, session).update_me(body).await?;
  Ok(MeUpdated)
}

/// The caller's stored preferences.
///
/// Whatever JSON was last written here, and an empty object if nothing ever was. The server keeps
/// it and reads nothing out of it, so its shape is the client's to decide.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = PREFS,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "get_preferences",
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (
      status = 200,
      description = "What was last stored, or an empty object",
      body = serde_json::Value,
      example = prefs_example,
    ),
    Unauthenticated,
  ),
))]
async fn get_prefs<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
) -> Result<Json<serde_json::Value>, SumidaError> {
  Ok(Json(MeService::new(state, session).get_prefs().await?))
}

/// Replaces the caller's stored preferences.
///
/// The body is stored as it arrives and **replaces** what was there — this is not a merge, and a
/// key left out is a key removed.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = PREFS,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "update_preferences",
  request_body(content = serde_json::Value, example = prefs_example),
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "Stored. The body carries nothing.", body = EmptyObject),
    Unreadable,
    Unauthenticated,
  ),
))]
async fn update_prefs<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, SumidaError> {
  MeService::new(state, session).update_prefs(body).await?;
  Ok(Json(json!({})))
}

// The wire shape is fixed by the CASL runtime the clients share: it turns each element into
// `can(action, `${namespace}:${object}`)`. Because a `service` object's id *is* the namespace
// string, projecting that half from tuples is exact rather than approximate. The administrator
// entry is synthesised from `platform:root#superuser` rather than read from a catch-all grant row,
// which is what makes platform administration grantable as a tuple without touching
// `users.is_admin`.
//
// The object tier is deliberately not projected. A row-level decision is a predicate over the row
// under test — `board_post#view` reaches a post through its board, its group and its author — and
// no enumeration of object ids can say that. Inventing flat records for it would tell the client
// something the evaluator never decided.

/// The caller's own grants, flattened for a policy client.
///
/// One entry per namespace the caller holds a service-tier grant on, with `object` fixed to
/// `["service"]`, plus a single `{"namespace": "*", "object": ["*"], "action": ["*"]}` entry when
/// the caller is a platform administrator.
///
/// **This is the route-gate tier only.** It says which APIs the caller may reach, not which rows
/// the caller may act on. Row-level permission is a question about a particular row, so it is not
/// enumerated here and cannot be; ask for the row. For the two tiers kept apart, ask
/// `GET /me/permissions` instead.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ACCESS_POLICIES,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "get_access_policies",
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "The caller's service-tier grants", body = Vec<ApForUser>),
    Unauthenticated,
  ),
))]
async fn get_access_policies<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
) -> Result<Json<Vec<ApForUser>>, SumidaError> {
  let db = state.db();
  let subject = SubjectSet::of(&session);

  // The literal `["service"]` the client concatenates. A service grant has exactly one object and
  // it is this string.
  let mut policies: Vec<ApForUser> = from_service_grants(Authz::service_grants(&subject, db).await?);

  if Authz::is_superuser(&subject, db).await? {
    policies.push(ApForUser {
      namespace: "*".to_owned(),
      object:    vec!["*".to_owned()],
      action:    vec!["*".to_owned()],
      effect:    "allow".to_owned(),
    });
  }

  Ok(Json(policies))
}

/// What the caller may actually do.
///
/// This is what a client drives its UI from, and what it does that `/me/access-policies` cannot is
/// keep two tiers apart. `services` is "may I reach this API at all" — what the route gate checks,
/// and what decides whether a screen exists. `operates` is "may I act on everyone's rows, not only
/// my own" — a client that conflates the two shows an operator's global list to somebody who holds
/// only the route gate, and then the server correctly returns their own rows and the screen looks
/// broken. `services` always contains `operates`.
///
/// Every entry is evaluated through the compiled rule rather than read off the stored grants, so
/// one operator grant on a namespace reports every verb on it and a superuser grant reports every
/// namespace. `subjects` is the set of identities this request is judged as — a sign-in and a
/// secured api key are different sets for the same person.
///
/// **The service tier only.** There is no finite map of row-level permission to hand a client; a
/// screen that needs to know whether *this* row is editable asks for the row.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = PERMISSIONS,
  context_path = MOUNT,
  tag = TAG,
  operation_id = "get_permissions",
  security(("session" = []), ("bearer" = []), ("apiKey" = [])),
  responses(
    (status = 200, description = "What the caller may do, by tier", body = Effective),
    Unauthenticated,
  ),
))]
async fn get_permissions<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
) -> Result<Json<Effective>, SumidaError> {
  Ok(Json(Authz::effective(&SubjectSet::of(&session), state.db()).await?))
}

impl<S> ApiController<S> for MeController<S>
where
  S: SumidaState,
{
  /// The guard covers every route here. Each one answers about the caller, so there is no caller
  /// to answer about until one has been established.
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new()
      .route("/", routing::get(get_me::<S>))
      .route("/", routing::patch(update_me::<S>))
      .route(PREFS, routing::get(get_prefs::<S>))
      .route(PREFS, routing::patch(update_prefs::<S>))
      .route(ACCESS_POLICIES, routing::get(get_access_policies::<S>))
      .route(PERMISSIONS, routing::get(get_permissions::<S>))
      .route_layer(user_guard!(state))
  }
}

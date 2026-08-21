use axum::extract::State;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde_json::json;

use crate::authz::Authz;
use crate::authz::effective::Effective;
use crate::authz::subject::SubjectSet;
use crate::controller::ApiController;
use crate::service::grants::{ApForUser, from_service_grants};
use crate::service::prelude::*;
use crate::{SumidaState, user_guard};

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

  async fn update_me(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<UpdateMe>,
  ) -> Result<MeUpdated, SumidaError> {
    MeService::new(state, session).update_me(body).await?;
    Ok(MeUpdated)
  }

  async fn get_me(session: UserSession, State(state): State<S>) -> Result<Me, SumidaError> {
    MeService::new(state, session).get_session().await?.ok_or(SumidaError::InvalidAccount)
  }

  async fn get_prefs(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    Ok(Json(MeService::new(state, session).get_prefs().await?))
  }

  async fn update_prefs(
    session: UserSession,
    State(state): State<S>,
    Json(body): Json<serde_json::Value>,
  ) -> Result<Json<serde_json::Value>, SumidaError> {
    MeService::new(state, session).update_prefs(body).await?;
    Ok(Json(json!({})))
  }

  /// The caller's own grants, in the flat shape the CASL client consumes.
  ///
  /// **The wire shape is fixed by that client.** The CASL runtime plugin the clients share turns
  /// each element into `can(action, `${namespace}:${object}`)`, and because a `service` object's id
  /// *is* the namespace string, projecting that half from tuples is exact rather than approximate.
  ///
  /// Two kinds of entry come out:
  ///
  /// * **route gate** — one entry per namespace, `object: ["service"]`.
  /// * **the administrator's `{namespace:"*", object:["*"], action:["*"]}` entry** — synthesised
  ///   from `platform:root#superuser`, which the client's `namespace === "*"` branch turns into
  ///   `can("manage","all")`. Taking it from the rule rather than from a catch-all grant row is
  ///   what makes platform administration grantable as a tuple, without touching `users.is_admin`.
  ///
  /// The **object tier is deliberately not projected.** A row-level decision is a predicate over
  /// the row under test, not a list of objects: `board_post#view` reaches a post through its board,
  /// its group and its author, and no enumeration of object ids can say that. Inventing flat
  /// `namespace | object | action` records for it would tell the client something the evaluator
  /// never decided. `/me/permissions` is the endpoint that answers "what may I do" without lying
  /// about shape.
  async fn get_access_policies(
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
  /// This is the endpoint a client drives its UI from, and the one thing it does that
  /// `/access-policies` cannot is keep the two tiers apart: `services` is "may I reach this API at
  /// all" and `operates` is "may I act on everyone's rows". A flat list of
  /// `namespace | object | action` records cannot express that distinction — the two arrive as two
  /// rows and the client's CASL rules collapse them.
  ///
  /// Every entry is evaluated through the compiled rule rather than read off the tuples, so one
  /// `service:<namespace>#operator` tuple reports every capability on it, and `platform:root#superuser`
  /// reports every namespace. A client that had to re-derive that from the raw grants would be
  /// carrying a second copy of `schema.sz` in TypeScript.
  ///
  /// It reports the **service tier only**, and now that the object tier routes too that is a
  /// statement about what can be enumerated rather than about which store decides. A row-level
  /// decision is a predicate over the row under test; there is no finite map of it to hand a
  /// client. A screen that needs to know whether *this* row is editable asks for the row.
  async fn get_permissions(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<Effective>, SumidaError> {
    Ok(Json(Authz::effective(&SubjectSet::of(&session), state.db()).await?))
  }
}

impl<S> ApiController<S> for MeController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new()
      .route("/", routing::get(Self::get_me))
      .route("/", routing::patch(Self::update_me))
      .route("/prefs", routing::get(Self::get_prefs))
      .route("/prefs", routing::patch(Self::update_prefs))
      .route("/access-policies", routing::get(Self::get_access_policies))
      .route("/permissions", routing::get(Self::get_permissions))
      .route_layer(user_guard!(state))
  }
}

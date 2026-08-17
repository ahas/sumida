use axum::extract::State;
use axum::{Json, Router, routing};
use error::SumidaError;
use infra::session::UserSession;
use serde_json::json;

use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::controller::ApiController;
use crate::service::prelude::*;
use crate::util::ap::{self, ApForUser};
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

  /// The caller's own grants, in the shape the CASL client has always received (design §9.7).
  ///
  /// The wire shape is unchanged, and unchanged deliberately: `updateAbilityFor`
  /// (`trainer/client/vendor/skytree/packages/casl/src/runtime/plugin.ts:19`) turns each element into
  /// `can(action, `${namespace}:${object}`)`, and because a `service` object's id *is* today's
  /// namespace string, projecting that half from tuples is exact rather than approximate.
  ///
  /// The payload is assembled tier by tier, and **each tier is read from whichever store actually
  /// decides it**. That is the only shape that cannot lie to the client during Phase 1:
  ///
  /// * **route gate** — from `relation_tuples`. `service_ap!` answers from the compiled rule now, so
  ///   reading this half from `access_policies` would show a capability the server no longer
  ///   consults (and hide one it does).
  /// * **object tier** — still from `access_policies`, `deny` rows included, because that is still
  ///   what the server resolves object-tier checks against (see `util/ap.rs`'s header for why it
  ///   has not moved). Dropping it would hide UI whose request the server would allow, which is a
  ///   defect in usage even though it errs "safe".
  /// * **the administrator's `{namespace:"*", object:["*"], action:["*"]}` entry** — synthesised
  ///   from `platform:root#superuser`, which the client's `namespace === "*"` branch
  ///   (`plugin.ts:27`) turns into `can("manage","all")`. Taking it from the rule rather than from
  ///   the catch-all row is what makes platform administration grantable as a tuple, without
  ///   touching `users.is_admin`.
  ///
  /// The second bullet is what disappears when the object tier routes; nothing about the wire shape
  /// changes then either.
  async fn get_access_policies(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<Vec<ApForUser>>, SumidaError> {
    let db = state.db();
    let subject = SubjectSet::of(&session);

    let mut policies: Vec<ApForUser> = Authz::service_grants(&subject, db)
      .await?
      .into_iter()
      .map(|grant| ApForUser {
        namespace: grant.namespace,
        // The literal the client concatenates. A service grant has exactly one object and it is
        // this string; the legacy `ARRAY_AGG(DISTINCT ap."object")` produced the same one element.
        object:    vec!["service".to_owned()],
        action:    grant.action,
        effect:    "allow".to_owned(),
      })
      .collect();

    policies.extend(
      ap::for_users()
        .who(&session)
        .find_all(db)
        .await?
        .into_iter()
        // The route gate is the half above, from tuples. The `*` catch-all is the entry below,
        // from the rule. Everything else is the object tier, and it is passed through untouched.
        .filter(|policy| policy.namespace != "*" && policy.object != ["service"]),
    );

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
      .route_layer(user_guard!(state))
  }
}

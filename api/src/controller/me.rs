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

  /// The caller's own grants, in the shape the CASL client has always received (design §9.7).
  ///
  /// The wire shape is unchanged, and unchanged deliberately: `updateAbilityFor`
  /// (`trainer/client/vendor/skytree/packages/casl/src/runtime/plugin.ts:19`) turns each element into
  /// `can(action, `${namespace}:${object}`)`, and because a `service` object's id *is* today's
  /// namespace string, projecting that half from tuples is exact rather than approximate.
  ///
  /// The payload has two tiers, and both are read from `relation_tuples` now that the object tier
  /// routes too:
  ///
  /// * **route gate** — one entry per namespace, `object: ["service"]`.
  /// * **the administrator's `{namespace:"*", object:["*"], action:["*"]}` entry** — synthesised
  ///   from `platform:root#superuser`, which the client's `namespace === "*"` branch
  ///   (`plugin.ts:27`) turns into `can("manage","all")`. Taking it from the rule rather than from
  ///   the catch-all row is what makes platform administration grantable as a tuple, without
  ///   touching `users.is_admin`.
  ///
  /// The **object tier is deliberately not projected**, and its removal is the visible half of §14
  /// step 13. It used to be passed through from `access_policies` because that was still what the
  /// server resolved row-level checks against; the server resolves them from the rules now, and a
  /// rule is not a list of rows. Synthesising entries for it would mean inventing a flat
  /// `namespace | object | action` record for a decision that is a predicate over the row under
  /// test — `board_post#view` reaches a post through its board, its group and its author, and no
  /// enumeration of objects can say that. `/me/permissions` is the endpoint that answers "what may
  /// I do" without lying about shape.
  async fn get_access_policies(
    session: UserSession,
    State(state): State<S>,
  ) -> Result<Json<Vec<ApForUser>>, SumidaError> {
    let db = state.db();
    let subject = SubjectSet::of(&session);

    // The literal `["service"]` the client concatenates. A service grant has exactly one object and
    // it is this string; the legacy `ARRAY_AGG(DISTINCT ap."object")` produced the same one element.
    let mut policies: Vec<ApForUser> =
      from_service_grants(Authz::service_grants(&subject, db).await?);

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

  /// What the caller may actually do (design §9.7).
  ///
  /// This is the endpoint the post-migration client drives its UI from, and the one thing it does
  /// that `/access-policies` cannot is separate the two tiers §5.2 made different grants: `services`
  /// is "may I reach this API at all" and `operates` is "may I act on everyone's rows". The legacy
  /// payload cannot express that distinction — it reports `object: ["service"]` and `object: ["*"]`
  /// as two rows in one flat list, and the client's CASL rules collapse them.
  ///
  /// Every entry is evaluated through the compiled rule rather than read off the tuples, so one
  /// `service:trainer:task#operator` tuple reports all eight capabilities and `platform:root#superuser`
  /// reports every namespace. A client that had to re-derive that from the raw grants would be
  /// carrying a second copy of `schema.zed` in TypeScript.
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

use axum::Router;

use crate::SumidaState;
use crate::layer::{ServiceApLayer, UserGuardLayer};
use crate::util::filter::{FilterSchema, filter_descriptor_route};

pub mod access_log;
pub mod api_key;
pub mod auth;
pub mod authz;
pub mod ban;
#[cfg(feature = "board")]
pub mod bps;
pub mod dashboard;
pub mod dashboard_overview;
#[cfg(feature = "file")]
pub mod fs;
#[cfg(feature = "mail-form")]
pub mod mail_form;
pub mod me;
#[cfg(feature = "membership")]
pub mod membership;
pub mod organization;
pub mod relation_tuple;
pub mod role;
pub mod user;
pub mod user_invitation;
pub mod user_verification;

pub mod prelude;

pub trait ApiController<StateT>
where
  StateT: Clone + Send + Sync + 'static,
{
  fn router(self, state: &StateT) -> axum::Router<StateT>;

  fn mount(self, prefix: &str, state: &StateT, root: axum::Router<StateT>) -> axum::Router<StateT>
  where
    Self: Sized,
  {
    root.nest(prefix, self.router(state))
  }
}

/// The per-action route split, held by one object instead of by every author copying the shape.
///
/// The namespace is stated once, at construction. Each action's routes receive that action's gate
/// and no other, the descriptor route reuses the same namespace the read routes gate on, and the
/// session guard goes over the whole bundle exactly once. There is no way to hand two actions to
/// one gate, which is the property [`ServiceApLayer`] depends on: a bundle that mixes actions is
/// checked against the weakest action in it.
///
/// A controller supplies only the routes it owns. Which paths, which methods and which handlers
/// belong to an action stay with the controller; the wiring that must not vary stays here.
pub struct ControllerRouter<'a, StateT: SumidaState> {
  state:     &'a StateT,
  namespace: &'a str,
  read:      Router<StateT>,
  create:    Router<StateT>,
  update:    Router<StateT>,
  delete:    Router<StateT>,
  ungated:   Router<StateT>,
}

impl<'a, StateT> ControllerRouter<'a, StateT>
where
  StateT: SumidaState,
{
  /// Start a bundle for one namespace — the string every gate below is built from.
  pub fn new(state: &'a StateT, namespace: &'a str) -> Self {
    Self {
      state,
      namespace,
      read: Router::new(),
      create: Router::new(),
      update: Router::new(),
      delete: Router::new(),
      ungated: Router::new(),
    }
  }

  /// Routes that only read. Called more than once, the routers accumulate under the one gate.
  pub fn read(mut self, router: Router<StateT>) -> Self {
    self.read = self.read.merge(router);
    self
  }

  pub fn create(mut self, router: Router<StateT>) -> Self {
    self.create = self.create.merge(router);
    self
  }

  pub fn update(mut self, router: Router<StateT>) -> Self {
    self.update = self.update.merge(router);
    self
  }

  pub fn delete(mut self, router: Router<StateT>) -> Self {
    self.delete = self.delete.merge(router);
    self
  }

  /// `GET <resource>/filters` for the resource's declared filter schema, joined to the read routes
  /// so it stands behind the very gate the list stands behind — the namespace cannot be spelled
  /// differently here because it is not spelled here at all.
  ///
  /// # Panics
  ///
  /// As [`filter_descriptor_route`], and for the same reason.
  pub fn descriptor<F>(self) -> Self
  where
    F: FilterSchema + Send + Sync + 'static,
  {
    self.read(filter_descriptor_route::<StateT, F>())
  }

  /// Routes that take the session guard but no route gate, for the few endpoints whose
  /// authorization is settled inside the handler rather than by the namespace. Naming the method is
  /// the declaration: an endpoint arrives here on purpose, not by being left out of an action.
  pub fn ungated(mut self, router: Router<StateT>) -> Self {
    self.ungated = self.ungated.merge(router);
    self
  }

  /// Apply each action's gate, merge, and put the session guard over the result.
  ///
  /// An action the controller never named holds no routes, and axum refuses a layer that has
  /// nothing under it, so such an action contributes nothing instead of a gate guarding no route.
  pub fn build(self) -> Router<StateT> {
    let Self {
      state,
      namespace,
      read,
      create,
      update,
      delete,
      ungated,
    } = self;
    let gate = |router: Router<StateT>, action: &str| match router.has_routes() {
      true => router.route_layer(ServiceApLayer::new(state.clone(), namespace, action)),
      false => router,
    };

    Router::new()
      .merge(gate(read, "read"))
      .merge(gate(create, "create"))
      .merge(gate(update, "update"))
      .merge(gate(delete, "delete"))
      .merge(ungated)
      .route_layer(UserGuardLayer::new(state.clone()))
  }
}

#[cfg(test)]
mod tests {
  use super::ApiController;
  use crate::SumidaState;
  use crate::controller::access_log::AccessLogController;
  use crate::controller::api_key::ApiKeyController;
  use crate::controller::ban::BanController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board::BoardController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_file::BoardFileController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_group::BoardGroupController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post::BoardPostController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post_reaction::BoardPostReactionController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post_report::BoardPostReportController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_topic::BoardTopicController;
  #[cfg(feature = "file")]
  use crate::controller::fs::file::FileController;
  #[cfg(feature = "mail-form")]
  use crate::controller::mail_form::MailFormController;
  #[cfg(feature = "membership")]
  use crate::controller::membership::coupon::CouponController;
  #[cfg(feature = "membership")]
  use crate::controller::membership::plan::PlanController;
  use crate::controller::organization::OrganizationController;
  use crate::controller::relation_tuple::RelationTupleController;
  use crate::controller::role::RoleController;
  use crate::controller::user::UserController;
  use crate::controller::user_invitation::UserInvitationController;

  #[derive(Clone)]
  struct DummyState;

  impl SumidaState for DummyState {
    fn db(&self) -> &sea_orm::DatabaseConnection {
      unreachable!("assembling a router never reaches the database")
    }
  }

  /// Assembly is where a duplicated path and an invalid filter declaration both panic, and it is
  /// the one part of a controller that runs before any request does.
  #[test]
  fn converted_controllers_assemble() {
    let state = DummyState;

    assert!(RoleController::new().router(&state).has_routes());
    assert!(UserController::new().router(&state).has_routes());
    assert!(OrganizationController::new().router(&state).has_routes());
    assert!(ApiKeyController::new().router(&state).has_routes());
    assert!(AccessLogController::new().router(&state).has_routes());
    assert!(BanController::new().router(&state).has_routes());
    assert!(RelationTupleController::new().router(&state).has_routes());
    assert!(UserInvitationController::new().router(&state).has_routes());

    #[cfg(feature = "mail-form")]
    assert!(MailFormController::new().router(&state).has_routes());

    #[cfg(feature = "board")]
    {
      assert!(BoardController::new().router(&state).has_routes());
      assert!(BoardFileController::new().router(&state).has_routes());
      assert!(BoardGroupController::new().router(&state).has_routes());
      assert!(BoardPostController::new().router(&state).has_routes());
      assert!(BoardPostReactionController::new().router(&state).has_routes());
      assert!(BoardPostReportController::new().router(&state).has_routes());
      assert!(BoardTopicController::new().router(&state).has_routes());
    }

    #[cfg(feature = "file")]
    assert!(FileController::new().router(&state).has_routes());

    #[cfg(feature = "membership")]
    {
      assert!(CouponController::new().router(&state).has_routes());
      assert!(PlanController::new().router(&state).has_routes());
    }
  }
}

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
    let gate = |action: &str| ServiceApLayer::new(state.clone(), namespace, action);

    Router::new()
      .merge(read.route_layer(gate("read")))
      .merge(create.route_layer(gate("create")))
      .merge(update.route_layer(gate("update")))
      .merge(delete.route_layer(gate("delete")))
      .merge(ungated)
      .route_layer(UserGuardLayer::new(state.clone()))
  }
}

#[cfg(test)]
mod tests {
  use super::ApiController;
  use crate::SumidaState;
  use crate::controller::api_key::ApiKeyController;
  use crate::controller::organization::OrganizationController;
  use crate::controller::role::RoleController;
  use crate::controller::user::UserController;

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
  }
}

pub mod controller;
pub mod layer;
pub mod middleware;
pub mod provider;
pub mod service;
pub mod util;

use std::marker::PhantomData;

use axum::Router;
use controller::prelude::*;
use serde::Serialize;

use self::provider::RoleProvider;

pub mod sumida_core {
  pub use ::core::*;
}

pub struct Sumida<'a, StateT, RoleAbilityT, RoleProviderT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleProviderT: RoleProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  pub state: Option<&'a StateT>,
  pub role: Option<RoleProviderT>,
  _role_ability: PhantomData<RoleAbilityT>,
}

impl<'a, StateT, RoleAbilityT, RoleProviderT> Sumida<'a, StateT, RoleAbilityT, RoleProviderT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleProviderT: RoleProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  pub fn new() -> Self {
    Self::default()
  }

  pub fn set_state(mut self, state: &'a StateT) -> Self {
    self.state = Some(state);
    self
  }

  pub fn set_role_provider(mut self, role: RoleProviderT) -> Self {
    self.role = Some(role);
    self
  }

  pub fn router(self) -> Router<StateT> {
    let state = self.state.expect("[SumdiaRouter]: state must be set");

    Router::new()
      .nest("/organizations", OrganizationController::new::<StateT>(state))
      .nest("/roles", RoleController::new::<StateT, RoleAbilityT, RoleProviderT>(state))
      .nest("/users", UserController::new::<StateT>(state))
      .nest("/user-invitations", UserInvitationController::new::<StateT>(state))
      .nest("/user-verifications", UserVerificationController::new::<StateT>(state))
      .nest("/preference", PreferenceController::new::<StateT>(state))
      .nest("/mail-forms", MailFormController::new::<StateT>(state))
  }
}

impl<StateT, RoleAbilityT, RoleProviderT> Default for Sumida<'_, StateT, RoleAbilityT, RoleProviderT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleProviderT: RoleProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  fn default() -> Self {
    Self {
      state: None,
      role: None,
      _role_ability: std::marker::PhantomData,
    }
  }
}

pub mod controller;
pub mod core;
pub mod entity;
pub mod middleware;
pub mod migration;
pub mod provider;
pub mod service;
pub mod util;

use std::marker::PhantomData;

use axum::Router;
use controller::prelude::*;
use serde::Serialize;

use self::provider::RoleProvider;

pub struct SumidaRouter<'a, StateT, RoleAbilityT, RoleProviderT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleProviderT: RoleProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  pub state: Option<&'a StateT>,
  pub role: Option<RoleProviderT>,
  _role_ability: PhantomData<RoleAbilityT>,
}

impl<'a, StateT, RoleAbilityT, RoleProviderT> SumidaRouter<'a, StateT, RoleAbilityT, RoleProviderT>
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

  pub fn set_role(mut self, role: RoleProviderT) -> Self {
    self.role = Some(role);
    self
  }
}

impl<StateT, RoleAbilityT, RoleProviderT> Default for SumidaRouter<'_, StateT, RoleAbilityT, RoleProviderT>
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

impl<StateT, RoleAbilityT, RoleProviderT> From<SumidaRouter<'_, StateT, RoleAbilityT, RoleProviderT>>
  for Router<StateT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleProviderT: RoleProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  fn from(value: SumidaRouter<'_, StateT, RoleAbilityT, RoleProviderT>) -> Self {
    let state = value.state.expect("[SumdiaRouter]: state must be set");

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

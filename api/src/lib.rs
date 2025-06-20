pub mod controller;
pub mod layer;
pub mod middleware;
pub mod provider;
pub mod service;
pub mod util;

use axum::Router;
use controller::{ApiController, prelude::*};
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
  _role_ability: std::marker::PhantomData<RoleAbilityT>,
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
    let state = self.state.expect("[SumidaRouter]: state must be set");

    let api_key_controller = ApiKeyController::<StateT>::new();
    let org_controller = OrganizationController::<StateT>::new();
    let role_controller = RoleController::<StateT, RoleAbilityT, RoleProviderT>::new();
    let user_controller = UserController::<StateT>::new();
    let user_block_controller = UserBlockController::<StateT>::new();
    let user_log_controller = UserLogController::<StateT>::new();
    let user_invitation_controller = UserInvitationController::<StateT>::new();
    let user_verification_controller = UserVerificationController::<StateT>::new();
    let preference_controller = PreferenceController::<StateT>::new();
    let mail_form_controller = MailFormController::<StateT>::new();

    Router::new()
      .nest("/api-keys", api_key_controller.router(state))
      .nest("/organizations", org_controller.router(state))
      .nest("/roles", role_controller.router(state))
      .nest("/users", user_controller.router(state))
      .nest("/user-logs", user_log_controller.router(state))
      .nest("/user-blocks", user_block_controller.router(state))
      .nest("/user-invitations", user_invitation_controller.router(state))
      .nest("/user-verifications", user_verification_controller.router(state))
      .nest("/preference", preference_controller.router(state))
      .nest("/mail-forms", mail_form_controller.router(state))
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

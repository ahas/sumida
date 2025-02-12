pub mod controller;
pub mod core;
pub mod entity;
pub mod middleware;
pub mod migration;
pub mod provider;
pub mod service;
pub mod util;

use axum::Router;
use controller::prelude::*;
use serde::Serialize;

use self::provider::RoleAbilityProvider;

pub fn new_router<StateT, RoleAbilityT, RoleAbilityProviderT>(state: &StateT) -> Router<StateT>
where
  StateT: Clone + Send + Sync + 'static,
  RoleAbilityT: Default + Serialize + 'static,
  RoleAbilityProviderT: RoleAbilityProvider<RoleAbilityT> + Clone + Send + Sync + 'static,
{
  Router::new()
    .nest("/organizations", OrganizationController::new::<StateT>(&state))
    .nest("/roles", RoleController::new::<StateT, RoleAbilityT, RoleAbilityProviderT>(&state))
    .nest("/users", UserController::new::<StateT>(&state))
    .nest("/user-invitations", UserInvitationController::new::<StateT>(&state))
    .nest("/user-verifications", UserVerificationController::new::<StateT>(&state))
    .nest("/preference", PreferenceController::new::<StateT>(&state))
    .nest("/mail-forms", MailFormController::new::<StateT>(&state))
}

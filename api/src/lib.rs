pub mod controller;
pub mod layer;
pub mod service;
pub mod util;

use axum::Router;
use controller::{ApiController, prelude::*};
use sea_orm::DatabaseConnection;

pub trait SumidaState: Clone + Send + Sync + 'static {
  fn db(&self) -> &DatabaseConnection;
}

pub struct Sumida<'a, StateT>
where
  StateT: SumidaState,
{
  pub state: Option<&'a StateT>,
}

impl<'a, StateT> Sumida<'a, StateT>
where
  StateT: SumidaState,
{
  pub fn new() -> Self {
    Self::default()
  }

  pub fn set_state(mut self, state: &'a StateT) -> Self {
    self.state = Some(state);
    self
  }

  pub fn router(self) -> Router<StateT> {
    let state = self.state.expect("[SumidaRouter]: state must be set");

    let api_key_controller = ApiKeyController::<StateT>::new();
    let org_controller = OrganizationController::<StateT>::new();
    let role_controller = RoleController::<StateT>::new();
    let auth_session_controller = AuthSessionController::<StateT>::new();
    let me_controller = MeController::<StateT>::new();
    let user_controller = UserController::<StateT>::new();
    let user_block_controller = UserBlockController::<StateT>::new();
    let user_log_controller = UserLogController::<StateT>::new();
    let user_invitation_controller = UserInvitationController::<StateT>::new();
    let user_verification_controller = UserVerificationController::<StateT>::new();
    let mail_form_controller = MailFormController::<StateT>::new();

    // Board System
    let board_category_controller = BoardCategoryController::<StateT>::new();
    let board_controller = BoardController::<StateT>::new();
    let board_post_controller = BoardPostController::<StateT>::new();
    let board_files_controller = BoardFileController::<StateT>::new();

    Router::new()
      .nest("/api-keys", api_key_controller.router(state))
      .nest("/organizations", org_controller.router(state))
      .nest("/roles", role_controller.router(state))
      .nest("/auth/session", auth_session_controller.router(state))
      .nest("/me", me_controller.router(state))
      .nest("/users", user_controller.router(state))
      .nest("/user-logs", user_log_controller.router(state))
      .nest("/user-blocks", user_block_controller.router(state))
      .nest("/user-invitations", user_invitation_controller.router(state))
      .nest("/user-verifications", user_verification_controller.router(state))
      .nest("/mail-forms", mail_form_controller.router(state))
      // Board System
      .nest("/boards", board_controller.router(state))
      .nest("/board-categories", board_category_controller.router(state))
      .nest("/board-posts", board_post_controller.router(state))
      .nest("/board-files", board_files_controller.router(state))
  }
}

impl<StateT> Default for Sumida<'_, StateT>
where
  StateT: SumidaState,
{
  fn default() -> Self {
    Self { state: None }
  }
}

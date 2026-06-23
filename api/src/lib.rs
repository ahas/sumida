pub mod controller;
pub mod layer;
pub mod service;
pub mod util;

use axum::Router;
use controller::ApiController;
use controller::prelude::*;
use sea_orm::DatabaseConnection;

use crate::controller::access_policy::AccessPolicyController;

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

    let access_log_controller = AccessLogController::<StateT>::new();
    let access_policy_controller = AccessPolicyController::<StateT>::new();
    let api_key_controller = ApiKeyController::<StateT>::new();
    let dashboard_monitoring_controller = DashboardMonitoringController::<StateT>::new();
    let org_controller = OrganizationController::<StateT>::new();
    let role_controller = RoleController::<StateT>::new();
    let auth_session_controller = AuthSessionController::<StateT>::new();
    let auth_token_controller = AuthTokenController::<StateT>::new();
    let me_controller = MeController::<StateT>::new();
    let user_controller = UserController::<StateT>::new();
    let ban_controller = BanController::<StateT>::new();
    let user_invitation_controller = UserInvitationController::<StateT>::new();
    let user_verification_controller = UserVerificationController::<StateT>::new();
    let mail_form_controller = MailFormController::<StateT>::new();

    // File System
    let file_controller = FileController::<StateT>::new();

    // Membership / Billing System
    let plan_controller = PlanController::<StateT>::new();
    let subscription_controller = SubscriptionController::<StateT>::new();
    let credit_controller = CreditController::<StateT>::new();
    let entitlement_controller = EntitlementController::<StateT>::new();
    let purchase_controller = PurchaseController::<StateT>::new();
    let payment_controller = PaymentController::<StateT>::new();

    // Board System
    let board_group_controller = BoardGroupController::<StateT>::new();
    let board_controller = BoardController::<StateT>::new();
    let board_topic_controller = BoardTopicController::<StateT>::new();
    let board_post_controller = BoardPostController::<StateT>::new();
    let board_files_controller = BoardFileController::<StateT>::new();

    Router::new()
      .nest("/access-logs", access_log_controller.router(state))
      .nest("/access-policies", access_policy_controller.router(state))
      .nest("/api-keys", api_key_controller.router(state))
      .nest("/dashboard/monitoring", dashboard_monitoring_controller.router(state))
      .nest("/bans", ban_controller.router(state))
      .nest("/organizations", org_controller.router(state))
      .nest("/roles", role_controller.router(state))
      .nest("/auth/session", auth_session_controller.router(state))
      .nest("/auth/token", auth_token_controller.router(state))
      .nest("/me", me_controller.router(state))
      .nest("/users", user_controller.router(state))
      .nest("/user-invitations", user_invitation_controller.router(state))
      .nest("/user-verifications", user_verification_controller.router(state))
      .nest("/mail-forms", mail_form_controller.router(state))
      // Board System
      .nest("/boards", board_controller.router(state))
      .nest("/board-groups", board_group_controller.router(state))
      .nest("/board-topics", board_topic_controller.router(state))
      .nest("/board-posts", board_post_controller.router(state))
      .nest("/board-files", board_files_controller.router(state))
      // Membership / Billing System
      .nest("/membership-plans", plan_controller.router(state))
      .nest("/subscriptions", subscription_controller.router(state))
      .nest("/credits", credit_controller.router(state))
      .nest("/entitlements", entitlement_controller.router(state))
      .nest("/membership-purchases", purchase_controller.router(state))
      .nest("/payments", payment_controller.router(state))
      // File System
      .nest("/files", file_controller.router(state))
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

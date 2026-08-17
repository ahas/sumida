/// Relational Zanzibar — the replacement for `util::ap`. Compiled and boot-validated, and called by
/// nothing yet, on purpose: see the module docs.
pub mod authz;
pub mod capability;
pub mod controller;
pub mod layer;
pub mod service;
pub mod util;

pub use capability::Capability;

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
    #[cfg(feature = "mail-form")]
    let mail_form_controller = MailFormController::<StateT>::new();

    // File System (feature = "file")
    #[cfg(feature = "file")]
    let file_controller = FileController::<StateT>::new();

    // Membership / Billing System (feature = "membership")
    #[cfg(feature = "membership")]
    let plan_controller = PlanController::<StateT>::new();
    #[cfg(feature = "membership")]
    let subscription_controller = SubscriptionController::<StateT>::new();
    #[cfg(feature = "membership")]
    let credit_controller = CreditController::<StateT>::new();
    #[cfg(feature = "membership")]
    let entitlement_controller = EntitlementController::<StateT>::new();
    #[cfg(feature = "membership")]
    let purchase_controller = PurchaseController::<StateT>::new();
    #[cfg(feature = "membership")]
    let payment_controller = PaymentController::<StateT>::new();
    #[cfg(feature = "membership")]
    let coupon_controller = CouponController::<StateT>::new();

    // Board System (feature = "board")
    #[cfg(feature = "board")]
    let board_group_controller = BoardGroupController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_controller = BoardController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_topic_controller = BoardTopicController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_post_controller = BoardPostController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_post_reaction_controller = BoardPostReactionController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_post_report_controller = BoardPostReportController::<StateT>::new();
    #[cfg(feature = "board")]
    let board_files_controller = BoardFileController::<StateT>::new();

    // 코어(항상 마운트) + 능력 조회 엔드포인트.
    #[allow(unused_mut)]
    let mut router = Router::new()
      .route("/capabilities", axum::routing::get(capabilities_handler))
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
      .nest("/user-verifications", user_verification_controller.router(state));

    // Mail Form (feature = "mail-form")
    #[cfg(feature = "mail-form")]
    {
      router = router.nest("/mail-forms", mail_form_controller.router(state));
    }

    // Board System (feature = "board")
    #[cfg(feature = "board")]
    {
      router = router
        .nest("/boards", board_controller.router(state))
        .nest("/board-groups", board_group_controller.router(state))
        .nest("/board-topics", board_topic_controller.router(state))
        .nest("/board-posts", board_post_controller.router(state))
        .nest("/board-post-reactions", board_post_reaction_controller.router(state))
        .nest("/board-post-reports", board_post_report_controller.router(state))
        .nest("/board-files", board_files_controller.router(state));
    }

    // Membership / Billing System (feature = "membership")
    #[cfg(feature = "membership")]
    {
      router = router
        .nest("/membership-plans", plan_controller.router(state))
        .nest("/subscriptions", subscription_controller.router(state))
        .nest("/credits", credit_controller.router(state))
        .nest("/entitlements", entitlement_controller.router(state))
        .nest("/membership-purchases", purchase_controller.router(state))
        .nest("/payments", payment_controller.router(state))
        .nest("/coupons", coupon_controller.router(state));
    }

    // File System (feature = "file")
    #[cfg(feature = "file")]
    {
      router = router.nest("/files", file_controller.router(state));
    }

    router
  }
}

/// 이 빌드에 켜진 능력 목록. 클라이언트가 기능별 UI를 게이팅하는 데 쓴다.
async fn capabilities_handler() -> axum::Json<Vec<&'static Capability>> {
  axum::Json(capability::enabled())
}

impl<StateT> Default for Sumida<'_, StateT>
where
  StateT: SumidaState,
{
  fn default() -> Self {
    Self { state: None }
  }
}

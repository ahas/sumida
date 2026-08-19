/// Relational Zanzibar — compiled, boot-validated, and the only evaluator there is. It replaced the
/// lateral-join evaluator over `access_policies`, which is deleted.
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
  /// 라우터를 만들면서 이 제품의 권한 규칙을 등록한다.
  ///
  /// sumida 는 자기 규칙만 싣는다. 제품 규칙이 무엇인지는 제품이 말해야 하고, 그 말을 하는
  /// 자리가 여기다 — 모든 제품이 반드시 부르는 유일한 호출이라, 빠뜨리면 컴파일이 안 된다.
  /// 자기 규칙이 없는 제품은 `&[]` 를 넘긴다("없다"와 "잊었다"가 같아 보이면 안 된다).
  ///
  /// 등록은 레지스트리를 **처음 읽기 전**이어야 한다. 부팅 검증이 라우터 구성보다 먼저 도는
  /// 배선이라면 `Authz::install_rules` 를 그 위에서 한 번 더 부른다 — 같은 슬라이스로 여러 번
  /// 부르는 것은 오류가 아니다.
  ///
  /// # Panics
  ///
  /// 이미 다른 규칙 집합이 등록돼 있거나 레지스트리를 이미 읽은 뒤라면 패닉이다. 둘 다 "이
  /// 프로세스가 어떤 규칙으로 판정하는지가 호출 순서에 달려 있다"는 뜻이고, 그건 배선 실수이지
  /// 런타임 상태가 아니다 — `set_state` 를 빠뜨렸을 때와 같은 성질의 실패다.
  pub fn new(rules: &'static [authz::registry::RuleFile]) -> Self {
    if let Err(error) = authz::Authz::install_rules(rules) {
      panic!("[SumidaRouter]: {error}");
    }
    Self { state: None }
  }

  pub fn set_state(mut self, state: &'a StateT) -> Self {
    self.state = Some(state);
    self
  }

  pub fn router(self) -> Router<StateT> {
    let state = self.state.expect("[SumidaRouter]: state must be set");

    let access_log_controller = AccessLogController::<StateT>::new();
    let relation_tuple_controller = RelationTupleController::<StateT>::new();
    let authz_controller = AuthzController::<StateT>::new();
    let api_key_controller = ApiKeyController::<StateT>::new();
    let dashboard_monitoring_controller = DashboardMonitoringController::<StateT>::new();
    let dashboard_overview_controller = DashboardOverviewController::<StateT>::new();
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
      .nest("/relation-tuples", relation_tuple_controller.router(state))
      .nest("/authz", authz_controller.router(state))
      .nest("/api-keys", api_key_controller.router(state))
      .nest("/dashboard/monitoring", dashboard_monitoring_controller.router(state))
      .nest("/dashboard", dashboard_overview_controller.router(state))
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

// `Default` 는 일부러 두지 않는다. 그것이 있으면 `Sumida::default()` 로 규칙 등록을 건너뛸 수
// 있고, 그 순간 "제품이 규칙 등록을 잊으면 컴파일이 안 된다"가 성립하지 않는다.

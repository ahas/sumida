use crate::stub_ctx::StubCtx;
use crate::core::db::set_test_connection;
use crate::service::user::{dto::SignInUser, sign_in};
use test_utils::db::prepared_db;

#[tokio::test]
async fn sign_in_succeeds_and_sets_session() {
  // 1) 전역 커넥션을 Mock DB로 교체
  set_test_connection(prepared_db());

  // 2) Stub 세션 객체
  let mut ctx = StubCtx::default();

  // 3) 테스트 대상 호출
  sign_in(
    &mut ctx,
    SignInUser {
      username: "alice".into(),
      password: "pw123".into(),
      remember_me: Some(false),
    },
  )
  .await
  .unwrap();

  assert!(ctx.signed_in);
}

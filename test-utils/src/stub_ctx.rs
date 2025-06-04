//! test-utils/src/stub_ctx.rs
//! ------------------------------------------------------------
//! ① 서비스 로직이 요구하는 최소 인터페이스만 충족
//! ② sign_in 호출 여부·만료정보를 확인할 수 있는 필드 보유
//! ------------------------------------------------------------

use async_trait::async_trait;
use sumida::core::error::SumidaError; // Sign-in 결과와 동일한 에러 타입
use sumida::core::session::UserContext;
use tower_sessions::Expiry; // ← 실제 Sumida 코드에서 쓰는 타입 // 프로덕션 트rait

/// ------------------------------------------------------------
/// 세션에 저장되는 User 프로필(지금은 username 하나면 충분)
/// ------------------------------------------------------------
#[derive(Default, Debug, Clone)]
pub struct StubProfile {
  pub username: String,
}

/// ------------------------------------------------------------
/// 테스트 더블(UserContext Stub)
/// ------------------------------------------------------------
#[derive(Default, Debug)]
pub struct StubCtx {
  pub profile: StubProfile,   // 서비스 코드에서 읽는 필드
  pub signed_in: bool,        // sign_in 성공 여부 플래그
  pub expiry: Option<Expiry>, // remember-me 만료 기간 확인용
}

#[async_trait]
impl UserContext for StubCtx {
  async fn sign_in(&mut self, username: &str, expiry: Option<Expiry>) -> Result<(), SumidaError> {
    self.profile.username = username.to_owned();
    self.signed_in = true;
    self.expiry = expiry;
    Ok(())
  }

  // 필요하다면 UserContext trait의 다른 메서드도
  // 아래처럼 “아무 동작 안 함”으로 채워 주세요.
  //
  // async fn sign_out(&mut self) -> Result<(), SumidaError> {
  //     self.signed_in = false;
  //     Ok(())
  // }
}

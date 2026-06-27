# Tasks / Roadmap

다른 PC에서도 동일하게 확인할 수 있는 후속 작업 목록. 마치면 항목을 제거하거나
별도 "완료" 섹션으로 옮긴다. 작업 단위가 크면 별도 TASKS_*.md 로 분리할 수 있다.

## P1 — 운영 직결

### G5c. RedisJwtStore 실구현
- 위치: `infra/src/jwt.rs:53-112`
- 현재 상태: 모든 메서드가 `SumidaError::Unsupported("Redis JWT store not yet implemented")` 반환.
  따라서 `session-store-redis` feature로 빌드하면 `/auth/token` 호출 시 501.
- 필요 작업
  - `fred` crate(`Cargo.toml`에 이미 의존성 있음)로 RedisJwtStore::new에서 연결 풀 생성
  - `store_refresh_token` / `get_refresh_token` / `delete_refresh_token` — 키 `refresh_token:{jti}` + TTL = `expires_at`
  - `blacklist_token` / `is_token_blacklisted` — 키 `blacklist:{jti}` + TTL
  - `cleanup_expired` — TTL 자동 만료에 의존(noop)으로 두거나 명시적 SCAN
- 검증
  - `examples/basic`을 full-local로 띄우고 user 시드 후 `POST /auth/token`로 happy-path 확인
  - refresh rotation: 새 refresh로 또 refresh했을 때 이전 jti가 무효화되는지 확인

### G5d. DynamoDB Local 기반 /auth/token e2e 테스트
- `DynamoJwtStore`(`infra/src/jwt.rs:117-262`)는 production-ready이지만 자동 검증이 없음
- `e2e/` 또는 `examples/basic/tests/`에 DynamoDB Local(또는 LocalStack) 기반 시나리오 추가
- 시나리오: login → refresh → 두 번째 refresh 시 첫 refresh 무효 확인 → logout → blacklist 적용 확인

### Access token 블랙리스트
- 현재는 refresh token만 logout 시 블랙리스트(`service/auth/token.rs::logout`)
- access token이 짧으니 봐줄 만하지만, 즉시 무효화가 필요한 경우(보안 사고, 권한 회수)에 부족
- 옵션 A: helper.rs에 `revoke_access_token(jti, expires_at)` + 검증 시 `is_token_blacklisted` 호출
- 옵션 B: access TTL을 더 짧게(예: 5분)로 줄이고 그대로 두기

## P2 — 정합성 / 정리

### examples/basic의 stale 바이너리 복구
- `examples/basic/src/bin/create_dummy_board_posts.rs` 컴파일 에러
- 원인: `OrganizationsActiveModel.description/is_enabled`, `RolesActiveModel.level/is_enabled` 등
  entity 단순화 이후 따라가지 못함
- 영향: `cargo check --all-targets`가 깨짐. lib 빌드와 `app` 바이너리는 영향 없음
- 진단: 단순 dead-code 갱신. 30분 내 작업

### 운영 정책 시드 가이드 문서화
- G7에서 도입한 `namespace="org:user"` 권한을 운영 DB에 시드해야 함:
  `(namespace="org:user", object="<org_id>", action=["update"], effect="allow", user="role:<org_admin_role_id>")`
- 시드 누락 시 `is_admin=false` 사용자의 `PATCH /users/{id}`는 항상 403
- `README.md` 또는 `docs/permissions.md`에 절차 + SQL 예시 명시

## P3 — 갈라파고스 정리 / 표준화 (사용자 합의 후)

### Figment 마이그레이션
- 현재 `infra/src/settings/mod.rs`는 `config` crate + lazy_static
- 마이그레이션 시 환경변수 override / SSM provider 통합 / 부팅 시 오류 누적 가능
- 전제: e2e 회귀 테스트가 동일 settings.toml에 대해 같은 deserialize 결과를 확인

### RFC 7807 Problem Details 응답 전환
- 현재 응답: `{ code, statusCode, reason? }` (sumida 자체 스키마)
- 표준 전환 시 클라이언트 호환성 영향 — major 버전 변경 등 협의 필요
- 전환 위치: `error/src/error.rs::SumidaApiError::to_response` 한 곳

### ReBAC ApOr / object_in_subquery 사용처 확대
- 현재 `ApOr`와 `object_in_subquery`는 `UserService`만 사용
- 게시판 등 "owner OR moderator" 같은 OR 정책이 필요한 곳에 점진 적용
- 적용 전 deny 시멘틱(OR 분기 시 자식 간 deny 전파 없음) 운영 정책 명문화

### G5b 후속 — token 관련 코멘트/주석 정리
- `service/auth/token.rs` 상단 doc 보강 (login/refresh/logout 흐름과 token_type 검증 요점)
- `controller/auth/token.rs`에 OpenAPI 스타일 doc-comment 추가 (다운스트림 클라이언트가 참고)

## P2 — 멤버십 후속 (대형 / 외부·제품 의사결정 필요)

멤버십·결제 도메인의 1차 구현은 완료(`docs/membership.md`). 남은 항목은 외부 의존성
또는 제품 결정이 필요해 보류함.

### 빌링키 발급(issuance) 서버 연동
- 현재: `charge_recurring`(빌링키 결제)만 실연동. 빌링키 *발급*은 클라이언트 SDK 단계
- 필요 작업: PortOne `BillingKey.Issued` 웹훅 처리 → 발급된 billing_key를 대상 구독에 저장
  (구독-빌링키 연결 플로우/식별자 설계 필요)

### per-currency 크레딧 월렛 / 환율
- 현재: 크레딧 월렛은 owner당 1개·단일 통화(통화 필드는 정보성). 통화는 plan/purchase/coupon에만 의미
- 진짜 멀티커런시는 통화별 잔액(스키마 변경) + 환율 정책 필요. (원안에서도 "과설계"로 분류)

### 크레딧 reserve/capture ↔ PG authorize-capture 연결
- 현재: 내부 원장 기반 hold만. 외부 결제 승인-매입과는 분리
- 비동기 결제(가상계좌 등)와 묶을 때 연동 검토

### 소소한 항목
- 조직 owner 알림 채널 없음(만료임박/저잔액 메일은 personal만) — org 연락처 모델 필요 시
- 쿠폰은 PG checkout 경로에만 적용(무료 subscribe 경로 제외) — 의도된 분리, 필요 시 확장
- 스케줄러 다중 인스턴스 운영 시 advisory lock/리더 선출(현재 단일 러너 가정, `docs/membership.md` 참고)

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 자주 쓰는 명령

- `cargo run --bin migration` — 엔티티/마이그레이션 수정 후 스키마 적용
- `cargo run --features full-local --example basic` — 로컬 스택(Redis+local-fs)으로 기본 예제 서버 실행
- `cargo run --features full-aws --example basic` — AWS 스택(DynamoDB+S3+SSM) 검증
- `bash scripts/test/suite.sh` — 통합 테스트 전체. **통합 테스트는 이걸로 돌린다.**
- `cargo test` — 유닛 테스트용. 통합 테스트까지 함께 돌리면 결과를 믿을 수 없다(아래)
- `cargo test --package e2e` — E2E 시나리오만
- `cargo test --features test-suite` — CI feature-gated 검사
- `cargo fmt`, `cargo clippy --all-targets --all-features` — 푸시 전 필수
- `cargo tarpaulin --out Html` — 커버리지 리포트

## 워크스페이스 구조

5 크레이트 + 루트 `sumida` re-export 크레이트:

| 크레이트 | 책임 |
|---|---|
| `api/` | Axum 컨트롤러·서비스·레이어·유틸. `Sumida::new().router()` 진입점, `ApiController` 트레잇, `authz`(규칙 평가기), `util::pagination/s3/storage/db/...` |
| `entity/` | 공통 SeaORM 엔티티(users/roles/organizations/relation_tuples/api_keys/bans/access_logs/mail_forms/user_invitations/user_verifications/user_prefs/files/board_*) |
| `infra/` | DB 풀, 세션 스토어(redis/dynamodb), AWS SDK, settings(`config` crate), `messenger`(socketioxide), tracing, jwt |
| `error/` | `SumidaError` enum + `IntoResponse` + `StatusCode` 매핑 |
| `e2e/`, `examples/` | 테스트/예제 |

cargo feature로 백엔드 토글: `full-local`(redis + local-fs + 정적 JWT) vs `full-aws`(dynamodb + s3 + SSM JWT). 개별 feature(`session-store-*`, `jwt-secret-store-*`, `storage-*`, `aws-s3`, `image`)로 부분 조합 가능.

워크스페이스 의존 그래프: `api → infra, entity, error` / `infra → entity, error` / `entity → error` / `e2e → api+infra+entity+error`. 루트 `sumida`는 다섯 크레이트를 묶어 re-export.

## 코딩 스타일

- `rustfmt.toml`: edition 2024, 2-space, 108컬럼, `imports_granularity = "Module"`, `group_imports = "StdExternalCrate"`.
- 식별자: 함수/모듈 `snake_case`, 타입 `PascalCase`, 상수 `SCREAMING_SNAKE_CASE`.
- 커밋: Conventional Commits(`type(scope): summary`). `wip:` 접두사는 초안만, 머지 전 스쿼시.

---

## Sumida 활용 가이드 (다운스트림 서비스 작성용)

`sumida`를 의존성으로 추가한 서비스가 표준적으로 따르는 패턴.

### 1. 진입점 통합

```rust
#[derive(Clone)]
pub struct AppState { pub db: DatabaseConnection }
impl sumida::api::SumidaState for AppState {
  fn db(&self) -> &DatabaseConnection { &self.db }
}

// 규칙 등록은 레지스트리를 처음 읽기 전이어야 하므로 부팅 검증보다 먼저다.
sumida::api::authz::Authz::install_rules(my_app::authz::RULES)?;
// 규칙이 읽는 테이블·컬럼이 이 데이터베이스에 없으면 요청을 받기 전에 죽는다.
sumida::api::authz::Authz::boot_or_abort(&db).await?;
// 관계 어휘 투영 — relation_tuples 의 외래키가 이것을 참조한다.
sumida::api::authz::Authz::sync_schema(&db).await?;

let app = Router::new()
  .merge(sumida::api::Sumida::new(my_app::authz::RULES).set_state(&state).router())  // 공통 라우트 nest
  .merge(my_domain::new_router(&state)?)                         // 도메인 라우트 nest
  .layer(sumida::infra::session::new_layer().await?)             // 세션
  .layer(sumida::api::layer::cors::new())                        // CORS
  .with_state(state);
```

`Sumida::new(RULES).router()`가 nest하는 경로. 기능 feature가 끄는 묶음은 그 feature를 함께 적었다 —
부분 빌드에서는 그 줄이 통째로 없다.

| 조건 | 경로 |
|---|---|
| 항상 | `/capabilities`, `/access-logs`, `/relation-tuples`, `/authz`, `/api-keys`, `/dashboard`, `/dashboard/monitoring`, `/bans`, `/organizations`, `/roles`, `/auth/session`, `/auth/token`, `/me`, `/users`, `/user-invitations`, `/user-verifications` |
| `mail-form` | `/mail-forms` |
| `board` | `/boards`, `/board-groups`, `/board-topics`, `/board-posts`, `/board-post-reactions`, `/board-post-reports`, `/board-files` |
| `membership` | `/membership-plans`, `/subscriptions`, `/credits`, `/entitlements`, `/membership-purchases`, `/payments`, `/coupons` |
| `file` | `/files` |

### 2. ReBAC 권한 모델 — 규칙 언어와 관계 튜플

권한은 `.sz` 규칙 파일에서 컴파일된 SQL 술어로 평가된다. 규칙이 **어떤 종류의 권한이 존재하고
어떻게 조합되는지**를 정하고, 관계 튜플이 **누가 그것을 가지는지**를 정한다. 규칙은 배포로만
바뀌고 튜플은 런타임에 바뀐다.

- 언어 문법과 설계 근거: [`docs/sz-language.md`](docs/sz-language.md)
- 선언 구조와 시드 실무: [`docs/permissions.md`](docs/permissions.md)

**sumida 는 자기 규칙 파일(`api/src/authz/schema.sz`) 하나만 싣는다.** 다운스트림 서비스는 자기
`.sz` 파일을 자기 저장소에 두고 `Sumida::new(RULES)` 로 등록한다. sumida 의 `organization` 에
자기 리소스 종류를 붙일 때는 자기 파일에서 `extend organization { scope <종류> }` 로 붙인다 —
sumida 쪽 파일은 고치지 않으며, sumida 는 어떤 서비스가 자기를 쓰는지 알지 않는다.

세 층을 모두 통과해야 허용된다.

```
platform:root#admin@user:1              플랫폼 — 전부 통과
service:<네임스페이스>#read@role:3#member   라우트 게이트 — 이 API를 부를 자격
file:<uuid>#viewer@user:9               객체(행) — 이 행에 닿을 자격
```

게이트 거절과 소유권 거절은 **서로 다른 오류 코드**로 나온다. 같은 403으로 뭉뚱그리면 행 스코프를
통째로 지워도 테스트가 초록으로 남는다.

호출 패턴은 둘뿐이다.

| 호출 | 용도 |
|---|---|
| 리스트 스코프 | 목록 쿼리의 `WHERE` 절을 규칙에서 만든다. 필터·`COUNT`·`ORDER BY`·`LIMIT`가 한 문장에 남는다 |
| 단건 점검 | 특정 행에 특정 권한이 있는지 묻는다. 수정·삭제 전 가드 |

**읽기와 쓰기는 다른 권한으로 검사한다.** 목록에 보이는 것과 지울 수 있는 것은 다른 규칙이며,
같은 스코프를 재사용하면 읽기 권한만 있는 행이 삭제된다.

부여와 회수는 관계 튜플 엔드포인트로 하고, **부여 자체가 인가된다** — 대상 객체에 대한 부여
권한이 있어야 하며 자기가 갖지 않은 것은 줄 수 없다.

서버는 부팅 때 규칙을 실제 스키마에 대조하고, 규칙이 읽는 컬럼이 없으면 **기동을 거부한다.**
조용히 잘못된 행을 고르는 것보다 낫다.

### 3. 서비스 표준

서비스는 자기 리소스의 리스트 스코프와 단건 점검을 직접 붙인다. 공통 트레이트를 통해 자동으로
얻는 구조가 아니므로, 읽기 경로에 스코프를 빠뜨리면 아무도 대신 채워 주지 않는다. **새 읽기
경로를 추가할 때 이것이 가장 흔한 누락 지점이다.**

부모-자식 종속은 규칙 쪽에서 푼다. 자식 정의가 부모로 가는 관계를 갖고 부모의 권한을 참조하면,
자식 조회에 부모 조인이 자동으로 따라붙는다. 서비스 코드에서 조인을 조립하지 않는다.

### 4. 컨트롤러 표준 — 액션별 `service_ap!` 분리

`RoleController`가 기준 패턴.

```rust
fn router(self, state: &S) -> Router<S> {
  let route_read   = Router::new().route("/", get(Self::list)).route("/{id}", get(Self::get))
    .route_layer(service_ap!(state, "my:ns", read));
  let route_create = Router::new().route("/", post(Self::create))
    .route_layer(service_ap!(state, "my:ns", create));
  let route_update = Router::new().route("/{id}", patch(Self::update))
    .route_layer(service_ap!(state, "my:ns", update));
  let route_delete = Router::new().route("/{id}", delete(Self::delete))
    .route_layer(service_ap!(state, "my:ns", delete));

  Router::new().merge(route_read).merge(route_create).merge(route_update).merge(route_delete)
    .route_layer(user_guard!(state))   // 최상위에 한 번만
}
```

레이어 종류:
- `user_guard!(state)` — 세션 또는 `Authorization: Bearer <jwt>` / `APIKEY <key>` 검증
- `admin_guard!(state)` — `profile.is_admin` 체크
- `service_ap!(state,"ns",action)` — 라우트 게이트. `service:<ns>#can_<action>` 을 통과해야 진입하며, 행 스코프는 그 뒤에 따로 검사된다

게시판 단위 권한을 위한 레이어는 없다. 그것은 규칙이 푼다 — `board_post#edit` 이 글의 그룹과
게시판과 작성자를 합집합으로 묶고 차단을 빼므로, 호출 자리에서 조립할 것이 남지 않는다.

**중요**: 액션별로 `service_ap!`를 따로 묶어야 한다. 한 묶음에 다른 액션을 섞으면 권한 체크가 약한 쪽으로 새어 나간다.

### 5. DTO 표준

`Filter`(Query) / `ListItem`(`FromQueryResult`) / `Get` / `Create`+`Update`(`IntoActiveModel`) / `Created`+`Updated`+`Deleted`(`IntoResponse`) 5종 세트.

PATCH 시멘틱은 `sea-orm-crud` 래퍼로 해결:
- `Optional<T>` — JSON에 키 없음=`NotSet`(변경 안 함), 있음=`Set(v)`
- `Nullable<T>` — 3-state(Value/Null/Undefined). null 명시=비우기, 미존재=변경 안 함

DTO `IntoActiveModel`에서 `.into_active_value()` 한 줄로 변환.

### 6. 인프라 유틸 재사용

- DB 연결: `sumida::infra::db::connect()` 한 번, 이후 `state.db()`로 접근
- AWS SDK: `aws::load_sdk_config().await` 한 번, `aws::get_sdk_config()`로 공유
- S3: `sumida::api::util::s3::{get_bucket, get_bytes, get_json, get_image, delete, IntoS3Uri, static_uri, parse_uri}`
- 파일 다운로드: `SumidaStorage::new().download(DownloadFile{...}, with_original_name)` — 로컬/S3 백엔드 추상화
- 페이지네이션: `Pagination{page,limit,sort,order}` → `.paginate(q)` / `.order_by(q, |c| match c { "name"=>Col::Name, .. })` + 응답 `List::from((items, count))`
- 목록 필터 선언: 리소스가 자기 `Filter` 옆에서 `util::filter::FilterSchema` 를 구현하면 정렬 키
  해석(`.order_by_schema::<MyFilter, _>(q)`)과 서술 엔드포인트(`GET <리소스>/filters`)가 같은
  선언을 읽는다. 서술 라우트는 `filter_descriptor_router`(네임스페이스로 게이트하는 목록) 또는
  `filter_descriptor_route`(세션 가드만 걸린 목록 — 게이트 없이 받아 읽기 묶음에 머지)로 붙인다.
  어휘·예약 키·전략은 [`docs/list-filters.md`](docs/list-filters.md)가 정본
- 다중 값 파라미터: `sea_orm_crud::OneOrMany` 하나로 통일 — `?id=1` / `?id=1,2` / `?id=1&id=2`,
  빈 값은 빈 목록(=필터 안 걸림), 모르는 값은 400
- 범위 파라미터: `sea_orm_crud::Bounds` 하나로 통일 — `?p=3`(eq) / `?p=3..7`(between) /
  `?p=3..`(gte) / `?p=..7`(lte), 빈 값은 필터 안 걸림, 읽을 수 없는 값은 400
- 검색: `util::db::wrap_ilike(Col.into_expr(), text)`, FTS는 `tsquery(col, search)`
- 직렬화 충돌 감지: `util::db::is_serialization_failure(&err)` — 트랜잭션 재시도 판단

### 7. 세션·인증

- `UserSession::from_request_parts`로 자동 추출. `session.profile`에 id/username/full_name/role/organization/is_admin/api_key/expiry
- `session.authorize(id, username, expiry, db).await` — 로그인 처리. role/org/is_admin이 자동 동기화(`identify`)
- `UserEnvironment`(IP + UA 파싱)는 `Bans` 검사·`AccessLogs` 기록에 사용. ip_source는 settings에서 `ConnectInfo`/`Forwarded`/`XForwardedFor` 등 선택
- JWT 비밀키 저장소: cargo feature `jwt-secret-store-static`(설정 파일) vs `jwt-secret-store-aws-ssm`(`/{app}/{stage}/jwt-secret`)
- 세션 스토어: `session-store-redis` vs `session-store-aws-dynamodb` — 동일 인터페이스, settings 키만 다름

### 8. 메신저(socketioxide)

```rust
// Extension<SumidaMessenger> 받아 사용
messenger.of("/ns").org([1,2]).emit("event", &payload).await?;
messenger.user([user_id]).emit("event", &payload).await?;
```

room 자동 가입 규칙: 접속 사용자는 `org:N`, `user:N`, `admin`(관리자) 룸에 자동 합류. 다운스트림은 `Messenger` 를 감싼 래퍼 구조체에 자기 도메인 helper를 추가하면 된다.

### 9. 에러 처리

핸들러 시그니처는 `Result<T, SumidaError>`. `DbErr`, `anyhow::Error`, `JoinError`, `ImageError` 등이 `From` 변환됨. 도메인 특수 에러는 자체 `enum` + `Into<StatusCode>` + `IntoResponse` 구현해서 옆에 둔다.

`SumidaError`의 주요 매핑:
- `NoCredentials`/`NotAuthorized`/`InvalidJwt`/`InvalidApiKey`/`OnlyAdminAllowed` → 401
- `NoAccessPermission`/`NoServiceAvailable` → 403
- `NoData(_, _)` → 404
- `BadRequest(_)` → 400
- `Timeout(_)` → 408, `Busy(_)` → 429

### 10. 새 도메인 추가 체크리스트

1. `entity/`에 SeaORM 엔티티 + prelude 노출 (다운스트림이 자체 entity면 자체 prelude)
2. 마이그레이션 추가 (`sea_orm_migration`)
3. DTO 5종 세트 작성 — `Optional`/`Nullable`로 PATCH 시멘틱 정리, 목록이 있으면 `Filter` 옆에
   `FilterSchema` 선언 + 컨트롤러 읽기 묶음에 서술 라우트 머지(`filter_descriptor_router` /
   세션 가드만 있으면 `filter_descriptor_route`)
4. `.sz` 에 정의를 추가하고 서비스에 리스트 스코프와 단건 점검을 붙인다 — 부모 종속은 규칙 쪽에서 부모 관계로 푼다
5. `Controller`에 list/get/create/update/delete + 액션별 `service_ap!` + `user_guard!`
6. 라우터 nest 추가
7. **라우트 게이트 시드** — 새 데이터베이스는 게이트가 비어 있어 보호 라우트가 전부 거절된다. 다운스트림 initiator가 역할 하나에 기본 게이트를 심는 패턴을 따른다.

### 11. 변경 시 주의 사항

- 읽기 경로를 새로 만들 때 리스트 스코프를 빠뜨리기 쉽다. 공통 트레이트가 대신 채워 주지 않으므로, 새 조회 라우트마다 스코프가 붙었는지 확인한다.
- 소유자와 조직은 부여 대상이 아니다 — 컬럼에서 파생되므로 튜플로 심으면 규칙과 데이터가 어긋난다.
- 새 namespace를 도입하면 README/AGENTS, `.env.example`, 다운스트림 시드 스크립트까지 같이 갱신.
- 세션 cookie 속성(`name`/`secure`/`same_site`/`domain`/`path`) 변경은 클라이언트 호환성 영향 큼 — settings 키와 함께 다운스트림에도 공지.
- AWS 프로파일 전환(local↔aws) 시 `target/`, Redis, S3 버킷 캐시를 비워야 리뷰가 깨끗하게 재현됨.

---

## 테스트

유닛 테스트는 대상 모듈 옆 `#[cfg(test)]`. E2E는 `e2e/`, feature flag를 모듈 docs에 명시. DB-heavy 케이스는 `infra`의 SeaORM 인-메모리 헬퍼 사용(ad-hoc 연결 금지).

**통합 테스트 바이너리는 한 번에 하나만 돈다.** 하네스가 시작할 때 데이터베이스를 통째로 지우고
다시 세우는데, `cargo test` 는 테스트 바이너리 여러 개를 동시에 띄운다. 그래서 한 쪽이 도는 중에
다른 쪽이 그 테이블을 지우고, 매 실행마다 다른 테스트가 401·403·500 으로 떨어진다. 혼자 돌리면
통과하므로 진짜 실패와 구분이 되지 않는다. `scripts/test/suite.sh` 가 대상을 하나씩 넘긴다.

## 후속 작업

미완 작업·기술 부채는 [`TASKS.md`](TASKS.md)에 기록한다. 새 작업 지시를 받기 전에 한 번 훑어보면 누락된 follow-up이 있는지 확인할 수 있다.

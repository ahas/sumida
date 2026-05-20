# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 자주 쓰는 명령

- `cargo run --bin migration` — 엔티티/마이그레이션 수정 후 스키마 적용
- `cargo run --features full-local --example basic` — 로컬 스택(Redis+local-fs)으로 기본 예제 서버 실행
- `cargo run --features full-aws --example basic` — AWS 스택(DynamoDB+S3+SSM) 검증
- `cargo test` — 워크스페이스 전체 유닛/통합 테스트
- `cargo test --package e2e` — E2E 시나리오만
- `cargo test --features test-suite` — CI feature-gated 검사
- `cargo fmt`, `cargo clippy --all-targets --all-features` — 푸시 전 필수
- `cargo tarpaulin --out Html` — 커버리지 리포트

## 워크스페이스 구조

5 크레이트 + 루트 `sumida` re-export 크레이트:

| 크레이트 | 책임 |
|---|---|
| `api/` | Axum 컨트롤러·서비스·레이어·유틸. `Sumida::new().router()` 진입점, `ApiController`/`ApiService` 트레잇, `util::ap`(ReBAC), `util::pagination/s3/storage/db/...` |
| `entity/` | 공통 SeaORM 엔티티(users/roles/organizations/access_policies/api_keys/bans/access_logs/mail_forms/user_invitations/user_verifications/user_prefs/files/board_*) |
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

`sumida`를 의존성으로 추가한 서비스(예: `lab-app-server`)가 표준적으로 따르는 패턴.

### 1. 진입점 통합

```rust
#[derive(Clone)]
pub struct AppState { pub db: DatabaseConnection }
impl sumida::api::SumidaState for AppState {
  fn db(&self) -> &DatabaseConnection { &self.db }
}

let app = Router::new()
  .merge(sumida::api::Sumida::new().set_state(&state).router())  // 공통 라우트 nest
  .merge(my_domain::new_router(&state)?)                         // 도메인 라우트 nest
  .layer(sumida::infra::session::new_layer().await?)             // 세션
  .layer(sumida::api::layer::cors::new())                        // CORS
  .with_state(state);
```

`Sumida::new().router()`가 자동으로 nest하는 경로:
`/access-logs`, `/access-policies`, `/api-keys`, `/dashboard/monitoring`, `/bans`, `/organizations`, `/roles`, `/auth/session`, `/me`, `/users`, `/user-invitations`, `/user-verifications`, `/mail-forms`, `/boards`, `/board-groups`, `/board-topics`, `/board-posts`, `/board-files`, `/files`.

### 2. ReBAC 권한 모델 — `api/src/util/ap.rs`

단일 테이블 `access_policies(uuid, namespace, object, action[], effect, user)`로 운영. `*` 와일드카드 지원.

```
user 식별자 형식 : "user:admin" | "org:N" | "role:N" | "user:N" | "api:KEY" | "*"
effect           : "allow" | "deny"
```

호출 패턴 3종:

| 호출 | SQL | 용도 |
|---|---|---|
| `ap::ns(ns).who(&s).join(E,Col).apply(query)` | LATERAL JOIN으로 read 필터 | 목록 조회 가시성 제한 |
| `ap::ns(ns).who(&s).action("..").try_access(query, db)` | `SELECT EXISTS(...)` | 단일 행 수정/삭제 전 가드 |
| `ap::ns(ns).who(&s).object(id).action("..").try_check_allowed(db)` | 정책 매칭만 | 신규 객체 생성 사전 검사 |

다중 조건 AND: `ap::chain().push(...).push(...)`. 관리 헬퍼:
- `ap::edit().namespace(..).object(..).action(..).user(..).effect("allow").create(db)` / `.update(effect, db)` / `.delete(db)`
- `ap::for_users().user(ApUser::Role(id)).find_all(db)` — 특정 user의 정책 목록
- `ap::find_users("ns").object(id).action(..).all(db)` — 특정 객체에 권한 있는 사용자 역추적
- `.who(&session)`는 user/role/org/admin/api 식별자를 한 번에 펼쳐 넣어 줌

### 3. 서비스 표준 — `ApiService<A, E>`

```rust
pub struct MyService<S: SumidaState> { state: S, session: UserSession }

impl<S: SumidaState> ApiService<ApCondition, MyEntity> for MyService<S> {
  fn ap(&self) -> ApCondition {
    ap::ns("my:ns").who(&self.session).join(MyEntity, my::Column::Id)
  }
}
```

`ap()`만 구현하면 다음이 자동 제공:
- `self.select(query)` = `ap().action("read").apply(query)` — 목록/단건 조회용
- `self.try_access("update"|"delete"|.., ::find_by_id(id), &txn)` — 단일 행 보호

부모-자식 종속(예: site → structure → photo → inspection)은 `default_query_with(query)`를 오버라이드해 LEFT JOIN으로 부모 id를 끌어와 같은 namespace에 흘려보낸다. `lab-app-server`의 `DaqsInspectionService::default_query_with`가 표준 사례.

### 4. 컨트롤러 표준 — 액션별 `service_ap!` 분리

`api/src/controller/role.rs:74-99`가 기준 패턴.

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
- `service_ap!(state,"ns",action)` — `ap::ns(ns).object("service").action(action).who(&s).check_allowed(db)` 통과해야 진입
- `board_ap!(state, [board_id,] action)` — 게시판 단위 권한

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
- 검색: `util::db::wrap_ilike(Col.into_expr(), text)`, FTS는 `tsquery(col, search)`
- 직렬화 충돌 감지: `util::db::is_serialization_failure(&err)` — 트랜잭션 재시도 판단

### 7. 세션·인증

- `UserSession::from_request_parts`로 자동 추출. `session.profile`에 id/username/full_name/role/organization/is_admin/api_key/expiry
- `session.authorize(id, username, expiry, db).await` — 로그인 처리. role/org/is_admin이 자동 동기화(`identify`)
- `UserEnvironment`(IP + UA 파싱)는 `Bans` 검사·`AccessLogs` 기록에 사용. ip_source는 settings에서 `ConnectInfo`/`Forwarded`/`XForwardedFor` 등 선택
- JWT 비밀키 저장소: cargo feature `jwt-secret-store-static`(설정 파일) vs `jwt-secret-store-aws-ssm`(`/trainer/{stage}/jwt-secret`)
- 세션 스토어: `session-store-redis` vs `session-store-aws-dynamodb` — 동일 인터페이스, settings 키만 다름

### 8. 메신저(socketioxide)

```rust
// Extension<SumidaMessenger> 받아 사용
messenger.of("/ns").org([1,2]).emit("event", &payload).await?;
messenger.user([user_id]).emit("event", &payload).await?;
```

room 자동 가입 규칙: 접속 사용자는 `org:N`, `user:N`, `admin`(관리자) 룸에 자동 합류. 다운스트림은 래퍼 구조체에 site/photo 같은 도메인 helper를 추가하면 된다(lab-app의 `DaqsMessenger` 패턴).

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
3. DTO 5종 세트 작성 — `Optional`/`Nullable`로 PATCH 시멘틱 정리
4. `Service`에 `ApiService<ApCondition, MyEntity>` 구현 — 부모 종속이면 `default_query_with`로 부모 id LEFT JOIN
5. `Controller`에 list/get/create/update/delete + 액션별 `service_ap!` + `user_guard!`
6. 라우터 nest 추가
7. **운영 DB의 `access_policies`에 namespace 시드 정책 삽입** — 안 하면 `service_ap` 레이어에서 전부 403. `examples/basic` 또는 다운스트림의 initiator(`DEFAULT_SERVICES` 배열 패턴) 참고.

### 11. 변경 시 주의 사항

- `try_access`/`select`는 `ap_check[_i]` alias로 LATERAL JOIN을 깐다 — 쿼리에 동명 alias 있으면 `ApCondition::alias(Alias::new("..."))`로 바꾼다.
- `access_policies`에 직접 INSERT/DELETE할 때 user 컬럼은 반드시 `ApUser::*.to_string()` 형식으로 저장.
- 새 namespace를 도입하면 README/AGENTS, `.env.example`, 다운스트림 시드 스크립트까지 같이 갱신.
- 세션 cookie 속성(`name`/`secure`/`same_site`/`domain`/`path`) 변경은 클라이언트 호환성 영향 큼 — settings 키와 함께 다운스트림에도 공지.
- AWS 프로파일 전환(local↔aws) 시 `target/`, Redis, S3 버킷 캐시를 비워야 리뷰가 깨끗하게 재현됨.

---

## 테스트

유닛 테스트는 대상 모듈 옆 `#[cfg(test)]`. E2E는 `e2e/`, feature flag를 모듈 docs에 명시. DB-heavy 케이스는 `infra`의 SeaORM 인-메모리 헬퍼 사용(ad-hoc 연결 금지).

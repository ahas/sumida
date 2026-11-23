[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# SUMIDA

**S**calable **U**tility **M**odules **I**ntegration for **D**ynamic **A**pplication

Axum으로 만든 종합 Rust 웹 애플리케이션 프레임워크입니다. 견고한 인증과 인가, 현대적인 웹 API 기능을 갖추고, 확장 가능한 멀티 테넌트 애플리케이션을 위해 설계되었습니다.

## ✨ 기능

### 🏗️ 핵심 아키텍처
- **모듈형 워크스페이스**: 다섯 개의 라이브러리 크레이트를 루트 `sumida` 크레이트가 다시 내보냅니다
- **계층형 설계**: Controller → Service → Repository 패턴
- **타입 안전성**: SeaORM과 함께 Rust 타입 시스템을 폭넓게 활용합니다
- **프로덕션 대응**: 로컬 개발과 AWS 클라우드 배포를 모두 지원합니다

### 👥 사용자 관리 시스템
- **멀티 테넌트 조직**: 완전한 조직 관리
- **관계 기반 접근 제어(ReBAC)**: 컴파일된 규칙 집합, 조직 계층 클로저, 부팅 시 스키마 검증을 갖춘 Zanzibar 방식의 관계 튜플
- **사용자 수명 주기**: 가입, 인증, 초대, 차단 워크플로
- **활동 추적**: 상세한 사용자 로그와 감사 기록

### 💬 게시판 시스템
- **토론 포럼**: 모든 기능을 갖춘 게시판과 게시글 관리
- **계층 구조**: Categories → Boards → Posts → Comments
- **풍부한 상호작용**: 좋아요/싫어요, 조회 추적, 파일 첨부
- **콘텐츠 관리**: 임시 저장/게시 워크플로, 소프트 삭제 지원

### 💳 멤버십과 결제
- **요금제**: 구독형과 크레딧형 요금제(개인 또는 조직 범위), 무료 체험, 소프트 삭제
- **크레딧 원장**: 만료가 있는 FIFO 로트, 예약(승인 후 매입), 멱등한 지급/사용, 감사 기록
- **수명 주기**: 자동 갱신, 결제 재시도(dunning)와 유예 기간, 재활성화, 일할 계산을 포함한 기간 중 요금제 변경
- **이용 권한 게이트**: 기간별 할당량을 갖춘 구독/크레딧 혼합 게이트(`entitlement_guard!` 레이어)
- **결제**: 교체 가능한 PG 추상화(PortOne V2 우선 지원), 결제 시 쿠폰과 세금, Standard-Webhooks
- **스케줄러**: 만료, 갱신, 정리, 알림을 처리하는 백그라운드 유지 관리
- 필요한 정책 시드, 설정, 스케줄러 배포는 [`docs/membership.md`](docs/membership.md)를 참고하십시오

### 🔐 인증과 보안
- **다양한 인증 방식**: 세션 기반 인증과 JWT 토큰 인증
- **API 키 관리**: 서비스 간 인증
- **안전한 세션**: Redis 또는 AWS DynamoDB 세션 저장소
- **비밀번호 보안**: 올바른 솔트 처리를 갖춘 Argon2 해싱

### ☁️ 클라우드와 저장소
- **유연한 배포**: 로컬 Redis와 파일 저장소, 또는 AWS DynamoDB와 S3
- **이미지 처리**: 내장된 이미지 처리와 최적화
- **이메일 연동**: SMTP 기반 알림과 사용자 인증
- **설정 관리**: `SUMIDA_*` 환경 변수로 덮어쓰는 TOML 설정, AWS SSM 지원
- **모니터링**: 관리자 전용 대시보드 모니터링 API로, `/dashboard/monitoring`에서 스냅샷과 SSE 스트리밍으로
  서버의 실시간 상태를 제공합니다

## 🚀 빠른 시작

### 사전 요구 사항

- **Rust**: 최신 stable(2024 에디션)
- **PostgreSQL**: 12 이상
- **Redis**: 6 이상(로컬 개발용) 또는 AWS DynamoDB(프로덕션용)

### 설치

1. **저장소 클론**
```bash
git clone <repository-url>
cd sumida
```

2. **설정 준비**
```bash
cp settings.example.toml settings.toml
# Edit settings.toml; SUMIDA_* environment variables override it
```

3. **데이터베이스 준비**
```bash
# Run migrations
cargo run --bin migration
```

4. **배포 프로필 선택**

#### 🏠 로컬 개발
```bash
cargo run -p basic
```

#### ☁️ AWS 프로덕션
```bash
cargo run -p basic --no-default-features --features full-aws
```

## 🏗️ 아키텍처

### 워크스페이스 구조
```
sumida/
├── api/           # REST API controllers and business logic
├── entity/        # Database entities (SeaORM models)
├── infra/         # Infrastructure (DB, AWS, sessions, settings)
├── error/         # Error handling utilities
├── e2e/          # End-to-end testing framework
└── examples/     # Usage examples and demos
```

### 워크스페이스 의존성 그래프

```mermaid
graph LR
  sumida[(sumida root crate)]
  api[[api]]
  infra[[infra]]
  entity[[entity]]
  error[[error]]
  e2e[[e2e]]

  sumida --> api
  sumida --> infra
  sumida --> entity
  sumida --> error
  sumida --> e2e

  api --> infra
  api --> entity
  api --> error

  infra --> entity
  infra --> error

  entity --> error

  e2e --> api
  e2e --> infra
  e2e --> entity
  e2e --> error
```

### 크레이트별 모듈 맵

#### 루트 `sumida`
```mermaid
graph TD
  sumida_root[(sumida)]
  sumida_root --> api_crate[[api]]
  sumida_root --> infra_crate[[infra]]
  sumida_root --> entity_crate[[entity]]
  sumida_root --> error_crate[[error]]
  sumida_root --> e2e_crate[[e2e]]
```

#### `api` 크레이트 요청 흐름

<details>
<summary>사용자</summary>

```mermaid
graph TD
  u_get["GET /users<br/>(read)"] --> u_ctrl_list["UserController::list"]
  u_ctrl_list --> u_srv_list["UserService::list"]
  u_srv_list --> u_logic_list{{"SeaORM select<br/>+ pagination"}}
  u_logic_list --> u_ret_list[["List<UserListItem>"]]

  u_post["POST /users<br/>(create)"] --> u_ctrl_create["UserController::create"]
  u_ctrl_create --> u_srv_create["UserService::create"]
  u_srv_create --> u_logic_create{{"hash_password +<br/>Users::insert"}}
  u_logic_create --> u_ret_create[["UserCreated (id)"]]

  u_patch["PATCH /users/{id}<br/>(update)"] --> u_ctrl_update["UserController::update"]
  u_ctrl_update --> u_srv_update["UserService::update"]
  u_srv_update --> u_logic_update{{"txn + try_access +<br/>optional hash + update"}}
  u_logic_update --> u_ret_update[["UserUpdated"]]

  u_delete["DELETE /users/{id}<br/>(delete)"] --> u_ctrl_delete["UserController::delete"]
  u_ctrl_delete --> u_srv_delete["UserService::delete"]
  u_srv_delete --> u_logic_delete{{"txn + try_access +<br/>Users::delete_by_id"}}
  u_logic_delete --> u_ret_delete[["UserDeleted"]]
```

</details>

<details>
<summary>역할</summary>

```mermaid
graph TD
  r_get["GET /roles<br/>(read)"] --> r_ctrl_list["RoleController::list"]
  r_ctrl_list --> r_srv_list["RoleService::list"]
  r_srv_list --> r_logic_list{{"filter + pagination"}}
  r_logic_list --> r_ret_list[["List<RoleListItem>"]]

  r_post["POST /roles<br/>(create)"] --> r_ctrl_create["RoleController::create"]
  r_ctrl_create --> r_srv_create["RoleService::create"]
  r_srv_create --> r_logic_create{{"txn + insert role +<br/>assign access policies"}}
  r_logic_create --> r_ret_create[["RoleCreated (id)"]]

  r_patch["PATCH /roles/{id}<br/>(update)"] --> r_ctrl_update["RoleController::update"]
  r_ctrl_update --> r_srv_update["RoleService::update"]
  r_srv_update --> r_logic_update{{"txn + try_access +<br/>update policies + save"}}
  r_logic_update --> r_ret_update[["RoleUpdated"]]

  r_delete["DELETE /roles/{id}<br/>(delete)"] --> r_ctrl_delete["RoleController::delete"]
  r_ctrl_delete --> r_srv_delete["RoleService::delete"]
  r_srv_delete --> r_logic_delete{{"txn + try_access +<br/>clear policies + delete"}}
  r_logic_delete --> r_ret_delete[["RoleDeleted"]]
```

</details>

<details>
<summary>조직</summary>

```mermaid
graph TD
  o_get["GET /organizations<br/>(read)"] --> o_ctrl_list["OrganizationController::list"]
  o_ctrl_list --> o_srv_list["OrganizationService::list"]
  o_srv_list --> o_logic_list{{"filter + order_by + pagination"}}
  o_logic_list --> o_ret_list[["List<OrganizationListItem>"]]

  o_post["POST /organizations<br/>(create)"] --> o_ctrl_create["OrganizationController::create"]
  o_ctrl_create --> o_srv_create["OrganizationService::create"]
  o_srv_create --> o_logic_create{{"Organizations::insert"}}
  o_logic_create --> o_ret_create[["OrganizationCreated (id)"]]

  o_patch["PATCH /organizations/{id}<br/>(update)"] --> o_ctrl_update["OrganizationController::update"]
  o_ctrl_update --> o_srv_update["OrganizationService::update"]
  o_srv_update --> o_logic_update{{"txn + try_access +<br/>update model"}}
  o_logic_update --> o_ret_update[["OrganizationUpdated"]]

  o_delete["DELETE /organizations/{id}<br/>(delete)"] --> o_ctrl_delete["OrganizationController::delete"]
  o_ctrl_delete --> o_srv_delete["OrganizationService::delete"]
  o_srv_delete --> o_logic_delete{{"txn + try_access +<br/>Organizations::delete_by_id"}}
  o_logic_delete --> o_ret_delete[["OrganizationDeleted"]]
```

</details>

<details>
<summary>관계 튜플(인가)</summary>

인가는 요청마다 정책 행을 읽는 방식이 아니라, `relation_tuples` 위에서 컴파일된 규칙 집합(`api/src/authz/`)으로
평가합니다. sumida는 규칙 파일 `schema.sz` 하나를 담고 있으며, 소비자는 `Sumida::new(RULES)`로 자신의 규칙을
등록합니다. 규칙 집합 전체는 부팅 시 실제 데이터베이스와 대조해 검증합니다. 존재하지 않는 열을 읽는 규칙이 있으면
요청 시점에 틀린 답을 내는 대신 서버가 아예 시작되지 않습니다.

```mermaid
graph TD
  rt_get["GET /relation-tuples<br/>(read)"] --> rt_ctrl_list["RelationTupleController::list"]
  rt_ctrl_list --> rt_srv_list["RelationTupleService::list"]
  rt_srv_list --> rt_logic_list{{"Authz::filter + pagination"}}
  rt_logic_list --> rt_ret_list[["List&lt;RelationTupleListItem&gt;"]]

  rt_post["POST /relation-tuples<br/>(create, upsert)"] --> rt_ctrl_create["RelationTupleController::create"]
  rt_ctrl_create --> rt_srv_create["RelationTupleService::create"]
  rt_srv_create --> rt_logic_create{{"Authz::write —<br/>requires &lt;type&gt;#grant"}}
  rt_logic_create --> rt_ret_create[["RelationTupleCreated (id)"]]

  rt_delete["DELETE /relation-tuples/{id}"] --> rt_ctrl_delete["RelationTupleController::delete"]
  rt_ctrl_delete --> rt_srv_delete["RelationTupleService::delete"]
  rt_srv_delete --> rt_logic_delete{{"Authz::revoke —<br/>requires &lt;type&gt;#grant"}}
  rt_logic_delete --> rt_ret_delete[["RelationTupleDeleted"]]

  schema["GET /authz/schema"] --> schema_ctrl["AuthzController::schema"]
  schema_ctrl --> schema_logic{{"compiled vocabulary —<br/>no grant, no table names"}}

  me["GET /me/permissions"] --> me_ctrl["MeController::permissions"]
  me_ctrl --> me_logic{{"Authz::effective —<br/>evaluated, not read raw"}}
```

`effect` 필드는 없습니다. 거부는 행이 아니라 규칙 구문(`- blocked`)으로 표현합니다. `PATCH`도 없습니다. 튜플의
유니크 제약 때문에 `POST`가 곧 upsert입니다.

튜플을 쓰는 일 자체도 인가 대상입니다. 호출자는 대상 객체에 대해 `<resource_type>#grant`를 가지고 있어야 하므로,
권한을 부여할 자격이 없는 사람은 그 권한을 부여할 수 없습니다.

</details>

<details>
<summary>파일 저장소</summary>

```mermaid
graph TD
  f_get["GET /files<br/>(read)"] --> f_ctrl_list["FileController::list"]
  f_ctrl_list --> f_srv_list["FileService::list"]
  f_srv_list --> f_logic_list{{"filter + pagination + partial model"}}
  f_logic_list --> f_ret_list[["List<FileListItem>"]]

  f_post["POST /files<br/>(create/upload)"] --> f_ctrl_upload["FileController::upload"]
  f_ctrl_upload --> f_srv_upload["FileService::upload"]
  f_srv_upload --> f_logic_upload{{"insert metadata +<br/>storage.store + status update"}}
  f_logic_upload --> f_ret_upload[["FileUploaded (id)"]]

  f_download["GET /files/{id}/download"] --> f_ctrl_download["FileController::download"]
  f_ctrl_download --> f_srv_download["FileService::download"]
  f_srv_download --> f_logic_download{{"storage.download"}}
  f_logic_download --> f_ret_download[["Response<Body>"]]

  f_delete["DELETE /files/{id}<br/>(delete)"] --> f_ctrl_delete["FileController::delete"]
  f_ctrl_delete --> f_srv_delete["FileService::delete"]
  f_srv_delete --> f_logic_delete{{"txn + fetch file +<br/>optional storage.delete + delete row"}}
  f_logic_delete --> f_ret_delete[["FileDeleted"]]
```

</details>

<details>
<summary>게시판 시스템</summary>

```mermaid
graph TD
  bg_get["GET /board-groups<br/>(read)"] --> bg_ctrl_list["BoardGroupController::list"]
  bg_ctrl_list --> bg_srv_list["BoardGroupService::list"]
  bg_srv_list --> bg_logic_list{{"filter + paginate<br/>BoardGroups::find"}}
  bg_logic_list --> bg_ret_list[["List<BoardGroupListItem>"]]

  bg_post["POST /board-groups<br/>(create)"] --> bg_ctrl_create["BoardGroupController::create"]
  bg_ctrl_create --> bg_srv_create["BoardGroupService::create"]
  bg_srv_create --> bg_logic_create{{"BoardGroups::insert"}}
  bg_logic_create --> bg_ret_create[["BoardGroupCreated (id)"]]

  bg_patch["PATCH /board-groups/{id}<br/>(update)"] --> bg_ctrl_update["BoardGroupController::update"]
  bg_ctrl_update --> bg_srv_update["BoardGroupService::update"]
  bg_srv_update --> bg_logic_update{{"txn + try_access +<br/>BoardGroups::update"}}
  bg_logic_update --> bg_ret_update[["BoardGroupUpdated"]]

  bg_delete["DELETE /board-groups/{id}<br/>(delete)"] --> bg_ctrl_delete["BoardGroupController::delete"]
  bg_ctrl_delete --> bg_srv_delete["BoardGroupService::delete"]
  bg_srv_delete --> bg_logic_delete{{"txn + try_access +<br/>BoardGroups::delete_by_id"}}
  bg_logic_delete --> bg_ret_delete[["BoardGroupDeleted"]]
```

```mermaid
graph TD
  b_get["GET /boards<br/>(read)"] --> b_ctrl_list["BoardController::list"]
  b_ctrl_list --> b_srv_list["BoardService::list"]
  b_srv_list --> b_logic_list{{"txn + filter +<br/>paginate Boards"}}
  b_logic_list --> b_ret_list[["List<BoardListItem>"]]

  b_post["POST /boards<br/>(create)"] --> b_ctrl_create["BoardController::create"]
  b_ctrl_create --> b_srv_create["BoardService::create"]
  b_srv_create --> b_logic_create{{"Boards::insert"}}
  b_logic_create --> b_ret_create[["BoardCreated (id)"]]

  b_patch["PATCH /boards/{id}<br/>(update)"] --> b_ctrl_update["BoardController::update"]
  b_ctrl_update --> b_srv_update["BoardService::update"]
  b_srv_update --> b_logic_update{{"txn + try_access +<br/>Boards::update"}}
  b_logic_update --> b_ret_update[["BoardUpdated"]]

  b_delete["DELETE /boards/{id}<br/>(delete)"] --> b_ctrl_delete["BoardController::delete"]
  b_ctrl_delete --> b_srv_delete["BoardService::delete"]
  b_srv_delete --> b_logic_delete{{"txn + try_access +<br/>Boards::delete_by_id"}}
  b_logic_delete --> b_ret_delete[["BoardDeleted"]]
```

```mermaid
graph TD
  bt_get["GET /board-topics<br/>(read)"] --> bt_ctrl_list["BoardTopicController::list"]
  bt_ctrl_list --> bt_srv_list["BoardTopicService::list"]
  bt_srv_list --> bt_logic_list{{"filter + paginate<br/>BoardTopics::find"}}
  bt_logic_list --> bt_ret_list[["List<BoardTopicListItem>"]]

  bt_post["POST /board-topics<br/>(create)"] --> bt_ctrl_create["BoardTopicController::create"]
  bt_ctrl_create --> bt_srv_create["BoardTopicService::create"]
  bt_srv_create --> bt_logic_create{{"BoardTopics::insert"}}
  bt_logic_create --> bt_ret_create[["BoardTopicCreated (id)"]]

  bt_patch["PATCH /board-topics/{id}<br/>(update)"] --> bt_ctrl_update["BoardTopicController::update"]
  bt_ctrl_update --> bt_srv_update["BoardTopicService::update"]
  bt_srv_update --> bt_logic_update{{"txn + try_access +<br/>BoardTopics::update"}}
  bt_logic_update --> bt_ret_update[["BoardTopicUpdated"]]

  bt_delete["DELETE /board-topics/{id}<br/>(delete)"] --> bt_ctrl_delete["BoardTopicController::delete"]
  bt_ctrl_delete --> bt_srv_delete["BoardTopicService::delete"]
  bt_srv_delete --> bt_logic_delete{{"txn + try_access +<br/>BoardTopics::delete_by_id"}}
  bt_logic_delete --> bt_ret_delete[["BoardTopicDeleted"]]
```

```mermaid
graph TD
  bp_get["GET /board-posts<br/>(read)"] --> bp_ctrl_list["BoardPostController::list"]
  bp_ctrl_list --> bp_srv_list["BoardPostService::list"]
  bp_srv_list --> bp_logic_list{{"conditional columns +<br/>FTS/snippet + pagination"}}
  bp_logic_list --> bp_ret_list[["List<BoardPostListItem>"]]

  bp_get_one["GET /board-posts/{id}"] --> bp_ctrl_get["BoardPostController::get"]
  bp_ctrl_get --> bp_srv_get["BoardPostService::get"]
  bp_srv_get --> bp_logic_get{{"txn + load content +<br/>attached files"}}
  bp_logic_get --> bp_ret_get[["GetBoardPost"]]

  bp_post["POST /board-posts<br/>(create)"] --> bp_ctrl_create["BoardPostController::create"]
  bp_ctrl_create --> bp_srv_create["BoardPostService::create"]
  bp_srv_create --> bp_logic_create{{"ap::chain checks +<br/>hash password + link files"}}
  bp_logic_create --> bp_ret_create[["BoardPostCreated (id)"]]

  bp_draft["POST /board-posts/draft"] --> bp_ctrl_draft["BoardPostController::create_draft"]
  bp_ctrl_draft --> bp_srv_draft["BoardPostService::create_draft"]
  bp_srv_draft --> bp_logic_draft{{"create draft record +<br/>optional password hash"}}
  bp_logic_draft --> bp_ret_draft[["BoardPostCreated (draft id)"]]

  bp_patch["PATCH /board-posts/{id}<br/>(update)"] --> bp_ctrl_update["BoardPostController::update"]
  bp_ctrl_update --> bp_srv_update["BoardPostService::update"]
  bp_srv_update --> bp_logic_update{{"try_access +<br/>BoardPosts::update + link files"}}
  bp_logic_update --> bp_ret_update[["BoardPostUpdated"]]

  bp_delete["DELETE /board-posts/{id}<br/>(delete)"] --> bp_ctrl_delete["BoardPostController::delete"]
  bp_ctrl_delete --> bp_srv_delete["BoardPostService::delete"]
  bp_srv_delete --> bp_logic_delete{{"try_access + cascade cleanup"}}
  bp_logic_delete --> bp_ret_delete[["BoardPostDeleted"]]
```

```mermaid
graph TD
  bf_get["GET /board-files<br/>(read)"] --> bf_ctrl_list["BoardFileController::list"]
  bf_ctrl_list --> bf_srv_list["BoardFileService::list"]
  bf_srv_list --> bf_logic_list{{"join Files + filter + paginate"}}
  bf_logic_list --> bf_ret_list[["List<BoardFileListItem>"]]

  bf_get_one["GET /board-files/{post_id}/{file_id}"] --> bf_ctrl_get["BoardFileController::get"]
  bf_ctrl_get --> bf_srv_get["BoardFileService::get"]
  bf_srv_get --> bf_logic_get{{"join Files + select metadata"}}
  bf_logic_get --> bf_ret_get[["GetBoardFile"]]

  bf_post["POST /board-files<br/>(upload)"] --> bf_ctrl_upload["BoardFileController::upload"]
  bf_ctrl_upload --> bf_srv_upload["BoardFileService::upload"]
  bf_srv_upload --> bf_logic_upload{{"board guard + size checks +<br/>delegate to FileService::upload"}}
  bf_logic_upload --> bf_ret_upload[["FileUploaded (id)"]]

  bf_delete["DELETE /board-files<br/>(delete)"] --> bf_ctrl_delete["BoardFileController::delete"]
  bf_ctrl_delete --> bf_srv_delete["BoardFileService::delete"]
  bf_srv_delete --> bf_logic_delete{{"BoardFiles::delete_many +<br/>optional FileService::delete"}}
  bf_logic_delete --> bf_ret_delete[["BoardFileDeleted"]]
```

</details>

### 기능 플래그

| 기능 | 설명 |
|---------|-------------|
| `full-local` | 로컬 개발용 전체 스택(Redis + 로컬 저장소) |
| `full-aws` | 프로덕션 AWS 스택(DynamoDB + S3 + SSM) |
| `test-suite` | 테스트 구성 |

## 📖 사용법

### 기본 통합

`examples/basic`이 완전한 서버이며, 그 핵심은 다음과 같습니다.

```rust
use std::net::SocketAddr;

use axum::{Extension, Router, routing};
use sea_orm::DatabaseConnection;
use sumida::api::{Sumida, SumidaState};

#[derive(Clone)]
pub struct AppState {
    db: DatabaseConnection,
}

impl SumidaState for AppState {
    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize infrastructure
    let settings = sumida::infra::settings::get();
    sumida::infra::tracing::subscribe();
    let state = AppState { db: sumida::infra::db::connect().await };

    let (messenger_layer, messenger) = sumida::infra::messenger::new_layer(state.db.clone());
    let session_store = sumida::infra::session::new_store(&settings.session).await?;

    // Build application; `&[]` declares no rule files beyond sumida's own
    let app = Router::new()
        .merge(Sumida::new(&[]).set_state(&state).router())
        .layer(messenger_layer)
        .layer(sumida::api::layer::session::new(session_store).await?)
        .layer(sumida::api::layer::cors::new())
        .layer(Extension(messenger))
        .layer(Extension(settings))
        .with_state(state)
        .route("/", routing::get(health_check))
        .layer(settings.session.ip_source.clone().into_extension())
        .into_make_service_with_connect_info::<SocketAddr>();

    // Start server
    let listener = tokio::net::TcpListener::bind(&settings.server.host).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}
```

### API 엔드포인트

프레임워크는 폭넓은 REST API를 제공합니다.

- **인증**: `/auth/*` - 로그인, 로그아웃, 가입
- **사용자**: `/users/*` - 사용자 관리와 프로필
- **조직**: `/organizations/*` - 멀티 테넌트 조직 관리
- **역할**: `/roles/*` - 역할과 권한 관리
- **게시판**: `/boards/*` - 토론 포럼 관리
- **게시글**: `/board-posts/*` - 콘텐츠 작성과 관리
- **API 키**: `/api-keys/*` - 서비스 인증

## 🧪 테스트

### 테스트 실행
```bash
# Integration tests, one test binary at a time (each one resets the test database)
bash scripts/test/suite.sh

# Unit tests
cargo test

# End-to-end tests
cargo test --package e2e

# With specific features
cargo test --features test-suite
```

### 테스트 커버리지
```bash
# Install cargo-tarpaulin for coverage
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## 🔧 설정

### 설정 파일

설정은 Figment로 읽습니다. `settings.toml`(경로는 `SUMIDA_SETTINGS`, 기본값 `./settings.toml`)이 바탕이 되고,
`SUMIDA_*` 환경 변수가 그 위에 덮어씁니다. 변수 이름에서 `__`는 중첩을, 단일 `_`는 `-`를 뜻하므로
`SUMIDA_DATABASE__URL`은 `database.url`을 설정합니다. 모든 섹션은 `settings.example.toml`에 나와 있습니다.

#### 로컬 개발
```toml
[server]
host = "0.0.0.0:7997"
stage = "dev"

[database]
url = "postgres://user:password@localhost:5432/sumida"

[session.redis]
url = "redis://localhost:6379/0"

[jwt]
secret = "REPLACE_WITH_JWT_SECRET"  # at least 32 bytes

[smtp]
server = "smtp.example.com"
```

#### AWS 프로덕션
```toml
[database]
url = "postgres://user:password@rds-endpoint:5432/sumida"

[session.dynamodb]
table = "sumida-sessions"

[storage.s3]
bucket = "sumida-storage"

[jwt]
ssm_path = "/sumida/prod/jwt-secret"  # default: /sumida/{stage}/jwt-secret
```

#### 보안 설정

모두 선택 항목이며, 적힌 값이 키를 비워 두었을 때의 기본값이다. 각 값이 무엇을 다스리는지는
[`docs/authentication.md`](docs/authentication.md)에 있다.

```toml
[session.cookie]
same_site = "lax"            # "lax"(기본) | "strict" | "none". 그 밖의 값이면 기동을 거부한다

[security.verification]
max_failed_attempts = 5      # 인증 코드 하나를 제시할 수 있는 횟수. 이 횟수에 닿으면 코드가 소진된다
issue_limit = 5              # 창 하나 동안 계정별·주소별로 요청할 수 있는 코드 수
attempt_limit = 20           # 창 하나 동안 계정별·주소별로 제시할 수 있는 코드 수
window = "1h"
```

헤더 자격(`Authorization: Bearer …` 또는 `ApiKey …`)은 그 요청 하나만 인증하며 세션 쿠키를 만들지
않는다. 인증 코드는 `mail-form` 기능이 켜진 빌드에서 `method = "email"` 일 때만 저장된다.

### 기능 구성

Cargo feature로 빌드를 조정합니다.

```toml
[dependencies]
sumida = { path = ".", features = ["full-aws", "image"] }
```

## 📊 성능과 모니터링

### 내장 관측 기능
- **구조화된 로깅**: 수준을 설정할 수 있는 Tracing 연동
- **헬스 체크**: 로드 밸런서용 기본 헬스 엔드포인트
- **오류 추적**: 맥락을 담은 체계적인 오류 처리

### 권장 모니터링 스택
- **메트릭**: Prometheus + Grafana
- **로깅**: ELK Stack 또는 AWS CloudWatch
- **트레이싱**: Jaeger 또는 AWS X-Ray
- **가동 시간**: 자체 헬스 체크 모니터링

## 🚀 배포

### 로컬 개발
```bash
# Start PostgreSQL and Redis first, then apply the schema
cargo run --bin migration

# Run application
cargo run -p basic
```

### Docker 배포
```dockerfile
FROM rust:1-bookworm AS builder
# .cargo/config.toml links with mold.
RUN apt-get update && apt-get install -y mold
WORKDIR /app
COPY . .
RUN cargo build --release -p basic --no-default-features --features full-aws

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/app /usr/local/bin/app
CMD ["app"]
```

### AWS 배포
권장 AWS 서비스는 다음과 같습니다.
- **컴퓨팅**: ECS Fargate 또는 EKS
- **데이터베이스**: RDS PostgreSQL
- **세션**: DynamoDB
- **저장소**: S3
- **비밀 값**: AWS Systems Manager Parameter Store

## 🤝 기여하기

### 개발 환경 준비
1. Rust 툴체인과 필요한 의존성을 설치합니다
2. 로컬 PostgreSQL과 Redis 인스턴스를 준비합니다
3. `settings.example.toml`을 `settings.toml`로 복사해 설정합니다
4. 데이터베이스 마이그레이션을 실행합니다: `cargo run --bin migration`
5. 개발 서버를 시작합니다: `cargo run -p basic`

### 코드 스타일
- Rust 2024 에디션 관례를 따릅니다
- `cargo +nightly fmt`로 포맷합니다. `rustfmt.toml`이 nightly 전용 옵션을 씁니다
- 린트는 `cargo clippy`로 실행합니다
- 테스트가 통과하는지 확인합니다: `cargo test`

### Pull Request 가이드라인
- 내용을 알 수 있는 커밋 메시지를 씁니다
- 새 기능에는 테스트를 함께 넣습니다
- API가 바뀌면 문서도 갱신합니다
- CI/CD 파이프라인이 통과하는지 확인합니다

## 📝 라이선스

이 프로젝트는 MIT 라이선스를 따릅니다. 자세한 내용은 [LICENSE](LICENSE) 파일을 참고하십시오.

## 🔗 관련 프로젝트

- [SeaORM](https://github.com/SeaQL/sea-orm) - 데이터베이스 ORM
- [Axum](https://github.com/tokio-rs/axum) - 웹 프레임워크
- [Tower](https://github.com/tower-rs/tower) - 미들웨어 생태계

---

**기술 지표**: PostgreSQL + SeaORM • Axum 0.8.4 • 프로덕션 대응 아키텍처

정책, 설정, 규칙 언어의 자세한 내용은 [`docs/`](docs/) 아래의 문서와 [CLAUDE.md](CLAUDE.md)를 참고하십시오.

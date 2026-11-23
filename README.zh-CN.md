[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# SUMIDA

**S**calable **U**tility **M**odules **I**ntegration for **D**ynamic **A**pplication

一个基于 Axum 构建的综合性 Rust Web 应用框架，面向可扩展的多租户应用，提供健全的认证、授权以及现代 Web API 能力。

## ✨ 功能

### 🏗️ 核心架构
- **模块化工作区**：五个库 crate，由根 `sumida` crate 统一重新导出
- **分层设计**：Controller → Service → Repository 模式
- **类型安全**：结合 SeaORM 全面利用 Rust 的类型系统
- **生产就绪**：同时支持本地开发与 AWS 云端部署

### 👥 用户管理系统
- **多租户组织**：完整的组织管理
- **基于关系的访问控制（ReBAC）**：Zanzibar 风格的关系元组，配有编译后的规则集、组织层级闭包以及启动时的 schema 校验
- **用户生命周期**：注册、验证、邀请和封禁流程
- **活动追踪**：详尽的用户日志与审计记录

### 💬 论坛系统
- **讨论区**：功能完备的版块与帖子管理
- **层级结构**：Categories → Boards → Posts → Comments
- **丰富的互动**：点赞/点踩、浏览量统计、文件附件
- **内容管理**：草稿/发布流程，支持软删除

### 💳 会员与计费
- **套餐**：订阅套餐与积分套餐（个人或组织范围）、免费试用、软删除
- **积分账本**：带过期时间的 FIFO 批次、预留（先授权后扣款）、幂等的发放/消耗、审计
- **生命周期**：自动续费、催缴（dunning）与宽限期、重新激活、周期内按比例计费的套餐变更
- **权益闸门**：订阅/积分混合闸门，支持按周期配额（`entitlement_guard!` 层）
- **支付**：可插拔的支付网关（PG）抽象（优先支持 PortOne V2）、结账时的优惠券与税费、Standard-Webhooks
- **调度器**：处理过期、续费、清理和通知的后台维护任务
- 所需的策略种子数据、配置和调度器部署，请参阅 [`docs/membership.md`](docs/membership.md)

### 🔐 认证与安全
- **多种认证方式**：基于会话的认证与 JWT 令牌认证
- **API 密钥管理**：服务间认证
- **安全会话**：Redis 或 AWS DynamoDB 会话存储
- **密码安全**：正确处理盐值的 Argon2 哈希

### ☁️ 云与存储
- **灵活部署**：本地 Redis 加文件存储，或 AWS DynamoDB 加 S3
- **图像处理**：内置图像处理与优化
- **邮件集成**：基于 SMTP 的通知与用户验证
- **配置管理**：TOML 配置，可由 `SUMIDA_*` 环境变量覆盖，支持 AWS SSM
- **监控**：仅限管理员的仪表盘监控 API，在 `/dashboard/monitoring` 通过快照和 SSE 流
  提供服务器的实时状态

## 🚀 快速开始

### 前置条件

- **Rust**：最新 stable 版本（2024 edition）
- **PostgreSQL**：12 及以上
- **Redis**：6 及以上（本地开发）或 AWS DynamoDB（生产环境）

### 安装

1. **克隆仓库**
```bash
git clone <repository-url>
cd sumida
```

2. **准备配置**
```bash
cp settings.example.toml settings.toml
# Edit settings.toml; SUMIDA_* environment variables override it
```

3. **准备数据库**
```bash
# Run migrations
cargo run --bin migration
```

4. **选择部署配置**

#### 🏠 本地开发
```bash
cargo run -p basic
```

#### ☁️ AWS 生产环境
```bash
cargo run -p basic --no-default-features --features full-aws
```

## 🏗️ 架构

### 工作区结构
```
sumida/
├── api/           # REST API controllers and business logic
├── entity/        # Database entities (SeaORM models)
├── infra/         # Infrastructure (DB, AWS, sessions, settings)
├── error/         # Error handling utilities
├── e2e/          # End-to-end testing framework
└── examples/     # Usage examples and demos
```

### 工作区依赖关系图

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

### 各 crate 的模块图

#### 根 `sumida`
```mermaid
graph TD
  sumida_root[(sumida)]
  sumida_root --> api_crate[[api]]
  sumida_root --> infra_crate[[infra]]
  sumida_root --> entity_crate[[entity]]
  sumida_root --> error_crate[[error]]
  sumida_root --> e2e_crate[[e2e]]
```

#### `api` crate 请求流程

<details>
<summary>用户</summary>

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
<summary>角色</summary>

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
<summary>组织</summary>

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
<summary>关系元组（授权）</summary>

授权并不是在每次请求时读取策略行，而是基于 `relation_tuples`，由编译后的规则集（`api/src/authz/`）进行求值。
sumida 自带一个规则文件 `schema.sz`，使用方通过 `Sumida::new(RULES)` 注册自己的规则。整个规则集会在启动时对照
实际数据库进行校验：如果某条规则读取了不存在的列，服务器会直接拒绝启动，而不是在请求时给出错误的结果。

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

没有 `effect` 字段：拒绝是一种规则构造（`- blocked`），而不是一行数据。也没有 `PATCH`：元组的唯一约束使得
`POST` 本身就是 upsert。

写入元组本身也需要授权：调用方必须对目标对象持有 `<resource_type>#grant`，因此没有授予权限资格的人无法授予
该权限。

</details>

<details>
<summary>文件存储</summary>

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
<summary>论坛系统</summary>

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

### 功能开关

| 功能 | 说明 |
|---------|-------------|
| `full-local` | 完整的本地开发栈（Redis + 本地存储） |
| `full-aws` | 生产环境 AWS 栈（DynamoDB + S3 + SSM） |
| `test-suite` | 测试配置 |

## 📖 使用

### 基本集成

`examples/basic` 是一个完整的服务器，其核心如下：

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

### API 端点

框架提供了全面的 REST API：

- **认证**：`/auth/*` - 登录、登出、注册
- **用户**：`/users/*` - 用户管理与个人资料
- **组织**：`/organizations/*` - 多租户组织管理
- **角色**：`/roles/*` - 角色与权限管理
- **版块**：`/boards/*` - 讨论区管理
- **帖子**：`/board-posts/*` - 内容创建与管理
- **API 密钥**：`/api-keys/*` - 服务认证

## 🧪 测试

### 运行测试
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

### 测试覆盖率
```bash
# Install cargo-tarpaulin for coverage
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## 🔧 配置

### 配置文件

配置通过 Figment 加载：以 `settings.toml`（路径取自 `SUMIDA_SETTINGS`，默认 `./settings.toml`）为基础，
再由 `SUMIDA_*` 环境变量覆盖。变量名中 `__` 表示嵌套，单个 `_` 表示 `-`，因此
`SUMIDA_DATABASE__URL` 设置的是 `database.url`。所有配置段都列在 `settings.example.toml` 中。

#### 本地开发
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

#### AWS 生产环境
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

#### 安全设置

均为可选项，所示值即未设置该键时的默认值。各项的作用见
[`docs/authentication.md`](docs/authentication.md)。

```toml
[server.cors]
allow_origins = []           # 可携带访客 Cookie 获得应答的其他源页面;"*" 会被忽略

[session.cookie]
same_site = "lax"            # "lax"(默认) | "strict" | "none";其他值会拒绝启动

[security.verification]
max_failed_attempts = 5      # 一个验证码可提交的次数,达到后该验证码作废
issue_limit = 5              # 每个窗口内按账户、按地址可请求的验证码数
attempt_limit = 20           # 每个窗口内按账户、按地址可提交的验证码数
window = "1h"
```

请求头凭据(`Authorization: Bearer …` 或 `ApiKey …`)只认证当次请求,不会设置会话 Cookie。
验证码只在启用 `mail-form` 功能的构建中、且 `method = "email"` 时才会保存。以 Cookie 认证、
会改变状态的请求(`GET`、`HEAD`、`OPTIONS` 以外的方法),若其 `Origin`(缺失时为 `Referer`)所指的页面
既不是服务器自身的,也不在 `server.cors.allow_origins` 之中,即被拒绝。

### 功能配置

通过 Cargo feature 定制构建：

```toml
[dependencies]
sumida = { path = ".", features = ["full-aws", "image"] }
```

## 📊 性能与监控

### 内置可观测性
- **结构化日志**：集成 Tracing，日志级别可配置
- **健康检查**：供负载均衡器使用的基础健康端点
- **错误追踪**：带上下文的完善错误处理

### 推荐的监控栈
- **指标**：Prometheus + Grafana
- **日志**：ELK Stack 或 AWS CloudWatch
- **链路追踪**：Jaeger 或 AWS X-Ray
- **可用性**：自定义健康检查监控

## 🚀 部署

### 本地开发
```bash
# Start PostgreSQL and Redis first, then apply the schema
cargo run --bin migration

# Run application
cargo run -p basic
```

### Docker 部署
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

### AWS 部署
推荐使用的 AWS 服务：
- **计算**：ECS Fargate 或 EKS
- **数据库**：RDS PostgreSQL
- **会话**：DynamoDB
- **存储**：S3
- **密钥**：AWS Systems Manager Parameter Store

## 🤝 参与贡献

### 开发环境准备
1. 安装 Rust 工具链及所需依赖
2. 准备本地 PostgreSQL 和 Redis 实例
3. 将 `settings.example.toml` 复制为 `settings.toml` 并完成配置
4. 运行数据库迁移：`cargo run --bin migration`
5. 启动开发服务器：`cargo run -p basic`

### 代码风格
- 遵循 Rust 2024 edition 的惯例
- 使用 `cargo +nightly fmt` 格式化：`rustfmt.toml` 用到了仅 nightly 支持的选项
- 使用 `cargo clippy` 进行 lint 检查
- 确保测试通过：`cargo test`

### Pull Request 指南
- 编写能说明改动内容的提交信息
- 新功能需附带测试
- API 变更时同步更新文档
- 确保 CI/CD 流水线通过

## 📝 许可证

本项目基于 MIT 许可证发布，详情请参阅 [LICENSE](LICENSE) 文件。

## 🔗 相关项目

- [SeaORM](https://github.com/SeaQL/sea-orm) - 数据库 ORM
- [Axum](https://github.com/tokio-rs/axum) - Web 框架
- [Tower](https://github.com/tower-rs/tower) - 中间件生态

---

**技术指标**：PostgreSQL + SeaORM • Axum 0.8.4 • 生产就绪的架构

关于策略、配置和规则语言的详细说明，请参阅 [`docs/`](docs/) 下的文档以及 [CLAUDE.md](CLAUDE.md)。

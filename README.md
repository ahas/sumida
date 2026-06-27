# SUMIDA

**S**calable **U**tility **M**odules **I**ntegration for **D**ynamic **A**pplication

A comprehensive Rust web application framework built with Axum, designed for scalable, multi-tenant applications with robust authentication, authorization, and modern web API capabilities.

## ✨ Features

### 🏗️ Core Architecture
- **Modular Workspace**: Clean separation across 6 specialized crates
- **Layered Design**: Controller → Service → Repository pattern
- **Type-Safe**: Comprehensive use of Rust's type system with SeaORM
- **Production-Ready**: Built for both local development and AWS cloud deployment

### 👥 User Management System
- **Multi-tenant Organizations**: Complete organization management
- **Role-Based Access Control (RBAC)**: Fine-grained permissions system
- **User Lifecycle**: Registration, verification, invitation, and blocking workflows
- **Activity Tracking**: Comprehensive user logs and audit trails

### 💬 Board System
- **Discussion Forums**: Full-featured board and post management
- **Hierarchical Organization**: Categories → Boards → Posts → Comments
- **Rich Interactions**: Like/dislike system, view tracking, file attachments
- **Content Management**: Draft/published workflows, soft delete support

### 💳 Membership & Billing
- **Plans**: Subscription and credit plans (personal or organization scope), free trials, soft-delete
- **Credit Ledger**: FIFO expiring lots, reservations (authorize-capture), idempotent grant/consume, audit
- **Lifecycle**: Auto-renew, dunning + grace, reactivation, mid-period plan change with proration
- **Entitlement Gate**: Hybrid subscription/credit gate with per-period quotas (`entitlement_guard!` layer)
- **Payments**: Pluggable PG abstraction (PortOne V2 first-class), coupons + tax at checkout, Standard-Webhooks
- **Scheduler**: Background maintenance for expiry, renewal, sweeps, and notifications
- See [`docs/membership.md`](docs/membership.md) for required policy seeds, settings, and scheduler deployment

### 🔐 Authentication & Security
- **Multiple Auth Methods**: Session-based and JWT token authentication
- **API Key Management**: Service-to-service authentication
- **Secure Sessions**: Redis or AWS DynamoDB session storage
- **Password Security**: Argon2 hashing with proper salt handling

### ☁️ Cloud & Storage
- **Deployment Flexibility**: Local Redis + file storage OR AWS DynamoDB + S3
- **Image Processing**: Built-in image handling and optimization
- **Email Integration**: SMTP-based notifications and user verification
- **Configuration Management**: Environment-based settings with AWS SSM support
- **Monitoring**: Admin-only dashboard monitoring API with snapshots and SSE streaming at
  `/dashboard/monitoring` for live server status

## 🚀 Quick Start

### Prerequisites

- **Rust**: Latest stable (2024 edition)
- **PostgreSQL**: 12+ 
- **Redis**: 6+ (for local development) OR AWS DynamoDB (for production)

### Installation

1. **Clone the repository**
```bash
git clone <repository-url>
cd sumida
```

2. **Set up environment**
```bash
cp .env.example .env
# Edit .env with your configuration
```

3. **Database setup**
```bash
# Run migrations
cargo run --bin migration
```

4. **Choose your deployment profile**

#### 🏠 Local Development
```bash
cargo run --features full-local --example basic
```

#### ☁️ AWS Production
```bash
cargo run --features full-aws --example basic
```

## 🏗️ Architecture

### Workspace Structure
```
sumida/
├── api/           # REST API controllers and business logic
├── entity/        # Database entities (SeaORM models)
├── infra/         # Infrastructure (DB, AWS, sessions, settings)
├── error/         # Error handling utilities
├── e2e/          # End-to-end testing framework
└── examples/     # Usage examples and demos
```

### Workspace Dependency Graph

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

### Crate-Level Module Maps

#### Root `sumida`
```mermaid
graph TD
  sumida_root[(sumida)]
  sumida_root --> api_crate[[api]]
  sumida_root --> infra_crate[[infra]]
  sumida_root --> entity_crate[[entity]]
  sumida_root --> error_crate[[error]]
  sumida_root --> e2e_crate[[e2e]]
```

#### `api` crate request flows

<details>
<summary>Users</summary>

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
<summary>Roles</summary>

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
<summary>Organizations</summary>

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
<summary>Access Policies</summary>

```mermaid
graph TD
  ap_get["GET /access-policies<br/>(read)"] --> ap_ctrl_list["AccessPolicyController::list"]
  ap_ctrl_list --> ap_srv_list["AccessPolicyService::list"]
  ap_srv_list --> ap_logic_list{{"AP guard + filter + pagination"}}
  ap_logic_list --> ap_ret_list[["List<AccessPolicyListItem>"]]

  ap_post["POST /access-policies<br/>(create)"] --> ap_ctrl_create["AccessPolicyController::create"]
  ap_ctrl_create --> ap_srv_create["AccessPolicyService::create"]
  ap_srv_create --> ap_logic_create{{"AccessPolicies::insert"}}
  ap_logic_create --> ap_ret_create[["AccessPolicyCreated (uuid)"]]

  ap_patch["PATCH /access-policies/{uuid}<br/>(update)"] --> ap_ctrl_update["AccessPolicyController::update"]
  ap_ctrl_update --> ap_srv_update["AccessPolicyService::update"]
  ap_srv_update --> ap_logic_update{{"txn + try_access +<br/>update policy"}}
  ap_logic_update --> ap_ret_update[["AccessPolicyUpdated"]]

  ap_delete["DELETE /access-policies/{uuid}<br/>(delete)"] --> ap_ctrl_delete["AccessPolicyController::delete"]
  ap_ctrl_delete --> ap_srv_delete["AccessPolicyService::delete"]
  ap_srv_delete --> ap_logic_delete{{"txn + try_access +<br/>AccessPolicies::delete_by_id"}}
  ap_logic_delete --> ap_ret_delete[["AccessPolicyDeleted"]]
```

</details>

<details>
<summary>File Storage</summary>

```mermaid
graph TD
  f_get["GET /fs/files<br/>(read)"] --> f_ctrl_list["FileController::list"]
  f_ctrl_list --> f_srv_list["FileService::list"]
  f_srv_list --> f_logic_list{{"filter + pagination + partial model"}}
  f_logic_list --> f_ret_list[["List<FileListItem>"]]

  f_post["POST /fs/files<br/>(create/upload)"] --> f_ctrl_upload["FileController::upload"]
  f_ctrl_upload --> f_srv_upload["FileService::upload"]
  f_srv_upload --> f_logic_upload{{"insert metadata +<br/>storage.store + status update"}}
  f_logic_upload --> f_ret_upload[["FileUploaded (id)"]]

  f_download["GET /fs/files/{id}/download"] --> f_ctrl_download["FileController::download"]
  f_ctrl_download --> f_srv_download["FileService::download"]
  f_srv_download --> f_logic_download{{"storage.download"}}
  f_logic_download --> f_ret_download[["Response<Body>"]]

  f_delete["DELETE /fs/files/{id}<br/>(delete)"] --> f_ctrl_delete["FileController::delete"]
  f_ctrl_delete --> f_srv_delete["FileService::delete"]
  f_srv_delete --> f_logic_delete{{"txn + fetch file +<br/>optional storage.delete + delete row"}}
  f_logic_delete --> f_ret_delete[["FileDeleted"]]
```

</details>

<details>
<summary>Board System</summary>

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

### Feature Flags

| Feature | Description |
|---------|-------------|
| `full-local` | Complete local development stack (Redis + local storage) |
| `full-aws` | Production AWS stack (DynamoDB + S3 + SSM) |
| `test-suite` | Testing configuration |

## 📖 Usage

### Basic Integration

```rust
use axum::{Extension, Router, routing};
use sumida::api::Sumida;

#[derive(Clone)]
pub struct AppState;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize infrastructure
    sumida::infra::tracing::subscribe();
    sumida::infra::db::establish_connection().await;
    
    let state = AppState;
    let settings = sumida::infra::settings::get();
    
    // Build application
    let app = Router::new()
        .merge(
            Sumida::new()
                .set_state(&state)
                .router(),
        )
        .layer(sumida::infra::session::new_layer().await?)
        .layer(sumida::infra::cors::new_layer())
        .layer(Extension(settings))
        .with_state(state)
        .route("/", routing::get(health_check));
    
    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app.into_make_service()).await?;
    
    Ok(())
}

async fn health_check() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}
```

### API Endpoints

The framework provides comprehensive REST APIs:

- **Authentication**: `/auth/*` - Login, logout, registration
- **Users**: `/users/*` - User management and profiles  
- **Organizations**: `/organizations/*` - Multi-tenant organization management
- **Roles**: `/roles/*` - Role and permission management
- **Boards**: `/boards/*` - Discussion forum management
- **Posts**: `/posts/*` - Content creation and management
- **API Keys**: `/api-keys/*` - Service authentication

## 🧪 Testing

### Run Tests
```bash
# Unit and integration tests
cargo test

# End-to-end tests
cargo test --package e2e

# With specific features
cargo test --features test-suite
```

### Test Coverage
```bash
# Install cargo-tarpaulin for coverage
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## 🔧 Configuration

### Environment Variables

Create a `.env` file based on your deployment:

#### Local Development
```env
DATABASE_URL=postgresql://user:password@localhost:5432/sumida
REDIS_URL=redis://localhost:6379
JWT_SECRET=your-secret-key
SMTP_HOST=localhost
SMTP_PORT=587
```

#### AWS Production
```env
DATABASE_URL=postgresql://user:password@rds-endpoint:5432/sumida
AWS_DYNAMODB_TABLE=sumida-sessions
AWS_S3_BUCKET=sumida-storage
AWS_SSM_JWT_SECRET_PATH=/sumida/jwt-secret
```

### Feature Configuration

Customize your build with Cargo features:

```toml
[dependencies]
sumida = { path = ".", features = ["full-aws", "image"] }
```

## 📊 Performance & Monitoring

### Built-in Observability
- **Structured Logging**: Tracing integration with configurable levels
- **Health Checks**: Basic health endpoints for load balancers
- **Error Tracking**: Comprehensive error handling with context

### Recommended Monitoring Stack
- **Metrics**: Prometheus + Grafana
- **Logging**: ELK Stack or AWS CloudWatch
- **Tracing**: Jaeger or AWS X-Ray
- **Uptime**: Custom health check monitoring

## 🚀 Deployment

### Local Development
```bash
# Start dependencies
docker-compose up -d postgres redis

# Run application
cargo run --features full-local --example basic
```

### Docker Deployment
```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --features full-aws

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/basic /usr/local/bin/app
CMD ["app"]
```

### AWS Deployment
Recommended AWS services:
- **Compute**: ECS Fargate or EKS
- **Database**: RDS PostgreSQL
- **Sessions**: DynamoDB
- **Storage**: S3
- **Secrets**: AWS Systems Manager Parameter Store

## 🤝 Contributing

### Development Setup
1. Install Rust toolchain and required dependencies
2. Set up local PostgreSQL and Redis instances  
3. Copy `.env.example` to `.env` and configure
4. Run database migrations: `cargo run --bin migration`
5. Start development server: `cargo run --features full-local --example basic`

### Code Style
- Follow Rust 2024 edition conventions
- Use `cargo fmt` for formatting
- Run `cargo clippy` for linting
- Ensure tests pass: `cargo test`

### Pull Request Guidelines
- Write descriptive commit messages
- Include tests for new features
- Update documentation for API changes
- Ensure CI/CD pipeline passes

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔗 Related Projects

- [SeaORM](https://github.com/SeaQL/sea-orm) - Database ORM
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Tower](https://github.com/tower-rs/tower) - Middleware ecosystem

---

**Technical Metrics**: 201 Rust source files • PostgreSQL + SeaORM • Axum 0.8.4 • Production-ready architecture

For detailed API documentation, architecture decisions, and deployment guides, see the [CLAUDE.md](CLAUDE.md) file.

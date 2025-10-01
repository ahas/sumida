# SUMIDA

**S**caleable **U**tility **M**odules **I**ntegration for **D**ynamic **A**pplication

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
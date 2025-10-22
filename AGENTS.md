# Repository Guidelines

## Project Structure & Module Organization
Sumida is a Rust workspace that composes layered crates. `api/` exposes Axum controllers, request DTOs, and service wiring. `entity/` defines the SeaORM models and generated entities. `infra/` bundles database, session, storage, tracing, and settings adapters; most environment-specific code lives here. `error/` centralizes shared error types. `e2e/` holds the end-to-end test harness, and `examples/` demonstrates runnable setups such as `basic`. The root `src/lib.rs` re-exports the public interface consumed by downstream applications. Workspace features in `Cargo.toml` (`full-local`, `full-aws`, `test-suite`) toggle storage backends and optional capabilities.

## Build, Test, and Development Commands
- `cargo run --bin migration` prepares the database schema; run after editing entities or merging migrations.
- `cargo run --features full-local --example basic` spins up the default local server once Redis/Postgres are available.
- `cargo run --features full-aws --example basic` validates AWS-specific wiring before deploying.
- `cargo test` covers unit and integration suites across all crates.
- `cargo test --package e2e` executes scenario tests located in `e2e/`.
- `cargo test --features test-suite` enables feature-gated checks used by CI.

## Coding Style & Naming Conventions
Code targets Rust 2024 with repository-wide rustfmt settings: 2-space indentation, 108-column width, module/import reordering. Format with `cargo fmt` before committing and keep modules, files, and feature flags in `snake_case`. Lint with `cargo clippy --all-targets --all-features`, resolving warnings or documenting intentional deviations.

## Testing Guidelines
Place focused unit tests alongside implementation modules under `#[cfg(test)]`. Use the SeaORM in-memory helpers from `infra` for database-heavy cases instead of ad-hoc connections. E2E scenarios in `e2e/` should document the required feature flags in their module docs. Generate coverage locally with `cargo tarpaulin --out Html` and keep coverage steady when introducing new user-facing flows.

## Commit & Pull Request Guidelines
Follow Conventional Commits (`type(scope): summary`) as seen in history (`refactor(fs): ...`). Reserve `wip` prefixes for drafts only and squash before merging. Each PR should include: a short change narrative, impacted features or flags, migration notes, and test evidence (commands and high-level results). Link issues with `Closes #id` and update docs (`README.md`, `CLAUDE.md`, `.env.example`) whenever behavior or configuration shifts.

## Configuration & Security Tips
Copy `.env.example` to `.env` and fill secrets locally; never commit credentials. Prefer AWS Parameter Store or local `.env` overrides for keys referenced by `infra::settings`. When switching between local and AWS profiles, clear cached data (`target/`, Redis, S3 buckets) to avoid stale state during reviews.

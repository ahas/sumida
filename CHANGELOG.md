# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to semantic versioning at the workspace level.

## [Unreleased]

### Added

- **An authorization test harness that can see what a status code cannot** (`e2e::authz`,
  `e2e::org_tree`). The pre-existing `ap_test_utils` suite asserts `resp.status()` and nothing
  else, which is the blind spot an authorization audit of this codebase kept falling into: an
  endpoint answering `200 OK` with another tenant's rows satisfies every assertion in it, and three
  of the confirmed criticals were exactly that. The new harness asserts **which rows came back**,
  by id and exactly, and **which layer refused** — a row filtered out by a query's own scope
  (`200` without it, or `404`) is a different outcome from a handler check returning `403`, and a
  test that accepts "denied somehow" stays green through a regression that deletes a list query's
  scope while leaving the single-row check in place. `Session` also gives each caller its own
  cookie jar; the shared one in `TestClient` meant logging in as a second user silently replaced
  the first, so a "B cannot see A's row" test was really asking B about B. `OrgTree` builds a real
  organization hierarchy — root / aunt / parent / subject / sibling / child / grandchild, plus
  arbitrary-depth chains and re-parenting — deep enough that a walk which stops early or runs
  toward ancestors instead of descendants shows up. `api/tests/authz_harness.rs` proves these
  claims against the running server, including negative cases that must fail if the assertions
  ever go blind.

- **Golden authorization tests** (`api/tests/authz_golden.rs`), written on that harness, recording
  what the model does today so the pending rewrite can be *proved* equivalent instead of argued to
  be. Cases are labelled: `pin_*` must survive the rewrite, `today_*` records a starting point the
  rewrite is expected to change — pinning a defect under the name "golden" would be worse than no
  test. Every case first grants the route-level `service` capability, because without it the gate
  refuses and the row scope under test never runs; a case that passes on a `403` from the gate
  would pass just as well with the scope deleted.

  Recorded while writing them: `GET /users/{id}` answers **401 `InvalidAccount`** for a row its
  scope excluded (`controller/user.rs:37` maps `get(..).ok_or(..)`), not 404 — so a `401` from that
  endpoint says nothing about authentication, and `Session::assert_authenticated` exists to
  establish the session is sound before a case reads one as "filtered".

### Security fixes

- **An API key could be issued in someone else's name — full account takeover.** `CreateApiKey.user_id`
  was taken from the request body and written verbatim, while `layer::user_guard::verify_api_key`
  authorizes the session as the key's owner. `is_secured` defaults to false, so the session inherits
  that owner's full permissions, `is_admin` included. Holding the coarse `api_key:create` capability
  was therefore enough to mint a key bound to an administrator and use it. A non-admin may now only
  issue a key for their own account; the request is refused rather than silently rewritten, so a
  caller learns the request was wrong. `UpdateApiKey` carries no `user_id`, so there is no rebind path.
- **`GET /files/stats` aggregated over every tenant's files.** It queried `Files::find()` directly
  while its sibling `list` went through the access-policy-scoped `select()`. The aggregate now runs
  over the same scoped row set.
- **`FileService::delete` authorized against `read`.** It reused the read-scoped select helper to find
  the row it was about to delete, so read-only access to a file permitted deleting it. It now
  authorizes against `delete`.

## [0.2.0] - 2026-06-10

A security-hardening release covering authentication/session, ReBAC authorization,
file/upload handling, and build/feature correctness. Includes breaking API changes
(see below) and a database migration.

### ⚠️ Breaking API changes

- **`api::service::api_key::ApiKeyService::create`** now returns the plaintext key
  (`Result<String, SumidaError>`) instead of `Result<InsertResult<..>, SumidaError>`.
  The plaintext is returned **only once**; the database stores a blake3 hash. Existing
  plaintext keys are converted by the new `api_key_hash` migration.
- **`api::service::auth::token::AuthTokenService::logout`** now takes a second argument
  `access_token: Option<String>` to blacklist the current access token, in addition to
  the refresh token.
- **`api::util::org::organization_ancestors_subquery`** now returns a complete
  `"ap"."object" = ANY(...)` comparison expression (previously just `ANY(...)`).
  Callers must add it directly to a condition, not wrap it with `col.eq(..)`.
- **`infra::jwt`**: `RedisJwtStore` is now a working implementation (was an
  `Unsupported` stub). `api::service::auth::token::create_jwt_store` returns a concrete
  `JwtStore` type alias, and a new `shared_jwt_store()` exposes a process-wide store.
- **`error::SumidaError::code`** now returns per-variant, category-based codes
  (`E1xxx` auth, `E2xxx` data, `E3xxx` mail, `E4xxx` file, `E5xxx` runtime, `E6xxx`
  request, `E7xxx` external) instead of a single `E-1`.

### Added

- New setting block `[storage.upload]` (`max-file-size`, `allowed-file-types`) applying
  global limits to the generic `/files` upload endpoint.
- New entity column `api_keys.key_prefix` (non-secret leading chars for list display).
- New migration `api_key_hash` (converts existing plaintext API keys and their
  `access_policies` identifiers to hashes; adds `key_prefix`). **Run before deploying.**
- JWT secret length validation (HS256 secret must be at least 32 bytes).
- `settings.e2e.example.toml` and `.env.e2e.example`; refreshed `settings.example.toml`.

### Security fixes

- **Critical** — `impersonate` was reachable without authentication; moved behind
  `user_guard` and gated by an explicit admin check.
- **Critical** — `activate` did not bind the invitation uuid to its `user_id`, allowing
  password reset of arbitrary accounts; now verified.
- **Critical** — local file storage allowed path traversal / absolute paths via the
  client-supplied `dir`; paths are now validated against the base directory.
- **High** — non-admin user creation could set `is_admin`/foreign org; now forced safe.
- **High** — non-admins could self-grant wildcard `allow` policies (access_policy / role /
  api_key); wildcard self-escalation is now rejected.
- **High** — password-change/verification codes ignored expiry and lacked send controls.
- **High** — `board_file` list/get bypassed ReBAC and delete lacked a row guard (IDOR).
- **High** — full-text search snippet delimiters were string-interpolated (SQL injection);
  now parameter-bound.
- **High** — the generic `/files` upload performed no validation; now enforced.
- **Medium** — JWT/API-key auth ignored `is_enabled`/ban and key expiry; session fixation
  (no `cycle_id`/`flush`); login timing user enumeration; Content-Disposition injection;
  multipart panics; API keys stored in plaintext.

### Fixed

- **Production authorization bug**: the org-scoped permission check generated invalid SQL
  (`col = (ANY(...))`), causing non-admin user `update`/`delete` to always return 403.
- `board_post` view-count increment condition was inverted.
- `pagination` underflowed on `?page=0`; `wrap_ilike` did not escape LIKE wildcards.
- Messenger user-room id mismatch; image decode lacked decompression-bomb limits.
- `BanSettings.enabled` was ignored; `ban_ip` arguments were swapped.

### Build / Internal

- `full-aws` now correctly selects `DynamoJwtStore` (workspace internal deps use
  `default-features = false`; `e2e` moved to dev-dependencies).
- Diagnostic messages on boot-time configuration failures.
- Restored the e2e test suite (ephemeral test-server ports, helpers, migration
  registration) and fixed the stale `create_dummy_board_posts` example.
- Comments and runtime/log messages standardized to English.
- Dependencies updated to latest semver-compatible versions.

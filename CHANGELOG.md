# Changelog

All notable changes to this project are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to semantic versioning at the workspace level.

## [Unreleased]

### Security fixes

- **The sign-in lockout never locked anybody out, and it does now — read this before upgrading.**
  The failure tally the check compared against `security.ban.max_failed_attempts` was written by
  reading "the last" failure row for the account with no ordering at all, so the database returned
  an arbitrary one. Every failure read the same value and wrote one more than it: the tally never
  passed 1, and the comparison against a threshold of ten was never true. No account and no address
  has ever been blocked by this mechanism on any installation. The check now counts the log itself
  — the run of password failures for the account, newest first, that is newer than its last
  successful password sign-in and unbroken by `security.ban.failure_reset_after` — and blocks when
  that run **reaches** the allowed number rather than exceeding it. It reads no more rows than the
  threshold needs.

  **What changes for a running installation:** the defaults are enabled, ten attempts, no reset
  window and no expiry, so from the first deploy an account or address that gets the password wrong
  ten times running is blocked, and with `security.ban.duration` at `forever` that block stands
  until an administrator lifts it. An installation that never wanted this should set
  `security.ban.enabled` to false before deploying. `security.ban.target` decides who the block
  lands on and defaults to `ip`, which blocks the network address rather than the account.

- **`security.ban.duration` was ignored; the ban expired on the reset window instead.** The expiry
  handed to the ban was parsed from `security.ban.failure_reset_after`, and the span the operator
  set as the length of the block was read and discarded. The two settings now do their own jobs:
  `duration` is the expiry, `failure_reset_after` only breaks the streak. `forever`, `none`, an
  empty value and anything unreadable all mean "no bound" for both, which is the strict side —
  an unreadable value cannot shorten a ban or forgive a failure, and it is logged at warn.

- **An unrecognised `security.ban.target` reported that a ban had happened.** The target is an
  operator-editable runtime setting, so a typo banned nobody while the caller told every user their
  account was blocked. An unrecognised target is now logged at warn and answers that nothing was
  banned.

- **`access_logs.count` recorded an arbitrary number.** Nothing reads the column — it is not in the
  log's response DTO and not in any client schema — but a column that states a number has to state
  the right one. It now records the consecutive password failures for the account since its last
  successful sign-in, the attempt being logged included, with no policy in it.

- **The sign-in log had no index touching the account** (`access_log_account_index`), so every read
  on the lockout path scanned the whole table, on the path an attacker drives hardest. The new
  composite is `(user_id, method, code, created_at DESC)`. Consumers must register it in their own
  migrator.

### Changed

- **sumida no longer knows its consumers.** The framework shipped every downstream product's
  authorization rules — four rule files compiled into one binary, and a `PRODUCTS` table naming
  each one — so a service built on sumida carried the names, resources and namespaces of unrelated
  services in its binary and in its source. It now ships **one** rule file, its own, and a product
  registers its own with `Sumida::new(RULES)`. Nothing in the framework enumerates products, counts
  them, or names one.

  `Sumida::new` grows a required argument, which is the point: forgetting to register is a compile
  error at the one call every product makes, rather than a 500 on the first authorized request. A
  product with no rules of its own passes `&[]` — "I have none" and "I forgot" must not look the
  same. Registration must happen **before** the rule set is first read, and installing after that
  is refused rather than warned about: a process that answered one request with one rule set and
  the next with another is two permission models wearing one deployment.

- **`extend NAME { … }`, a new rule-language construct.** A rule file may not redeclare a definition
  another file declared — one name with two definitions is one question with two answers — so
  without this, a product could not add its own resource families to sumida's `organization` and
  the framework would have to enumerate them for it. `extend` puts that list back on the side that
  owns it. An extension may not restate the definition's identity (`on`, `closure`), and a member
  that collides with one the target already has is a boot failure: a product silently redefining a
  framework permission by adding a file is the worst outcome the construct could have.

- **The rule files are `.sz`, not `.zed`.** The extension belonged to SpiceDB, and the language is a
  dialect rather than a subset — `on TABLE.COL`, `column(...)` derivations, `closure … via` with
  `^`, `scope`, and now `extend`, none of which SpiceDB has. Pointing SpiceDB's tooling at one of
  these files fails. `docs/sz-language.md` is the language's canonical statement.

- **Boot validation is stricter, and the rule that made a switched-off feature survivable is
  unchanged.** With no foreign product's rules in the binary there is no per-product marker table
  to decide whose rules to check, so `Authz::boot` checks every rule it carries. What still absorbs
  an optional feature being off is the branch that was always doing it: a rule whose **table** is
  absent is `Absent`, because a feature that is off never created its tables while its rules are
  compiled in regardless. A missing **column** is `Drift` and still refuses start-up.

- **`Authz::sync_schema` is part of the boot sequence.** `relation_tuples` has a foreign key onto
  the vocabulary projection, so a relation the rules declare has to be there before the first grant
  — and with each product carrying its own rules, no migration can know what that vocabulary is. It
  is projected from the compiled registry at boot instead. The bootstrap seed in the `rebac`
  migration keeps only sumida's own five pairs.

### Removed

- **`rebac_backfill` and `rebac_mirror` are tombstones.** Both translated the legacy
  `access_policies` table, and both carried the resource vocabulary of whichever services existed
  when they were written — which is exactly what a framework must not hold. `access_policies` is
  gone from every database they ran against. The modules survive because sea-orm tracks migrations
  by file stem and deleting one breaks `migration up` on every database that applied it;
  `rebac_backfill` still refuses to continue past policy rows it would once have translated, so a
  stale dump restored onto a fresh checkout fails loudly instead of losing grants silently.

  `api/tests/authz_evaluation.rs::the_mirror_writes_what_the_backfill_would_have` is **deleted with
  no replacement.** It asserted that two implementations of that translation agreed with each
  other; there are now zero implementations, and nothing else covered what it covered because
  nothing else needs to. A deleted test with no replacement deserves a sentence, and that is it.


### Added

- **`DELETE /users/{user_id}/ban` — a ban can be lifted.** `POST /users/{user_id}/ban` had no
  counterpart, so the only way out of a ban was deleting the row, which destroys the very record
  the ban history exists to keep. The new route stamps `lifted_at` on every ban that user currently
  holds — not already lifted, not expired — and leaves the rows where they are, so `isBanned` on
  the user list and the user detail flips while the history stays readable. A user with no active
  ban is answered `200` with nothing changed: "already not banned" is the state the caller asked
  for, not a failure.

  It sits behind the same session guard and the same `user` **update** route gate as the ban route
  it undoes, and every ban it would lift is checked for `ban#edit` individually — the same row
  scope the ban service's own `update` and `delete` apply, because lifting is a write on those
  rows and a route gate answers a different question than a row scope does.

- **The login lockout policy is an operational setting, not a boot-fixed one.** The failed-attempt
  ceiling, the reset span, the ban span, the ban target, and the switch that turns the whole thing
  off now live in `system_settings` and are read at the point the policy is applied, so an operator
  changes them on a running service instead of editing a file and restarting. Each field falls back
  to its settings-file value independently, and a missing row, a wrong-typed value, or a failed
  query all take the fallback and log a warning — the login path never fails because of the store.
  Reading them requires the `system-settings` feature; without it the settings file is the only
  source, exactly as before.

- **`system_settings.category`.** A setting declares which kind of thing it governs — `auth`,
  `execution`, `retention`, `storage`, or `general` — so a settings screen can group by that instead
  of listing every key flat. The column is added by its own migration (`system_settings_category`),
  which also seeds the framework's own authentication settings; consumers add it to their `Migrator`
  after the migration that creates the table. Rows that declare nothing get `general`.

  `SystemSettingItem` gains a `category` field, so any client reading `GET /system-settings` sees it.

- **`docs/system-settings.md`** — the canonical statement of the settings mechanism: which values
  belong in the settings file, which belong in the runtime store, which stay code constants, the
  category vocabulary, and how a downstream service registers and reads its own settings.

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

### Changed

- **`access_policies` is no longer read at run time.** Authorization is evaluated from a compiled
  rule set (`api/src/authz/`, ~5,400 lines) over `relation_tuples`, `org_closure` and
  `board_group_closure`. The rules live in four schema files — `schema.zed` plus one per consumer —
  and are compiled to SQL predicates rather than assembled per request. `Authz::filter` supplies the
  `WHERE` clause of a list query and `Authz::try_check` answers a single row, so filter, `COUNT`,
  `ORDER BY` and `LIMIT` stay in one statement. The table itself is untouched and its mirror trigger
  still runs; nothing in `api/src` queries it.

- **Boot fails loudly on rule drift.** `Authz::boot_or_abort` validates every rule against the live
  schema before the server serves. A rule reading a column that does not exist used to mean a
  predicate that silently selected the wrong rows — authorization wrong while the service reported
  healthy. It is now a refusal to start.

### Added

- **`POST|GET|DELETE /relation-tuples`** — the grant surface that replaces writing `access_policies`
  rows. Writing a tuple is itself authorized: the caller must hold `<resource_type>#grant` on the
  named object, so a capability cannot be granted by someone who does not hold the authority to
  grant it.
- **`GET /authz/schema`** — the compiled vocabulary (relations, permissions, subject types) so a
  client can render a permission form without hardcoding names. Carries no grant and names no table
  or column; requires only a session.
- **`GET /me/permissions`** — what the calling subject may do, evaluated through the rules rather
  than read off raw relations, so one `service:<ns>#operator` tuple reports every verb it confers.

### Breaking

- **`GET|POST|PATCH|DELETE /access-policies` are gone**, along with `AccessPolicyService`. Move
  grants to `/relation-tuples`. The wire shape differs: there is no `effect` field — deny is a rule
  construct (`- blocked`), not a row — and no `PATCH`, because the unique constraint makes `POST` an
  upsert. Impacted: any caller managing permissions through that endpoint.

  ```
  # Before
  POST /access-policies
  { "namespace": "file", "object": "*", "action": ["read"],
    "user": "user:7", "effect": "allow" }

  # After
  POST /relation-tuples
  { "resourceType": "file", "resourceId": "<uuid>",
    "relation": "viewer", "subject": "user:7" }
  ```

- **`ApiService<A: ApGuard, E>` and the `ap` module are removed** (`util::ap`, `util::org`,
  `ApGuard`, `ApCondition`, `ApChain`, `ApOr`, `try_access`, `assert_policy_within_authority`,
  `managed_org_ids`). Services implement `select()` with `Authz::filter` and check single rows with
  `Authz::try_check`. Impacted: every service that carried the generic — 26 implementors across
  sumida and lab-app.

- **A grant that only wildcards is no longer accepted where a concrete authority is required.**
  `assert_policy_within_authority` refused only rows containing `*`, so a row naming somebody else's
  resource concretely passed — `access_policies` records no grantor, leaving nothing to compare
  against. The rule set starts from refusal and names each path to yes
  (`organization#grant = manage = manager^`, `service#grant = operator | platform:root->superuser`).
  Impacted: callers that relied on being able to grant beyond their own authority.

- **A secured API key bound to an administrator is no longer a superuser.** `users.is_admin` is now
  reachable only through the subject set as `platform#flagged`, so a secured key carries what the
  key holds and not what its owner is. Impacted: any integration depending on the old behaviour.

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

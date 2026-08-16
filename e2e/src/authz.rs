//! Authorization test harness.
//!
//! The existing [`crate::ap_test_utils`] suite asserts `resp.status()` and nothing else. That is
//! the exact blind spot an authorization audit of this codebase kept falling into: an endpoint
//! that answers `200 OK` with **another tenant's rows** satisfies every assertion in it. Three of
//! the confirmed criticals were read paths that returned somebody else's record with a perfectly
//! ordinary status code.
//!
//! So this module asserts two things that one does not:
//!
//! 1. **Which rows came back** — by id, exactly, not "the request succeeded".
//! 2. **Which layer refused** — a request denied by a handler check (`403`) and a request whose
//!    row was filtered out by the query's own scope (`200` with the row absent, or `404`) are
//!    different outcomes. They look identical to a test that only reads a status code, and that
//!    is precisely how a scope can be deleted from a list query without any test going red: some
//!    handler check elsewhere still returns `403` for the single-row case, so the suite stays
//!    green while the list leaks.
//!
//! Every case therefore has to declare which of the two it expects. That is the point: a test
//! that accepts "denied somehow" cannot tell a correct SQL filter from a handler check that
//! happens to agree with it today.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::Duration;

use reqwest::{Method, RequestBuilder, StatusCode, Url};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;

use crate::user::TestUser;

/// How a request ended, at the granularity authorization actually cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
  /// `2xx`, and the resource under test is present in the response.
  Visible,
  /// `2xx`, but the resource is absent — the query's own scope excluded it.
  ///
  /// This is the outcome a list endpoint produces when it is correctly scoped, and the one that
  /// is indistinguishable from [`Access::Visible`] to a status-code assertion.
  Filtered,
  /// `403` — a handler check refused before the row was ever fetched.
  Forbidden,
  /// `404` — the route denied the resource exists. For a scoped `find_by_id` this is the
  /// single-row equivalent of [`Access::Filtered`]; keep them distinct because which one an
  /// endpoint produces is itself a behaviour worth pinning.
  NotFound,
  /// `401`.
  ///
  /// Kept separate from [`Access::Forbidden`] so a broken login fixture cannot read as a passing
  /// authorization test. Beware that it does **not** always mean "not logged in": some handlers
  /// map a row their scope filtered out onto `401` rather than `404` — `GET /users/{id}`
  /// (`controller/user.rs:37`) answers `InvalidAccount` when the scoped select returns nothing.
  /// A case that expects this must say so and explain why, or it is asserting nothing.
  Unauthenticated,
  /// Anything else, carried verbatim so a failure message can say what actually happened.
  Other(u16),
}

impl Access {
  fn from_status(status: StatusCode) -> Option<Self> {
    match status.as_u16() {
      401 => Some(Self::Unauthenticated),
      403 => Some(Self::Forbidden),
      404 => Some(Self::NotFound),
      s if (200..300).contains(&s) => None, // 본문을 봐야 결정된다
      s => Some(Self::Other(s)),
    }
  }
}

/// One authenticated caller, with a cookie jar of its own.
///
/// [`crate::client::TestClient`] keeps a single shared jar, so logging in as a second user
/// silently replaces the first. Authorization tests need several callers alive at once — the
/// whole point is comparing what each of them sees of the same row — so each session owns its
/// client.
pub struct Session {
  pub label: String,
  pub user_id: i32,
  base: SocketAddr,
  http: reqwest::Client,
}

impl Session {
  /// Logs `user` in and returns a session bound to that login.
  ///
  /// `label` appears in assertion failures. Use the role the user plays in the case
  /// ("org-100 manager", "peer member"), not their username — a failure that says
  /// `sibling-org member saw task 7` is readable; one that says `test-1755…-Xq2 saw task 7` is not.
  pub async fn login(base: SocketAddr, user: &TestUser, label: impl Into<String>) -> Self {
    let http = build_client();
    let label = label.into();

    let resp = http
      .post(url(base, "/auth/session"))
      .json(&serde_json::json!({
        "username": user.model.username,
        "password": user.password,
        "rememberMe": false,
      }))
      .send()
      .await
      .expect("login request failed");

    assert!(
      resp.status().is_success(),
      "[{label}] login failed with {} — the fixture is broken, not the authorization under test",
      resp.status()
    );

    Self { label, user_id: user.model.id, base, http }
  }

  /// Confirms this session is really authenticated.
  ///
  /// Some endpoints answer `401` for a row their scope excluded, so a case that reads a `401` as
  /// "filtered" needs to have established that the session itself is sound. Call this once, then
  /// the later `401` can only be about the row.
  pub async fn assert_authenticated(&self) -> &Self {
    let resp = self.request(Method::GET, "/me").send().await.expect("request failed");
    assert!(
      resp.status().is_success(),
      "[{}] the session is not authenticated (GET /me returned {}) — any later 401 says nothing about authorization",
      self.label,
      resp.status()
    );
    self
  }

  /// A caller that never logged in. Used to pin that a route requires authentication at all.
  pub fn anonymous(base: SocketAddr) -> Self {
    Self { label: "anonymous".into(), user_id: 0, base, http: build_client() }
  }

  fn request(&self, method: Method, path: &str) -> RequestBuilder {
    self.http.request(method, url(self.base, path))
  }

  /// Fetches `path` and classifies the outcome for the single resource `id`.
  ///
  /// A `2xx` counts as [`Access::Visible`] only if `id` appears in the body — an endpoint that
  /// answers `200` with an empty object has filtered, not shown.
  pub async fn probe_one(&self, path: &str, id: &dyn ResourceId) -> Probe {
    let resp = self.request(Method::GET, path).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);

    let access = Access::from_status(status).unwrap_or_else(|| {
      if id.appears_in(&body) { Access::Visible } else { Access::Filtered }
    });

    Probe { label: self.label.clone(), path: path.to_string(), status, access, body }
  }

  /// Fetches a list endpoint and collects the ids it returned.
  ///
  /// Handles both the framework's `{ items: [...], count: n }` envelope and a bare array.
  pub async fn probe_list(&self, path: &str) -> ListProbe {
    let resp = self.request(Method::GET, path).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);

    let items = match &body {
      Value::Object(o) => o.get("items").cloned().unwrap_or(Value::Null),
      other => other.clone(),
    };
    let ids = match items {
      Value::Array(rows) => rows.iter().filter_map(|r| r.get("id").map(id_to_string)).collect(),
      _ => BTreeSet::new(),
    };

    ListProbe { label: self.label.clone(), path: path.to_string(), status, ids, body }
  }

  /// Issues a write and classifies it. The caller is expected to follow up with a database
  /// assertion — a `2xx` from a write proves the request was accepted, not that the row changed.
  pub async fn probe_write(&self, method: Method, path: &str, body: Option<&Value>) -> Probe {
    let mut req = self.request(method, path);
    if let Some(b) = body {
      req = req.json(b);
    }
    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);

    let access = Access::from_status(status).unwrap_or(Access::Visible);

    Probe { label: self.label.clone(), path: path.to_string(), status, access, body }
  }
}

/// The outcome of a single-resource request.
pub struct Probe {
  label: String,
  path: String,
  status: StatusCode,
  access: Access,
  body: Value,
}

impl Probe {
  pub fn access(&self) -> Access {
    self.access
  }

  /// Asserts the exact outcome, including **which layer** produced it.
  ///
  /// Passing [`Access::Forbidden`] where the endpoint actually filters (or the reverse) fails.
  /// That strictness is deliberate: those two are the states a regression moves between.
  #[track_caller]
  pub fn assert(self, expected: Access) -> Self {
    assert_eq!(
      self.access, expected,
      "[{}] {} — expected {:?}, got {:?} (HTTP {}). body: {}",
      self.label,
      self.path,
      expected,
      self.access,
      self.status,
      truncate(&self.body)
    );
    self
  }

  pub fn body(&self) -> &Value {
    &self.body
  }
}

/// The outcome of a list request.
pub struct ListProbe {
  label: String,
  path: String,
  status: StatusCode,
  ids: BTreeSet<String>,
  body: Value,
}

impl ListProbe {
  pub fn ids(&self) -> &BTreeSet<String> {
    &self.ids
  }

  /// Asserts the response contains **exactly** these ids — no more, no fewer.
  ///
  /// Exactness is the whole assertion. "contains what I expect" passes while the list also leaks
  /// every other tenant's rows, which is the failure this harness exists to catch.
  #[track_caller]
  pub fn assert_exactly(self, expected: &[&dyn ResourceId]) -> Self {
    assert!(
      self.status.is_success(),
      "[{}] {} — expected a list, got HTTP {}. body: {}",
      self.label,
      self.path,
      self.status,
      truncate(&self.body)
    );

    let want: BTreeSet<String> = expected.iter().map(|i| i.as_id()).collect();
    if self.ids != want {
      let extra: Vec<_> = self.ids.difference(&want).cloned().collect();
      let missing: Vec<_> = want.difference(&self.ids).cloned().collect();
      panic!(
        "[{}] {} returned the wrong rows.\n  leaked (present, must not be): {:?}\n  hidden (absent, must be): {:?}\n  got: {:?}\n  want: {:?}",
        self.label, self.path, extra, missing, self.ids, want
      );
    }
    self
  }

  /// Asserts the caller sees nothing — a correctly scoped list for someone with no grant.
  #[track_caller]
  pub fn assert_empty(self) -> Self {
    self.assert_exactly(&[])
  }

  /// Asserts the request was refused outright rather than scoped to nothing.
  #[track_caller]
  pub fn assert_refused(self, expected: Access) -> Self {
    let actual = Access::from_status(self.status).unwrap_or(Access::Visible);
    assert_eq!(
      actual, expected,
      "[{}] {} — expected the request to be refused with {:?}, got {:?} (HTTP {})",
      self.label, self.path, expected, actual, self.status
    );
    self
  }
}

/// Assertions about what is actually in the database, for write paths.
///
/// A write that answers `2xx` has been accepted, not applied — and a write that answers `403` may
/// still have committed something before the check. Both are worth pinning separately from the
/// status code.
pub struct DbState<'c, C: ConnectionTrait> {
  db: &'c C,
}

impl<'c, C: ConnectionTrait> DbState<'c, C> {
  pub fn new(db: &'c C) -> Self {
    Self { db }
  }

  async fn count(&self, table: &str, predicate: &str) -> i64 {
    let sql = format!(r#"SELECT count(*)::bigint AS n FROM "{table}" WHERE {predicate}"#);
    let row = self
      .db
      .query_one(Statement::from_string(DbBackend::Postgres, sql.clone()))
      .await
      .unwrap_or_else(|e| panic!("db assertion query failed: {sql}\n{e}"))
      .unwrap_or_else(|| panic!("db assertion returned no row: {sql}"));
    row.try_get::<i64>("", "n").expect("count column")
  }

  /// Asserts a row matching `predicate` exists.
  pub async fn assert_exists(&self, table: &str, predicate: &str) {
    let n = self.count(table, predicate).await;
    assert!(n > 0, "expected a row in {table} where {predicate}, found none");
  }

  /// Asserts no row matches — e.g. that a refused delete really left the row alone, or that a
  /// refused create wrote nothing.
  pub async fn assert_absent(&self, table: &str, predicate: &str) {
    let n = self.count(table, predicate).await;
    assert_eq!(n, 0, "expected no row in {table} where {predicate}, found {n}");
  }

  pub async fn assert_count(&self, table: &str, predicate: &str, expected: i64) {
    let n = self.count(table, predicate).await;
    assert_eq!(n, expected, "expected {expected} rows in {table} where {predicate}, found {n}");
  }
}

/// Anything that identifies a resource in a response body.
///
/// Implemented for the integer and uuid-ish ids this codebase uses, so a case can name its
/// fixtures directly instead of stringifying at every call site.
pub trait ResourceId {
  fn as_id(&self) -> String;

  fn appears_in(&self, body: &Value) -> bool {
    match body.get("id") {
      Some(v) => id_to_string(v) == self.as_id(),
      None => false,
    }
  }
}

impl ResourceId for i32 {
  fn as_id(&self) -> String {
    self.to_string()
  }
}

impl ResourceId for i64 {
  fn as_id(&self) -> String {
    self.to_string()
  }
}

impl ResourceId for String {
  fn as_id(&self) -> String {
    self.clone()
  }
}

impl ResourceId for &str {
  fn as_id(&self) -> String {
    (*self).to_string()
  }
}

fn id_to_string(v: &Value) -> String {
  match v {
    Value::String(s) => s.clone(),
    other => other.to_string(),
  }
}

fn truncate(v: &Value) -> String {
  let s = v.to_string();
  if s.len() > 400 { format!("{}…", &s[..400]) } else { s }
}

fn url(base: SocketAddr, path: &str) -> Url {
  Url::parse(&format!("http://{base}/"))
    .expect("base url")
    .join(path.trim_start_matches('/'))
    .expect("path")
}

fn build_client() -> reqwest::Client {
  reqwest::Client::builder()
    .cookie_store(true)
    .connect_timeout(Duration::from_secs(30))
    .timeout(Duration::from_secs(30))
    .redirect(reqwest::redirect::Policy::none())
    .build()
    .expect("failed to build a test http client")
}

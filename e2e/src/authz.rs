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
//!
//! [`StreamProbe`] carries the same two questions to SSE endpoints, where they are harder. A
//! firehose filters a live broadcast per event, so "was the request refused" answers nothing: the
//! subscription is supposed to open. What has to be asserted is **which events reached this
//! subscriber**, exactly, and that a refused subscription is not mistaken for an open one that
//! delivered nothing.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

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

  /// Opens an SSE subscription and returns **as soon as the response headers arrive**, before any
  /// of the body has been read.
  ///
  /// Returning at the headers is what makes the live cases possible. A handler subscribes to its
  /// broadcast channel before it produces the response, so by the time this returns the subscriber
  /// is attached and nothing published afterwards can be missed. A case can therefore open the
  /// stream, *then* cause the events, and only then call [`StreamProbe::collect`] — which is the
  /// only order in which "which events reached this subscriber" is a meaningful question.
  ///
  /// A refused subscription is read to the end here, because its body is a short error document
  /// and a case needs it to tell which layer refused.
  pub async fn probe_stream(&self, path: &str) -> StreamProbe {
    let resp = self
      .request(Method::GET, path)
      .header("accept", "text/event-stream")
      .send()
      .await
      .expect("request failed");
    let status = resp.status();

    let (resp, body) = if status.is_success() {
      (Some(resp), Value::Null)
    } else {
      (None, resp.json().await.unwrap_or(Value::Null))
    };

    StreamProbe {
      label: self.label.clone(),
      path: path.to_string(),
      status,
      body,
      resp,
      buf: Vec::new(),
      events: Vec::new(),
      closed: false,
    }
  }
}

/// How long a [`StreamProbe`] listens, and what may stop it early.
///
/// Every stream assertion needs a bound. Without one, the failure mode this harness exists to
/// catch — a filter that lets nothing through, or one that lets everything through and then goes
/// quiet — hangs the suite instead of failing it.
#[derive(Clone, Copy, Debug)]
pub struct StreamWindow {
  timeout: Duration,
  stop_after: Option<usize>,
}

impl StreamWindow {
  /// Listen for at most `timeout` and keep everything that arrives in it.
  ///
  /// This is the form an exactness assertion needs.
  pub fn of(timeout: Duration) -> Self {
    Self { timeout, stop_after: None }
  }

  /// Stop as soon as `n` events have arrived.
  ///
  /// Use it only when the case asks whether something gets through *at all*. Never combine it with
  /// [`StreamProbe::assert_exactly`]: stopping at the n-th event cannot tell you that an
  /// (n+1)-th — somebody else's — was not on its way, so the exactness would be an illusion.
  pub fn stop_after(mut self, n: usize) -> Self {
    self.stop_after = Some(n);
    self
  }
}

/// One event parsed out of an SSE stream.
#[derive(Clone, Debug)]
pub struct StreamEvent {
  /// The `event:` name — `task:created`, `task:log`. Empty when the server sent none.
  pub name: String,
  /// The `data:` payload parsed as JSON. A payload that is not JSON is carried as a string.
  pub data: Value,
}

/// A subscription to a stream endpoint, and the events it delivered.
///
/// The distinction that matters here is the one a status code cannot make: **the subscription was
/// refused** and **the subscription opened and delivered nothing** are different outcomes. The
/// first is a handler check; the second is a per-event filter doing its job — or an over-broad fix
/// that closed the stream to its rightful subscriber too. A case has to say which it expects:
/// [`Self::assert_refused`] for the first, [`Self::assert_opened`] plus [`Self::assert_empty`] for
/// the second.
pub struct StreamProbe {
  label: String,
  path: String,
  status: StatusCode,
  /// The refusal body. `Null` while the stream is open — the body is being streamed, not read.
  body: Value,
  resp: Option<reqwest::Response>,
  buf: Vec<u8>,
  events: Vec<StreamEvent>,
  closed: bool,
}

impl StreamProbe {
  pub fn status(&self) -> StatusCode {
    self.status
  }

  /// The refusal body, for a case that needs to say **which** layer refused — two different
  /// checks can both answer `403`.
  pub fn body(&self) -> &Value {
    &self.body
  }

  pub fn events(&self) -> &[StreamEvent] {
    &self.events
  }

  /// The `event:` names received, in order.
  pub fn names(&self) -> Vec<&str> {
    self.events.iter().map(|e| e.name.as_str()).collect()
  }

  /// True once the server closed the stream on its own.
  pub fn closed(&self) -> bool {
    self.closed
  }

  /// The resource ids the subscriber actually received.
  ///
  /// Read out of each payload by the shapes the SSE endpoints in this codebase emit: the resource
  /// itself (`{"id": …}`), a tagged envelope (`{"kind": …, "data": {"id": …}}`), and a log line
  /// that names the task it belongs to (`{"taskId": …}`), in that order. A payload with none of
  /// them contributes no id — assert on [`Self::events`] for those.
  pub fn ids(&self) -> BTreeSet<String> {
    self.events.iter().filter_map(|e| event_id(&e.data)).collect()
  }

  /// Asserts the subscription was established.
  ///
  /// Call it before collecting. A stream that was refused collects nothing, and "no events
  /// arrived" would then read as a passing filter test when the truth is that no subscription ever
  /// existed.
  #[track_caller]
  pub fn assert_opened(self) -> Self {
    assert!(
      self.resp.is_some(),
      "[{}] {} — the subscription was refused with HTTP {}, so it delivered nothing for a reason \
       that has nothing to do with per-event filtering. body: {}",
      self.label,
      self.path,
      self.status,
      truncate(&self.body)
    );
    self
  }

  /// Asserts the subscription was refused outright, and by which layer.
  #[track_caller]
  pub fn assert_refused(self, expected: Access) -> Self {
    let actual = Access::from_status(self.status).unwrap_or(Access::Visible);
    assert_eq!(
      actual, expected,
      "[{}] {} — expected the subscription to be refused with {:?}, got {:?} (HTTP {}). body: {}",
      self.label,
      self.path,
      expected,
      actual,
      self.status,
      truncate(&self.body)
    );
    self
  }

  /// Listens on an open subscription for `window` and keeps whatever arrived.
  ///
  /// Can be called more than once to listen again over a later window; events accumulate.
  pub async fn collect(mut self, window: StreamWindow) -> Self {
    // 거절된 구독에 대고 collect 하면 무엇을 단언하든 0건이 나온다. 조용히 통과시키지 않는다.
    let mut resp = self.resp.take().unwrap_or_else(|| {
      panic!(
        "[{}] {} — collect() on a subscription that was refused with HTTP {}; assert_refused() is \
         the assertion this case wants",
        self.label, self.path, self.status
      )
    });

    let deadline = Instant::now() + window.timeout;

    loop {
      if window.stop_after.is_some_and(|n| self.events.len() >= n) {
        break;
      }
      let Some(left) = deadline.checked_duration_since(Instant::now()) else {
        break;
      };

      match tokio::time::timeout(left, resp.chunk()).await {
        // 창이 닫혔다. 스트림 테스트의 정상 종료 조건이다.
        Err(_) => break,
        Ok(Ok(None)) => {
          self.closed = true;
          break;
        }
        Ok(Ok(Some(chunk))) => {
          self.buf.extend_from_slice(&chunk);
          drain_sse_events(&mut self.buf, &mut self.events);
        }
        Ok(Err(e)) => {
          panic!("[{}] {} — reading the stream failed: {e}", self.label, self.path)
        }
      }
    }

    self.resp = Some(resp);
    self
  }

  /// Asserts the subscriber received **exactly** these ids — no more, no fewer.
  ///
  /// Exactness is the whole assertion, and on a broadcast it is the only one worth making: a
  /// firehose that correctly delivers the caller's own events while also delivering every other
  /// tenant's satisfies "I got mine".
  #[track_caller]
  pub fn assert_exactly(self, expected: &[&dyn ResourceId]) -> Self {
    let got = self.ids();
    let want: BTreeSet<String> = expected.iter().map(|i| i.as_id()).collect();
    if got != want {
      let extra: Vec<_> = got.difference(&want).cloned().collect();
      let missing: Vec<_> = want.difference(&got).cloned().collect();
      panic!(
        "[{}] {} delivered the wrong events.\n  leaked (received, must not be): {:?}\n  hidden (not received, must be): {:?}\n  got: {:?}\n  want: {:?}\n  event names: {:?}",
        self.label,
        self.path,
        extra,
        missing,
        got,
        want,
        self.names()
      );
    }
    self
  }

  /// Asserts the subscription opened and then delivered nothing.
  ///
  /// Distinct from [`Self::assert_refused`] on purpose — see the type's own documentation.
  #[track_caller]
  pub fn assert_empty(self) -> Self {
    assert!(
      self.events.is_empty(),
      "[{}] {} — expected an open subscription to deliver nothing, got {} event(s): {:?}",
      self.label,
      self.path,
      self.events.len(),
      self.names()
    );
    self
  }
}

/// Pulls every complete blank-line-terminated block out of `buf`, leaving any partial tail behind.
///
/// A chunk boundary can land anywhere, including mid-event and mid-UTF-8-sequence, so only whole
/// blocks are decoded. axum's SSE writer terminates blocks with `\n\n`.
fn drain_sse_events(buf: &mut Vec<u8>, out: &mut Vec<StreamEvent>) {
  while let Some(end) = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2) {
    let block: Vec<u8> = buf.drain(..end).collect();
    if let Some(event) = parse_sse_block(&String::from_utf8_lossy(&block)) {
      out.push(event);
    }
  }
}

/// Parses one SSE block. Returns `None` for a block that carries no event — a keep-alive comment.
fn parse_sse_block(block: &str) -> Option<StreamEvent> {
  let mut name = String::new();
  let mut data = String::new();

  for line in block.lines() {
    // 빈 줄과 `:` 로 시작하는 주석(keep-alive)은 이벤트가 아니다.
    if line.is_empty() || line.starts_with(':') {
      continue;
    }
    let (field, value) = line.split_once(':').unwrap_or((line, ""));
    let value = value.strip_prefix(' ').unwrap_or(value);
    match field {
      "event" => name = value.to_string(),
      "data" => {
        if !data.is_empty() {
          data.push('\n');
        }
        data.push_str(value);
      }
      _ => {}
    }
  }

  if name.is_empty() && data.is_empty() {
    return None;
  }
  let data = serde_json::from_str(&data).unwrap_or(Value::String(data));
  Some(StreamEvent { name, data })
}

fn event_id(data: &Value) -> Option<String> {
  data
    .get("id")
    .or_else(|| data.get("data").and_then(|d| d.get("id")))
    .or_else(|| data.get("taskId"))
    .map(id_to_string)
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

  /// The raw body — for a refusal, the error document that says **which** check refused. Two
  /// different checks can both answer `403`.
  pub fn body(&self) -> &Value {
    &self.body
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

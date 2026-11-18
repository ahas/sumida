//! What the fragment says about the authentication surface, held against what the server does.
//!
//! A client generated from a document begins here: it signs in, and everything else it does depends
//! on having done so. So the statements checked here are the ones a wrong document makes
//! unrecoverable — which routes take a credential, which take none, what the bodies are called, and
//! whether the credentials an operation asks for are described anywhere.
#![cfg(feature = "openapi")]

use std::collections::BTreeSet;

use api::controller::{auth, me};
use api::openapi::{ErrorBody, fragment};
use api::{Sumida, SumidaState};
use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use error::SumidaError;
use sea_orm::DatabaseConnection;
use serde_json::Value as JsonValue;
use tower::ServiceExt;
use tower_sessions::{MemoryStore, SessionManagerLayer};
use utoipa::PartialSchema;

/// The methods an operation can be written under. A path item carries at most one of each.
const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// The controller sources this file speaks for, and where each one's router is mounted.
///
/// axum's router cannot be asked what it stands up, so the statement that wires a route and the
/// statement that describes it are read out of the same source as text. Both name the same path
/// constant, which is what makes comparing them a comparison of constant names.
fn controllers() -> [(&'static str, &'static str); 3] {
  [
    (include_str!("../src/controller/auth/session.rs"), auth::session::MOUNT),
    (include_str!("../src/controller/auth/token.rs"), auth::token::MOUNT),
    (include_str!("../src/controller/me.rs"), me::MOUNT),
  ]
}

#[derive(Clone)]
struct ProbeState(DatabaseConnection);

impl SumidaState for ProbeState {
  fn db(&self) -> &DatabaseConnection {
    // Nothing here reaches a query: a request carrying no credential is turned away by the guard,
    // and one that gets past it is refused by the body reader before the handler runs. A connection
    // that is not one errors rather than ending a test in a panic.
    &self.0
  }
}

/// The router a deployment stands up, with a session store behind it so the session extractor has
/// somewhere to read from.
fn router() -> axum::Router {
  let state = ProbeState(DatabaseConnection::Disconnected);

  Sumida::new(&[])
    .set_state(&state)
    .router()
    .with_state(state)
    .layer(SessionManagerLayer::new(MemoryStore::default()))
}

/// The fragment as JSON.
fn document() -> JsonValue {
  serde_json::to_value(fragment()).expect("the fragment is JSON")
}

/// Every operation in the fragment, as (path, method, operation).
fn operations(document: &JsonValue) -> Vec<(String, &'static str, &JsonValue)> {
  document["paths"]
    .as_object()
    .expect("a path table")
    .iter()
    .flat_map(|(path, item)| {
      METHODS
        .iter()
        .filter_map(move |method| item.get(*method).map(|operation| (path.clone(), *method, operation)))
    })
    .collect()
}

/// Whether a path is one of the ones this file speaks for.
fn ours(path: &str) -> bool {
  controllers()
    .iter()
    .any(|(_, mount)| path == *mount || path.starts_with(&format!("{mount}/")))
}

/// The keys of a JSON object.
fn keys(value: &JsonValue) -> BTreeSet<String> {
  value.as_object().expect("an object").keys().cloned().collect()
}

/// The names a schema gives its properties.
fn properties<T: PartialSchema>() -> BTreeSet<String> {
  let schema = serde_json::to_value(T::schema()).expect("a schema is JSON");
  schema["properties"].as_object().expect("an object schema").keys().cloned().collect()
}

/// The properties a schema calls required.
fn required<T: PartialSchema>() -> BTreeSet<String> {
  let schema = serde_json::to_value(T::schema()).expect("a schema is JSON");
  schema["required"]
    .as_array()
    .map(|names| names.iter().filter_map(|name| name.as_str().map(str::to_owned)).collect())
    .unwrap_or_default()
}

/// Does every example the document offers read back through the reader that will receive it, and
/// does the schema call required exactly what the reader demands?
///
/// An example is what a client author copies, so one the server would refuse is worse than none.
/// The dropping half of this catches the mistake the derive cannot: it settles requiredness from a
/// field's syntax, and the wrapper types are not spelled `Option` however optional they are.
#[test]
fn every_example_reads_back_and_says_what_may_be_left_out() {
  use api::controller::auth::token::{LogoutRequest, RefreshTokenRequest};
  use api::service::auth::dto::LoginSession;
  use api::service::me::dto::UpdateMe;
  use sea_orm_crud::assert_required_matches_serde;

  assert_required_matches_serde::<LoginSession>(auth::session::sign_in_example());
  assert_required_matches_serde::<LoginSession>(auth::token::sign_in_example());
  assert_required_matches_serde::<RefreshTokenRequest>(auth::token::refresh_example());
  assert_required_matches_serde::<LogoutRequest>(auth::token::revoke_example());
  assert_required_matches_serde::<UpdateMe>(me::update_example());

  // The preferences body is read as whatever JSON arrives, so there is no required list to compare
  // — only that the example is JSON the reader accepts.
  serde_json::from_value::<serde_json::Value>(me::prefs_example()).expect("preferences are JSON");
}

/// Does the server stand up every path the fragment names on this surface?
///
/// The source scans below read text. This one puts real requests into the assembled router, so a
/// path whose syntax is not what axum reads, or a mount that has drifted, comes back a 404.
///
/// It does not separate methods: the route gate answers before a 405 would be raised.
#[tokio::test]
async fn every_path_the_fragment_names_is_served() {
  let router = router();

  for path in fragment().paths.paths.keys().filter(|path| ours(path)) {
    let response = router
      .clone()
      .oneshot(Request::builder().uri(path).body(Body::empty()).expect("a request"))
      .await
      .expect("a router does not fail");

    assert_ne!(response.status(), StatusCode::NOT_FOUND, "nobody stands up `{path}`");
  }
}

/// Is a route the fragment calls open actually reachable without a credential, and is one it calls
/// guarded actually closed?
///
/// This is the statement the whole surface turns on. A client built from a document that marks the
/// sign-in route as needing a credential cannot sign in at all, and one that marks a guarded route
/// as open sends the first request of every session into a 401.
///
/// Nothing is presented, so the guard answers 401 wherever it stands. Where it does not, the
/// request gets past it and is refused by the body reader instead — every open route on this
/// surface reads a body — which is why nothing here reaches the database.
#[tokio::test]
async fn a_route_is_open_exactly_where_the_fragment_says_it_is() {
  let router = router();
  let document = document();

  for (path, method, operation) in operations(&document).into_iter().filter(|(path, ..)| ours(path)) {
    // An operation is open when the empty requirement is one of the ways to satisfy it.
    let open = operation["security"]
      .as_array()
      .expect("an operation on this surface states its credentials")
      .iter()
      .any(|requirement| keys(requirement).is_empty());

    let response = router
      .clone()
      .oneshot(
        Request::builder()
          .method(method.to_uppercase().as_str())
          .uri(&path)
          .body(Body::empty())
          .expect("a request"),
      )
      .await
      .expect("a router does not fail");

    assert_eq!(
      response.status() == StatusCode::UNAUTHORIZED,
      !open,
      "{method} {path}: the fragment calls it {}, and the server answered {}",
      if open { "open" } else { "guarded" },
      response.status()
    );
  }
}

/// Does every credential an operation asks for exist as a scheme, and does every operation on this
/// surface say something about credentials at all?
///
/// A name that matches nothing under `securitySchemes` tells a client to authenticate with
/// something the document never describes. Silence is the subtler failure: a consumer's document may
/// declare a global requirement of its own, and an operation that says nothing inherits it — so
/// saying nothing here is a statement about somebody else's document rather than about this route.
#[test]
fn every_credential_an_operation_asks_for_is_declared() {
  let document = document();
  let declared: BTreeSet<String> = document["components"]["securitySchemes"]
    .as_object()
    .map(|schemes| schemes.keys().cloned().collect())
    .unwrap_or_default();

  assert!(!declared.is_empty(), "a surface that authenticates describes no way of authenticating");

  for (path, method, operation) in operations(&document) {
    match operation.get("security").and_then(JsonValue::as_array) {
      Some(requirements) => {
        for requirement in requirements {
          for name in keys(requirement) {
            assert!(declared.contains(&name), "{method} {path}: asks for undeclared `{name}`");
          }
        }
      }
      None => assert!(!ours(&path), "{method} {path}: says nothing about credentials"),
    }
  }
}

/// Does every operation on this surface answer to a name of its own?
///
/// A generated client turns an operation id into a method name, so two operations sharing one is
/// two methods with the same name — and the id has to stay unique after the fragment is merged into
/// a document that already has operations of its own. Signing in and signing out exist twice here,
/// once for the cookie and once for the tokens, so this is exactly where the default (the handler's
/// own name) collides.
#[test]
fn every_operation_on_this_surface_answers_to_a_name_of_its_own() {
  let document = document();
  let mut seen = BTreeSet::new();

  for (path, method, operation) in operations(&document).into_iter().filter(|(path, ..)| ours(path)) {
    let id = operation["operationId"]
      .as_str()
      .unwrap_or_else(|| panic!("{method} {path}: has no operation id"));

    assert!(seen.insert(id.to_owned()), "{method} {path}: `{id}` is already some other operation");
  }
}

/// A fragment must not carry a document-level security requirement.
///
/// Merging appends it to the consumer's own, so one declared here would quietly demand our
/// credentials on the consumer's own routes.
#[test]
fn the_fragment_imposes_nothing_on_the_document_it_joins() {
  assert!(
    document().get("security").is_none(),
    "a fragment states credentials per operation and never for a document"
  );
}

/// Does the fragment name a schema that is defined nowhere?
///
/// A wrapper type used as a query parameter is where this bites: a parameter emits a `$ref` without
/// registering what it points at, so a forgotten inline leaves a reference hanging.
#[test]
fn no_reference_in_the_fragment_dangles() {
  fn collect(value: &JsonValue, into: &mut BTreeSet<String>) {
    match value {
      JsonValue::Object(map) => {
        for (key, child) in map {
          if key == "$ref"
            && let Some(name) = child.as_str().and_then(|reference| reference.rsplit('/').next())
          {
            into.insert(name.to_owned());
          }
          collect(child, into);
        }
      }
      JsonValue::Array(items) => items.iter().for_each(|item| collect(item, into)),
      _ => {}
    }
  }

  let document = document();
  let defined: BTreeSet<String> = document["components"]["schemas"]
    .as_object()
    .map(|schemas| schemas.keys().cloned().collect())
    .unwrap_or_default();

  let mut referenced = BTreeSet::new();
  collect(&document, &mut referenced);

  let dangling: Vec<&String> = referenced.difference(&defined).collect();
  assert!(dangling.is_empty(), "points at schemas nobody defines: {dangling:?}");
}

/// Is the failure body this surface sends the one the document describes — media type, key names,
/// value types, and which of them may be left out?
///
/// The guard and the handlers both reach the wire through one conversion, so what is compared here
/// is how every refusal on this surface is written. `reason` rides only in a debug build; the
/// document calls it optional and says the same thing whichever build produced it, which is why
/// splitting on the build is this check's job alone.
#[tokio::test]
async fn the_failures_this_surface_raises_are_written_as_the_document_says() {
  // What a sign-in refuses with, what the guard refuses with, and the one refusal that is not a 401.
  let errors = [
    SumidaError::InvalidAccount,
    SumidaError::InactiveAccount,
    SumidaError::AccountBanned,
    SumidaError::NoCredentials,
    SumidaError::InvalidJwt,
    SumidaError::InvalidApiKey,
    SumidaError::InvalidAuthorizationToken,
    SumidaError::NoJwtSecret,
    SumidaError::InvalidAuthorizationMethod("Basic".to_owned()),
  ];

  let named = properties::<ErrorBody>();
  let required = required::<ErrorBody>();

  for error in errors {
    let status = StatusCode::from(error.clone());
    let response = error.clone().into_response();

    assert_eq!(response.status(), status);
    assert_eq!(
      response.headers().get(CONTENT_TYPE).map(|value| value.as_bytes()),
      Some(&b"application/json"[..]),
      "`{error}` leaves on a media type the document does not name"
    );

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("a body");
    let written: JsonValue = serde_json::from_slice(&body).expect("the body is JSON");
    let sent = keys(&written);

    assert!(sent.is_subset(&named), "a key the document does not name goes out: {sent:?}");
    assert!(required.is_subset(&sent), "a key the document requires is missing: {sent:?}");

    assert!(written["code"].is_string(), "`code` is a string");
    assert_eq!(written["statusCode"].as_u64(), Some(u64::from(status.as_u16())));

    match written.get("reason") {
      Some(reason) => assert!(reason.is_string(), "`reason` is a string"),
      None => assert!(!required.contains("reason")),
    }

    assert_eq!(
      sent.contains("reason"),
      cfg!(debug_assertions),
      "`reason` rides in a debug build and nowhere else, which is why the document calls it optional"
    );
  }

  // The two statuses the surface claims besides 401 come from this same mapping, and a change to it
  // would move them without touching a single annotation.
  assert_eq!(StatusCode::from(SumidaError::NoCredentials), StatusCode::UNAUTHORIZED);
  assert_eq!(
    StatusCode::from(SumidaError::InvalidAuthorizationMethod("Basic".to_owned())),
    StatusCode::NOT_IMPLEMENTED
  );
}

/// What a reader refuses, it refuses as plain text — the ground for the plain-text responses this
/// surface declares.
///
/// The reader belongs to axum rather than to this crate, so it does not move with the error layer.
/// If its wording changes, several statements in the document go false at once.
#[tokio::test]
async fn what_the_reader_refuses_comes_back_as_plain_text() {
  use api::service::auth::dto::LoginSession;
  use axum::http::HeaderValue;
  use axum::{Json, Router, routing};

  let router = Router::new().route("/body", routing::post(|Json(_): Json<LoginSession>| async {}));

  let post = |content_type: Option<&'static str>, body: &'static str| {
    let mut request = Request::builder().method("POST").uri("/body");
    if let Some(content_type) = content_type {
      request = request.header(CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    request.body(Body::from(body)).expect("a request")
  };

  let cases = [
    // Not JSON at all.
    (post(Some("application/json"), "{"), StatusCode::BAD_REQUEST),
    // JSON, but not the shape this operation takes.
    (post(Some("application/json"), "{}"), StatusCode::UNPROCESSABLE_ENTITY),
  ];

  for (request, expected) in cases {
    let response = router.clone().oneshot(request).await.expect("a router does not fail");

    assert_eq!(response.status(), expected);

    let media = response.headers().get(CONTENT_TYPE).expect("a media type");
    assert!(
      media.to_str().expect("a media type is text").starts_with("text/plain"),
      "the reader refused as `{media:?}` rather than as plain text"
    );
  }
}

/// The (method, path constant) pairs a controller source wires and describes.
///
/// The collection slot is the one place the two statements spell different strings — axum wants a
/// leading `/` and the document wants the prefix itself — so that pair is read as the same.
fn scan(source: &str, pattern: &str, method: usize, path: usize) -> BTreeSet<(String, String)> {
  fn normalize(path: &str) -> String {
    match path {
      "\"\"" => "\"/\"".to_owned(),
      other => other.to_owned(),
    }
  }

  regex::Regex::new(pattern)
    .expect("a pattern")
    .captures_iter(source)
    .map(|captured| (captured[method].to_owned(), normalize(&captured[path])))
    .collect()
}

fn wired(source: &str) -> BTreeSet<(String, String)> {
  scan(
    source,
    r#"\.route\(\s*("[^"]*"|[A-Z][A-Z_0-9]*)\s*,\s*routing::(get|post|put|patch|delete)\("#,
    2,
    1,
  )
}

fn described(source: &str) -> BTreeSet<(String, String)> {
  scan(
    source,
    r#"utoipa::path\(\s*(get|post|put|patch|delete),\s*path = ("[^"]*"|[A-Z][A-Z_0-9]*)\s*,"#,
    1,
    2,
  )
}

/// A route gained without a description, or a description left behind by a route, is caught here.
#[test]
fn every_route_is_described_and_every_description_has_a_route() {
  for (source, mount) in controllers() {
    let wired = wired(source);

    assert!(
      !wired.is_empty(),
      "`{mount}`: read no route at all — the scan and the source have parted"
    );
    assert_eq!(wired, described(source), "`{mount}`: wires and describes different routes");
  }
}

/// Did what was described reach the fragment?
///
/// An annotation can sit on a handler whose name was never added to the `paths` list, and the
/// operation goes quietly missing. The scans cannot see that gap, so it is counted.
#[test]
fn the_fragment_carries_every_route_the_controllers_mount() {
  let document = document();

  for (source, mount) in controllers() {
    let carried = operations(&document)
      .iter()
      .filter(|(path, ..)| path == mount || path.starts_with(&format!("{mount}/")))
      .count();

    assert_eq!(wired(source).len(), carried, "`{mount}`: describes routes the fragment does not carry");
  }
}

/// Is a body described exactly where a body is read?
///
/// The annotation only sits beside the handler, so it can demand a body the handler never reads and
/// hide one it does. Both are written in the same place, so annotation and signature are paired up.
#[test]
fn a_body_is_described_exactly_where_a_body_is_read() {
  let mut seen = 0;

  for (source, mount) in controllers() {
    for chunk in source.split("utoipa::path(").skip(1) {
      let (annotation, rest) = chunk.split_once("\n))]\n").expect("an annotation closes");
      let signature = rest.split_once(" {\n").expect("a handler opens its body").0;
      let name = signature.split_once('(').expect("a signature opens a paren").0;

      // A body is read by taking `Json(...)` apart. Handing one back as `Json<...>` is not reading
      // one.
      assert_eq!(
        annotation.contains("request_body"),
        signature.contains("Json("),
        "`{mount}`: `{name}` describes and reads different things"
      );

      seen += 1;
    }
  }

  assert!(seen > 0, "read no operation at all — the scan and the source have parted");
}

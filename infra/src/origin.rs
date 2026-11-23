//! Which page a browser says a request came from, and whether this server answers that page.
//!
//! A browser names the page behind a request in `Origin` — on every cross-site request that is not a
//! plain navigation, and on every WebSocket handshake — and, where it leaves `Origin` out, usually
//! still in `Referer`. A request carrying the visitor's cookie from another site's page is exactly
//! what this judges, so the socket handshake and the cookie-authenticated HTTP request ask the same
//! question here and nowhere else.

use axum::http::header::{HOST, ORIGIN, REFERER};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Uri};

use crate::settings;

/// The origins the deployment names besides its own: `server.cors.allow_origins`, without the `*`
/// the CORS layer ignores.
pub fn allowed_origins() -> &'static [String] {
  settings::get()
    .server
    .cors
    .as_ref()
    .map_or(&[][..], |cors| cors.allow_origins.as_slice())
}

/// Whether the page a request came from is one this server answers.
///
/// The page is the request's `Origin`, or, when it carries none, the origin of its `Referer`. It is
/// answered when it is listed in `allowed` or is the server's own — the authority it names is the
/// one this request was sent to (`Host`, or the request target's authority over HTTP/2). A request
/// that names no page at all did not come from another site's page, which a browser always names on
/// such a request, and is left to its credentials.
pub fn source_allowed(parts: &Parts, allowed: &[String]) -> bool {
  let headers = &parts.headers;
  let origin = match (headers.get(ORIGIN), headers.get(REFERER)) {
    (Some(origin), _) => match origin.to_str() {
      Ok(origin) => origin.to_owned(),
      Err(_) => return false,
    },
    (None, Some(referer)) => match referer_origin(referer.to_str().ok()) {
      Some(origin) => origin,
      None => return false,
    },
    (None, None) => return true,
  };

  if allowed.iter().any(|allowed| allowed != "*" && *allowed == origin) {
    return true;
  }

  let authority = origin.split_once("://").map_or(origin.as_str(), |(_, authority)| authority);
  own_authorities(headers, &parts.uri).any(|own| own == authority)
}

/// The `scheme://authority` a `Referer` names, or `None` when it names no absolute URL.
fn referer_origin(referer: Option<&str>) -> Option<String> {
  let uri: Uri = referer?.parse().ok()?;
  Some(format!("{}://{}", uri.scheme_str()?, uri.authority()?))
}

/// The authorities this request was addressed to.
fn own_authorities<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> impl Iterator<Item = &'a str> {
  let host = headers.get(HOST).and_then(|host| host.to_str().ok());
  let target = uri.authority().map(|authority| authority.as_str());
  host.into_iter().chain(target)
}

#[cfg(test)]
mod tests {
  use axum::http::Request;
  use axum::http::header::{HOST, ORIGIN, REFERER};
  use axum::http::request::Parts;

  use super::source_allowed;

  fn request(origin: Option<&str>, referer: Option<&str>, host: &str) -> Parts {
    let mut request = Request::builder().uri("/users").header(HOST, host);
    if let Some(origin) = origin {
      request = request.header(ORIGIN, origin);
    }
    if let Some(referer) = referer {
      request = request.header(REFERER, referer);
    }
    request.body(()).unwrap().into_parts().0
  }

  #[test]
  fn another_sites_page_is_refused() {
    let allowed = vec!["https://app.example.com".to_owned()];

    assert!(!source_allowed(
      &request(Some("https://evil.example"), None, "api.example.com"),
      &allowed
    ));
  }

  #[test]
  fn a_listed_origin_and_the_servers_own_are_admitted() {
    let allowed = vec!["https://app.example.com".to_owned()];

    assert!(source_allowed(
      &request(Some("https://app.example.com"), None, "api.example.com"),
      &allowed
    ));
    assert!(source_allowed(
      &request(Some("https://api.example.com"), None, "api.example.com"),
      &allowed
    ));
  }

  #[test]
  fn a_wildcard_in_the_list_admits_nobody_by_itself() {
    let allowed = vec!["*".to_owned()];

    assert!(!source_allowed(
      &request(Some("https://evil.example"), None, "api.example.com"),
      &allowed
    ));
  }

  #[test]
  fn an_opaque_origin_is_refused() {
    assert!(!source_allowed(&request(Some("null"), None, "api.example.com"), &[]));
  }

  #[test]
  fn without_an_origin_the_referer_names_the_page() {
    let host = "api.example.com";

    assert!(!source_allowed(&request(None, Some("https://evil.example/form.html"), host), &[]));
    assert!(source_allowed(&request(None, Some("https://api.example.com/app/page?x=1"), host), &[]));
    assert!(!source_allowed(&request(None, Some("not a url"), host), &[]));
  }

  #[test]
  fn the_origin_wins_over_the_referer() {
    let host = "api.example.com";

    assert!(!source_allowed(
      &request(Some("https://evil.example"), Some("https://api.example.com/"), host),
      &[]
    ));
  }

  #[test]
  fn a_request_that_names_no_page_is_left_to_its_credentials() {
    assert!(source_allowed(&request(None, None, "api.example.com"), &[]));
  }
}

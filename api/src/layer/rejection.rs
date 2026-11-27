//! The body a refused request reads when the framework, not a handler, refuses it.
//!
//! A body that does not parse, a missing or wrong media type and the like are refused by the request
//! readers before any handler runs, and they answer in plain text with the parser's own sentence —
//! the language, the framework and the names of the server's request types. Every other refusal
//! goes out as the error envelope, so these are rewritten into it: same status, the bad-request code,
//! and nothing of the sentence, which is logged instead.

use axum::Json;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// The code the envelope carries for a request the server could not read.
const BAD_REQUEST_CODE: &str = "E6000";

/// Rewrites a plain-text refusal of the request readers into the error envelope.
pub async fn envelope(response: Response) -> Response {
  let status = response.status();
  let plain = response
    .headers()
    .get(CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|value| value.starts_with("text/plain"));
  let refused_reading = matches!(
    status,
    StatusCode::BAD_REQUEST
      | StatusCode::PAYLOAD_TOO_LARGE
      | StatusCode::UNSUPPORTED_MEDIA_TYPE
      | StatusCode::UNPROCESSABLE_ENTITY
  );

  if !(plain && refused_reading) {
    return response;
  }

  if tracing::enabled!(tracing::Level::DEBUG) {
    let body = axum::body::to_bytes(response.into_body(), 4096).await.unwrap_or_default();
    tracing::debug!("request refused before a handler: {status} {}", String::from_utf8_lossy(&body));
  }

  (status, Json(json!({ "code": BAD_REQUEST_CODE, "statusCode": status.as_u16() }))).into_response()
}

#[cfg(test)]
mod tests {
  use axum::body::Body;
  use axum::http::{Response, StatusCode};

  use super::envelope;

  async fn body_of(response: axum::response::Response) -> String {
    String::from_utf8(axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec())
      .unwrap()
  }

  #[tokio::test]
  async fn a_readers_plain_refusal_becomes_the_envelope_without_its_sentence() {
    let refused = Response::builder()
      .status(StatusCode::UNPROCESSABLE_ENTITY)
      .header("content-type", "text/plain; charset=utf-8")
      .body(Body::from(
        "Failed to deserialize the JSON body into the target type: expected struct LoginSession",
      ))
      .unwrap();

    let rewritten = envelope(refused).await;
    assert_eq!(rewritten.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_of(rewritten).await;
    assert_eq!(body, r#"{"code":"E6000","statusCode":422}"#);
    assert!(!body.contains("LoginSession"));
  }

  #[tokio::test]
  async fn other_responses_pass_untouched() {
    let ok = Response::builder()
      .status(StatusCode::OK)
      .header("content-type", "text/plain")
      .body(Body::from("fine"))
      .unwrap();
    assert_eq!(body_of(envelope(ok).await).await, "fine");

    let json = Response::builder()
      .status(StatusCode::BAD_REQUEST)
      .header("content-type", "application/json")
      .body(Body::from(r#"{"code":"E6000","statusCode":400}"#))
      .unwrap();
    assert_eq!(body_of(envelope(json).await).await, r#"{"code":"E6000","statusCode":400}"#);
  }
}

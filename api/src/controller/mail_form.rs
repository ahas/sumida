use axum::extract::{Multipart, Path, Query, State};
use axum::{Json, Router, routing};
#[cfg(feature = "storage-aws-s3")]
use infra::aws;
use infra::settings;
use serde_json::json;

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{EmptyObject, ErrorBody, MAIL_FORM_TAG, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};
#[cfg(feature = "storage-aws-s3")]
use crate::util::s3;

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/mail-forms";

const ONE: &str = "/{kind}";
const FILES: &str = "/files";

#[derive(Clone, Debug, Default)]
pub struct MailFormController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> MailFormController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// What an image dropped into the template editor comes back as.
///
/// It is a doc-only shape: the handler builds the object by hand rather than from a type, so this
/// stands beside it and `the_upload_answer_is_the_one_the_handler_builds` holds the two together.
#[cfg(feature = "openapi")]
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct MailFormFileUploaded {
  /// Where the file can be fetched from afterwards, built from the deployment's CDN host — or, when
  /// none is configured, from its own host.
  pub url: String,
}

/// The form of a `POST /mail-forms/files` request. The handler reads the parts itself rather than
/// through an extractor, so the shape is written out here.
#[cfg(feature = "openapi")]
#[derive(utoipa::ToSchema)]
pub struct MailFormFileUpload {
  /// The bytes. The part **must** be named `upload`; every other part is skipped, and a request
  /// with no `upload` part is refused.
  #[schema(value_type = String, format = Binary)]
  pub upload: String,
}

/// The templates outgoing mail is rendered from, one row per kind.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_list",
  params(MailFormFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of templates, and how many there are behind it", body = List<MailFormListItem>),
    (
      status = 400,
      description = "A query-string value is not of the shape the reader takes (plain text), or \
                     the condition tree names a key, an operator or a size this list does not \
                     accept (error body).",
      content((String = "text/plain"), (ErrorBody = "application/json")),
    ),
    Refused,
  ),
))]
async fn list<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Query(filter): Query<MailFormFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<MailFormListItem>, SumidaError> {
  Ok(MailFormService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One template, with its body. The list leaves the body out; this is where it is read.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_get",
  params(("kind" = String, Path, description = "The template's kind, which is its key")),
  responses(
    (status = 200, description = "The template and its body", body = GetMailForm),
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(kind): Path<String>,
) -> Result<GetMailForm, SumidaError> {
  MailFormService::new(state, session)
    .get(&kind)
    .await?
    .ok_or_else(|| SumidaError::NoData("MailForm".to_owned(), kind.to_string()))
}

/// Add a template.
///
/// `kind` is the primary key and the name the mailer asks for by, so it is chosen by the caller
/// rather than assigned. A kind already in use is refused.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_create",
  request_body = CreateMailForm,
  responses(
    (status = 201, description = "The kind the template was stored under", body = MailFormCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateMailForm>,
) -> Result<MailFormCreated, SumidaError> {
  Ok(MailFormCreated {
    kind: MailFormService::new(state, session).create(body).await?.last_insert_id,
  })
}

/// Change a template. Only the fields sent are touched; `kind` may be sent to rename the row.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_update",
  params(("kind" = String, Path, description = "The template's kind, which is its key")),
  request_body = UpdateMailForm,
  responses(
    (status = 200, description = "Changed. The body is an empty object and names nothing", body = EmptyObject),
    Unreadable,
    WrongBody,
    NotFound,
    Refused,
  ),
))]
async fn update<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(kind): Path<String>,
  Json(body): Json<UpdateMailForm>,
) -> Result<MailFormUpdated, SumidaError> {
  MailFormService::new(state, session).update(kind, body).await?;
  Ok(MailFormUpdated)
}

/// Remove a template. Mail of that kind can no longer be rendered until one is added again.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_delete",
  params(("kind" = String, Path, description = "The template's kind, which is its key")),
  responses(
    (status = 200, description = "Removed. The body is an empty object and names nothing", body = EmptyObject),
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(kind): Path<String>,
) -> Result<MailFormDeleted, SumidaError> {
  MailFormService::new(state, session).delete(kind).await?;
  Ok(MailFormDeleted)
}

/// Put an image where a template body can point at it.
///
/// The request is `multipart/form-data` and the one part that is read is named `upload`; the first
/// such part is taken and the rest of the request is not looked at.
///
/// **The answer is a URL, not a promise that the bytes are there.** Where the deployment was built
/// with S3 storage, the file is written to the bucket before the answer goes out. Where it was not,
/// the same URL is answered and nothing is stored — this route has no local-storage path.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = FILES,
  context_path = MOUNT,
  tag = MAIL_FORM_TAG,
  operation_id = "mail_form_upload_files",
  request_body(content = MailFormFileUpload, content_type = "multipart/form-data"),
  responses(
    (status = 200, description = "Where the file can be fetched from", body = MailFormFileUploaded),
    (
      status = 501,
      description = "The request carried no part named `upload`, or the parts could not be read.",
      body = ErrorBody,
    ),
    Refused,
  ),
))]
async fn upload_files(mut multipart: Multipart) -> Result<Json<serde_json::Value>, SumidaError> {
  while let Some(field) =
    multipart.next_field().await.map_err(|e| SumidaError::Unsupported(e.to_string()))?
  {
    let Some(name) = field.name().map(|s| s.to_string()) else {
      continue;
    };

    if name != "upload" {
      continue;
    }

    let uuid = uuid::Uuid::new_v4().to_string();

    let settings = settings::get();
    let stage = &settings.server.stage;

    #[cfg(feature = "storage-aws-s3")]
    {
      let content_type = field
        .content_type()
        .map(|ct| ct.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());
      let data = field.bytes().await.map_err(|e| SumidaError::Unsupported(e.to_string()))?;
      let bucket = s3::get_bucket().map_err(|e| SumidaError::Unsupported(e.to_string()))?;
      let client = aws_sdk_s3::Client::new(aws::get_sdk_config());

      client
        .put_object()
        .bucket(bucket)
        .key(format!("static/{stage}/public/uploads/mail-form/{uuid}"))
        .content_type(content_type)
        .body(data.into())
        .send()
        .await?;
    }

    let host = settings.server.host.clone();
    let cdn_url = settings.server.cdn_url.clone().unwrap_or(host);

    return Ok(Json(json!({
      "url": format!("{cdn_url}/static/{stage}/uploads/mail-form/{uuid}")
    })));
  }

  Err(SumidaError::Unsupported("could not upload a file".to_owned()))
}

impl<S> ApiController<S> for MailFormController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "mail_form")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<MailFormFilter>()
      .create(
        Router::new()
          .route("/", routing::post(create::<S>))
          .route(FILES, routing::post(upload_files)),
      )
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

#[cfg(all(test, feature = "openapi"))]
mod tests {
  use super::*;

  /// The upload answer is built as a value, so the shape written beside it is the only description
  /// there is. This reads the value back through that shape.
  #[test]
  fn the_upload_answer_is_the_one_the_handler_builds() {
    let built = json!({ "url": "https://cdn.example/static/dev/uploads/mail-form/abc" });
    let read: MailFormFileUploaded = serde_json::from_value(built).expect("the answer is readable");

    assert!(read.url.ends_with("/abc"));
  }
}

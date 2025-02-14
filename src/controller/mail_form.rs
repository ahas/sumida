use axum::extract::Multipart;
use axum::extract::{Path, Query, State};
use axum::{routing, Json, Router};
use serde_json::json;

#[cfg(feature = "aws")]
use crate::core::aws;
use crate::core::error::SumidaError;
use crate::core::settings;
use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
#[cfg(feature = "aws-s3")]
use crate::util::s3;
use crate::{admin_guard, user_guard};

async fn list(
  _session: UserSession,
  Query(filter): Query<MailFormFilter>,
  Query(pagination): Query<Pagination>,
) -> Result<List<MailFormListItem>, SumidaError> {
  Ok(MailFormService::list(filter, pagination).await?.into())
}

async fn get<S>(State(_state): State<S>, Path(kind): Path<String>) -> Result<GetMailForm, SumidaError> {
  match MailFormService::get(&kind).await? {
    Some(mail_form) => Ok(mail_form),
    None => Err(SumidaError::NoData("MailForm".to_owned(), kind.to_string())),
  }
}

async fn create<S>(
  State(_state): State<S>,
  Json(body): Json<CreateMailForm>,
) -> Result<MailFormCreated, SumidaError> {
  Ok(MailFormCreated {
    kind: MailFormService::create(body).await?.last_insert_id,
  })
}

async fn update<S>(
  State(_state): State<S>,
  Path(kind): Path<String>,
  Json(body): Json<UpdateMailForm>,
) -> Result<MailFormUpdated, SumidaError> {
  MailFormService::update(kind, body).await?;
  Ok(MailFormUpdated)
}

async fn delete<S>(
  State(_state): State<S>,
  Path(kind): Path<String>,
) -> Result<MailFormDeleted, SumidaError> {
  MailFormService::delete(kind).await?;
  Ok(MailFormDeleted)
}

async fn upload_files(mut multipart: Multipart) -> Result<Json<serde_json::Value>, SumidaError> {
  while let Some(field) = multipart.next_field().await.unwrap() {
    let name = field.name().unwrap().to_string();

    if name != "upload" {
      continue;
    }

    let uuid = uuid::Uuid::new_v4().to_string();

    let settings = settings::get();
    let stage = &settings.server.stage;

    #[cfg(feature = "storage-aws-s3")]
    {
      let content_type = field.content_type().unwrap().to_string();
      let data = field.bytes().await.unwrap();
      let bucket = s3::get_bucket().unwrap();
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

pub fn new<S>(state: &S) -> Router<S>
where
  S: Clone + Send + Sync + 'static,
{
  let admin_guard_layer = admin_guard!(state);

  let route_read = Router::new()
    .route("/", routing::get(list))
    .route("/{kind}", routing::get(get::<S>))
    .route_layer(admin_guard_layer);
  let route_create = Router::new()
    .route("/", routing::post(create::<S>))
    .route("/files", routing::post(upload_files))
    .route_layer(admin_guard!(state));
  let route_update =
    Router::new().route("/{kind}", routing::patch(update::<S>)).route_layer(admin_guard!(state));
  let route_delete =
    Router::new().route("/{kind}", routing::delete(delete::<S>)).route_layer(admin_guard!(state));

  Router::<S>::new()
    .merge(route_read)
    .merge(route_create)
    .merge(route_update)
    .merge(route_delete)
    .route_layer(user_guard!(state))
}

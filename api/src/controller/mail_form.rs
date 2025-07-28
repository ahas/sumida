use axum::extract::Multipart;
use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};
#[cfg(feature = "storage-aws-s3")]
use core::aws;
use core::settings;
use serde_json::json;

use crate::service::prelude::*;
use crate::util::crud::{List, Pagination};
#[cfg(feature = "storage-aws-s3")]
use crate::util::s3;
use crate::{service_ap, user_guard};

use super::ApiController;

#[derive(Clone)]
pub struct MailFormController<S> {
  _state: std::marker::PhantomData<S>,
}

impl<S> MailFormController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }

  async fn list(
    user_ctx: UserContext,
    Query(filter): Query<MailFormFilter>,
    Query(pagination): Query<Pagination>,
  ) -> Result<List<MailFormListItem>, SumidaError> {
    Ok(MailFormService::list(&user_ctx, filter, pagination).await?.into())
  }

  async fn get(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(kind): Path<String>,
  ) -> Result<GetMailForm, SumidaError> {
    MailFormService::get(&user_ctx, &kind)
      .await?
      .ok_or_else(|| SumidaError::NoData("MailForm".to_owned(), kind.to_string()))
  }

  async fn create(
    user_ctx: UserContext,
    State(_state): State<S>,
    Json(body): Json<CreateMailForm>,
  ) -> Result<MailFormCreated, SumidaError> {
    Ok(MailFormCreated {
      kind: MailFormService::create(&user_ctx, body).await?.last_insert_id,
    })
  }

  async fn update(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(kind): Path<String>,
    Json(body): Json<UpdateMailForm>,
  ) -> Result<MailFormUpdated, SumidaError> {
    MailFormService::update(&user_ctx, kind, body).await?;
    Ok(MailFormUpdated)
  }

  async fn delete(
    user_ctx: UserContext,
    State(_state): State<S>,
    Path(kind): Path<String>,
  ) -> Result<MailFormDeleted, SumidaError> {
    MailFormService::delete(&user_ctx, kind).await?;
    Ok(MailFormDeleted)
  }

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
}

impl<S> ApiController<S> for MailFormController<S>
where
  S: Clone + Send + Sync + 'static,
{
  fn router(self, state: &S) -> axum::Router<S> {
    let route_read = Router::new()
      .route("/", routing::get(Self::list))
      .route("/{kind}", routing::get(Self::get))
      .route_layer(service_ap!(mail_form, read));
    let route_create = Router::new()
      .route("/", routing::post(Self::create))
      .route("/files", routing::post(Self::upload_files))
      .route_layer(service_ap!(mail_form, create));
    let route_update = Router::new() //
      .route("/{kind}", routing::patch(Self::update))
      .route_layer(service_ap!(mail_form, update));
    let route_delete = Router::new() //
      .route("/{kind}", routing::delete(Self::delete))
      .route_layer(service_ap!(mail_form, delete));

    Router::<S>::new()
      .merge(route_read)
      .merge(route_create)
      .merge(route_update)
      .merge(route_delete)
      .route_layer(user_guard!(state))
  }
}

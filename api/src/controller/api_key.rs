use axum::extract::{Path, Query, State};
use axum::{Json, Router, routing};

use super::{ApiController, ControllerRouter};
use crate::SumidaState;
#[cfg(feature = "openapi")]
use crate::openapi::{API_KEY_TAG, EmptyObject, ErrorBody, NotFound, Refused, Unreadable, WrongBody};
use crate::service::prelude::*;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/api-keys";

/// The path segment is spelled `{id}`, and what it takes is the **hashed** key — the same string
/// the list and the read answer with. The plaintext is not an identifier and is never accepted here.
const ONE: &str = "/{id}";

#[derive(Clone, Debug, Default)]
pub struct ApiKeyController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> ApiKeyController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The keys issued in this installation.
///
/// `key` is the stored **hash**, not the credential — the plaintext exists once, in the answer to
/// the issuing request. `keyPrefix` is the first eight characters of that plaintext and is kept so a
/// person can tell two keys apart on a screen; it is not a secret and it is not enough to sign in.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = "",
  context_path = MOUNT,
  tag = API_KEY_TAG,
  operation_id = "api_key_list",
  params(ApiKeyFilter, ConditionQuery, Pagination),
  responses(
    (status = 200, description = "One page of keys, and how many there are behind it", body = List<ApiKeyListItem>),
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
  Query(filter): Query<ApiKeyFilter>,
  Query(conditions): Query<ConditionQuery>,
  Query(pagination): Query<Pagination>,
) -> Result<List<ApiKeyListItem>, SumidaError> {
  Ok(ApiKeyService::new(state, session).list(filter, conditions, pagination).await?.into())
}

/// One key, with the grants written against it.
///
/// `accessPolicies` is filled only for a key issued as secured; an unsecured key carries no grants
/// of its own because it acts with everything its owner may do.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = ONE,
  context_path = MOUNT,
  tag = API_KEY_TAG,
  operation_id = "api_key_get",
  params(("id" = String, Path, description = "The stored hash of the key, as the list reports it")),
  responses(
    (status = 200, description = "The key's record. Never the credential itself", body = GetApiKey),
    NotFound,
    Refused,
  ),
))]
async fn get<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(key): Path<String>,
) -> Result<GetApiKey, SumidaError> {
  ApiKeyService::new(state, session)
    .get(&key)
    .await?
    .ok_or_else(|| SumidaError::NoData("ApiKey".to_owned(), key.to_string()))
}

/// Issue a key.
///
/// **The credential comes back here and nowhere else.** Only its hash is stored, so a key that is
/// not written down at this moment cannot be recovered — it can only be deleted and issued again.
/// Present it afterwards as `Authorization: APIKEY <key>`.
///
/// A key is issued against an account, and whoever holds it acts as that account. Issuing one in
/// somebody else's name is refused unless the caller is an administrator, because a key that
/// inherits its owner's permissions would otherwise be a way to take the account over.
///
/// `isSecured` decides the reach: secured, the key carries only the grants sent as
/// `accessPolicies`; unsecured, it carries everything its owner may do and `accessPolicies` is
/// ignored.
#[cfg_attr(feature = "openapi", utoipa::path(
  post,
  path = "",
  context_path = MOUNT,
  tag = API_KEY_TAG,
  operation_id = "api_key_create",
  request_body = CreateApiKey,
  responses(
    (status = 201, description = "The plaintext key. It is not retrievable again", body = ApiKeyCreated),
    Unreadable,
    WrongBody,
    Refused,
  ),
))]
async fn create<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Json(body): Json<CreateApiKey>,
) -> Result<ApiKeyCreated, SumidaError> {
  // The plaintext key returned by create() is exposed only in this response (DB stores only the hash).
  Ok(ApiKeyCreated {
    key: ApiKeyService::new(state, session).create(body).await?,
  })
}

/// Change a key's expiry, its grants, or whether it is secured.
///
/// The credential itself never changes: there is no rotation here, only issuing a new key and
/// deleting the old one.
#[cfg_attr(feature = "openapi", utoipa::path(
  patch,
  path = ONE,
  context_path = MOUNT,
  tag = API_KEY_TAG,
  operation_id = "api_key_update",
  params(("id" = String, Path, description = "The stored hash of the key, as the list reports it")),
  request_body = UpdateApiKey,
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
  Path(key): Path<String>,
  Json(body): Json<UpdateApiKey>,
) -> Result<ApiKeyUpdated, SumidaError> {
  ApiKeyService::new(state, session).update(&key, body).await?;
  Ok(ApiKeyUpdated)
}

/// Revoke a key. It stops admitting requests at once, and the grants written against it go with it.
#[cfg_attr(feature = "openapi", utoipa::path(
  delete,
  path = ONE,
  context_path = MOUNT,
  tag = API_KEY_TAG,
  operation_id = "api_key_delete",
  params(("id" = String, Path, description = "The stored hash of the key, as the list reports it")),
  responses(
    (status = 200, description = "Revoked. The body is an empty object and names nothing", body = EmptyObject),
    NotFound,
    Refused,
  ),
))]
async fn delete<S: SumidaState>(
  session: UserSession,
  State(state): State<S>,
  Path(key): Path<String>,
) -> Result<ApiKeyDeleted, SumidaError> {
  ApiKeyService::new(state, session).delete(&key).await?;
  Ok(ApiKeyDeleted)
}

impl<S> ApiController<S> for ApiKeyController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    ControllerRouter::new(state, "api_key")
      .read(
        Router::new() //
          .route("/", routing::get(list::<S>))
          .route(ONE, routing::get(get::<S>)),
      )
      .descriptor::<ApiKeyFilter>()
      .create(Router::new().route("/", routing::post(create::<S>)))
      .update(Router::new().route(ONE, routing::patch(update::<S>)))
      .delete(Router::new().route(ONE, routing::delete(delete::<S>)))
      .build()
  }
}

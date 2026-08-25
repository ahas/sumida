use axum::{Json, Router, routing};
use error::SumidaError;

use super::ApiController;
use crate::authz::schema_view::SchemaView;
#[cfg(feature = "openapi")]
use crate::openapi::{AUTHZ_TAG, Refused};
use crate::{SumidaState, user_guard};

/// Where these routes stand. The router nests here and the document hangs its operations here.
pub const MOUNT: &str = "/authz";

const SCHEMA: &str = "/schema";

/// `GET /authz/schema` — the vocabulary the `/relation-tuples` form is built from.
///
/// **Gated at authentication and no further, deliberately.**
///
/// It carries no grant. Not a subject, not an object id, not a `created_by` — only which relations
/// and permissions *exist*, which is the content of `schema.sz`, a file compiled into a binary
/// that ships to four products. So there is no authorization state here to leak, and the only thing
/// a session requirement buys is that the resource model is not enumerable by an anonymous
/// caller — cheap, and uniform with every other route in this tree.
///
/// What it must **not** be gated on is `service:access_policy#can_read`, which is the obvious wrong
/// answer. Two callers need this payload and neither of them administers policy: the
/// `/relation-tuples` form needs the relation names to render its selects *before* the server has
/// told it anything, and any screen that wants to display a permission from `/me/permissions` needs
/// the vocabulary to name it. Gating the dictionary behind the capability to edit the entries would
/// leave those callers hardcoding relation strings — which is the state this endpoint exists to end.
///
/// One thing it withholds: the `Derivation` behind a derived relation, which names tables, columns
/// and a raw SQL fragment. See `authz::schema_view`.
#[derive(Clone, Debug, Default)]
pub struct AuthzController<S: SumidaState> {
  _state: std::marker::PhantomData<S>,
}

impl<S: SumidaState> AuthzController<S> {
  pub fn new() -> Self {
    Self {
      _state: std::marker::PhantomData,
    }
  }
}

/// The relations and permissions this deployment knows.
///
/// This is a dictionary, not a grant: it says what *can* be written and asked for, never what the
/// caller holds. Read it once at start-up and build the grant form's selects from it instead of
/// hardcoding relation names — a name that is not here is a grant the evaluator will never read,
/// and writing one stores authority that matches nothing.
///
/// A relation whose `kind` is `derived` comes from a domain column rather than from a stored row.
/// It must not be offered as something a person can grant, and writing one is refused.
///
/// The rules are compiled into the binary, so this changes only when a new build is deployed and
/// there is no route that edits it. Both lists are sorted, which is what makes two deployments'
/// schemas comparable.
#[cfg_attr(feature = "openapi", utoipa::path(
  get,
  path = SCHEMA,
  context_path = MOUNT,
  tag = AUTHZ_TAG,
  operation_id = "authz_schema",
  responses(
    (status = 200, description = "The whole vocabulary, sorted", body = SchemaView),
    Refused,
  ),
))]
async fn schema() -> Result<Json<SchemaView>, SumidaError> {
  Ok(Json(SchemaView::current()?))
}

impl<S> ApiController<S> for AuthzController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new() //
      .route(SCHEMA, routing::get(schema))
      .route_layer(user_guard!(state))
  }
}

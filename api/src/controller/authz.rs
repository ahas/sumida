use axum::{Json, Router, routing};
use error::SumidaError;

use super::ApiController;
use crate::authz::schema_view::SchemaView;
use crate::{SumidaState, user_guard};

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

  /// No database round trip and no session read: the registry is memoised behind a `OnceLock`, so
  /// this is a projection of process-local state.
  async fn get_schema() -> Result<Json<SchemaView>, SumidaError> {
    Ok(Json(SchemaView::current()?))
  }
}

impl<S> ApiController<S> for AuthzController<S>
where
  S: SumidaState,
{
  fn router(self, state: &S) -> axum::Router<S> {
    Router::new() //
      .route("/schema", routing::get(Self::get_schema))
      .route_layer(user_guard!(state))
  }
}

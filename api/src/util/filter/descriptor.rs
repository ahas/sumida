//! The declaration, served to the client that has to build a filter form out of it.
//!
//! It is a projection of process-local state — the resource's own `&'static [FieldSpec]` — so there
//! is no database round trip and nothing to keep in sync. What the endpoint answers is exactly what
//! the sort resolver reads and what a condition compiler will read: a client cannot be told about a
//! field the server would then refuse.
//!
//! It carries no labels and no language. Names are keys, not captions; how a key reads to a person
//! is the client's business.
//!
//! **The declaration is not authority.** Every field here is one a caller *may name*; whether any
//! row comes back is settled before the filter is applied, by the read scope on the list route. The
//! route sits behind the same session guard and the same read access-policy layer as the list it
//! describes, so it cannot enumerate a resource its caller could not list.

use axum::{Json, Router, routing};
use serde::Serialize;

use super::schema::FilterSchema;
use super::spec::FieldSpec;
use crate::SumidaState;
use crate::layer::ServiceApLayer;
use crate::util::pagination::{MAX_LIMIT, Pagination};

/// The path the descriptor is mounted at, under the resource's own prefix.
pub const DESCRIPTOR_PATH: &str = "/filters";

/// What the list envelope will accept, so a client can cap its own page size instead of discovering
/// the ceiling by being silently clamped.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterLimits {
  pub max_limit:     u64,
  pub default_limit: u64,
}

impl Default for FilterLimits {
  fn default() -> Self {
    Self {
      max_limit:     MAX_LIMIT,
      default_limit: Pagination::default().limit,
    }
  }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDescriptor {
  pub fields:       &'static [FieldSpec],
  /// The key this resource orders by when the request names none.
  pub default_sort: &'static str,
  pub limits:       FilterLimits,
}

impl FilterDescriptor {
  pub fn of<F>() -> Self
  where
    F: FilterSchema,
  {
    Self {
      fields:       F::fields(),
      default_sort: F::default_sort(),
      limits:       FilterLimits::default(),
    }
  }
}

async fn get_descriptor<F>() -> Json<FilterDescriptor>
where
  F: FilterSchema + Send + Sync + 'static,
{
  Json(FilterDescriptor::of::<F>())
}

/// `GET <resource>/filters` for any resource that declares a `FilterSchema`.
///
/// Merge it into the controller's router alongside the read routes, before the session guard the
/// controller applies at its top level — it is the same pair of gates the list route sits behind,
/// and the namespace passed here is the one the list route already gates on.
///
/// # Panics
///
/// A declaration that fails `FilterSchema::validate` panics here, while the router is being built.
/// Every such failure is a wiring mistake — a reserved key, an operator the field cannot mean, a
/// default sort that is not sortable — and it is the same class of failure as a rule that does not
/// compile: it must stop the process, not the request that happens to trip over it.
pub fn filter_descriptor_router<S, F>(state: &S, namespace: &str) -> Router<S>
where
  S: SumidaState,
  F: FilterSchema + Send + Sync + 'static,
{
  if let Err(error) = F::validate() {
    panic!("[FilterSchema]: {error}");
  }

  Router::new()
    .route(DESCRIPTOR_PATH, routing::get(get_descriptor::<F>))
    .route_layer(ServiceApLayer::new(state.clone(), namespace, "read"))
}

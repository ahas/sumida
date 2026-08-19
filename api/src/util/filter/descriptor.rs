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
//! route sits behind whatever gate the list route already stands behind, so it cannot enumerate a
//! resource its caller could not list.

use axum::{Json, Router, routing};
use serde::Serialize;

use super::condition::{
  MAX_CONDITION_BYTES,
  MAX_CONDITION_DEPTH,
  MAX_CONDITION_FULL_TEXT,
  MAX_CONDITION_NODES,
  MAX_CONDITION_SUBSTRING,
  MAX_CONDITION_TEXT_LENGTH,
  MAX_CONDITION_VALUES,
};
use super::schema::FilterSchema;
use super::spec::{FieldSpec, OPERATORS, OperatorSpec};
use crate::SumidaState;
use crate::layer::ServiceApLayer;
use crate::util::pagination::{MAX_LIMIT, Pagination};

/// The path the descriptor is mounted at, under the resource's own prefix.
pub const DESCRIPTOR_PATH: &str = "/filters";

/// What the list envelope will accept, so a client can cap its own page size instead of discovering
/// the ceiling by being silently clamped, and refuse a condition tree it has already built rather
/// than sending one the server would answer with a bad request.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterLimits {
  pub max_limit:                 u64,
  pub default_limit:             u64,
  pub max_condition_bytes:       usize,
  pub max_condition_depth:       usize,
  pub max_condition_nodes:       usize,
  pub max_condition_values:      usize,
  pub max_condition_text_length: usize,
  pub max_condition_full_text:   usize,
  pub max_condition_substring:   usize,
}

impl Default for FilterLimits {
  fn default() -> Self {
    Self {
      max_limit:                 MAX_LIMIT,
      default_limit:             Pagination::default().limit,
      max_condition_bytes:       MAX_CONDITION_BYTES,
      max_condition_depth:       MAX_CONDITION_DEPTH,
      max_condition_nodes:       MAX_CONDITION_NODES,
      max_condition_values:      MAX_CONDITION_VALUES,
      max_condition_text_length: MAX_CONDITION_TEXT_LENGTH,
      max_condition_full_text:   MAX_CONDITION_FULL_TEXT,
      max_condition_substring:   MAX_CONDITION_SUBSTRING,
    }
  }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterDescriptor {
  pub fields:       &'static [FieldSpec],
  /// The key this resource orders by when the request names none.
  pub default_sort: &'static str,
  /// Every operator and how many operands it takes. The same list for every resource, because
  /// arity belongs to the operator; what a field accepts is the field's own declaration.
  pub operators:    &'static [OperatorSpec],
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
      operators:    OPERATORS,
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

/// `GET <resource>/filters` for any resource that declares a `FilterSchema`, carrying no gate of
/// its own.
///
/// Merge it into the branch that already holds the resource's read routes. Whatever that branch is
/// gated with is what the descriptor is gated with, which is the property the contract asks for: a
/// caller who cannot list cannot read the field list either. The base states the requirement and
/// mounts the route; which gate answers for it is the resource's own business.
///
/// # Panics
///
/// A declaration that fails `FilterSchema::validate` panics here, while the router is being built.
/// Every such failure is a wiring mistake — a reserved key, an operator the field cannot mean, a
/// default sort that is not sortable — and it is the same class of failure as a rule that does not
/// compile: it must stop the process, not the request that happens to trip over it.
pub fn filter_descriptor_route<S, F>() -> Router<S>
where
  S: Clone + Send + Sync + 'static,
  F: FilterSchema + Send + Sync + 'static,
{
  if let Err(error) = F::validate() {
    panic!("[FilterSchema]: {error}");
  }

  Router::new().route(DESCRIPTOR_PATH, routing::get(get_descriptor::<F>))
}

/// The same route with one read access-policy layer applied, for a resource whose list route is
/// gated on a namespace.
///
/// Pass the namespace the list route already gates on, and merge the result beside the read routes
/// so the controller's own top-level session guard covers it too.
///
/// # Panics
///
/// As [`filter_descriptor_route`], and for the same reason.
pub fn filter_descriptor_router<S, F>(state: &S, namespace: &str) -> Router<S>
where
  S: SumidaState,
  F: FilterSchema + Send + Sync + 'static,
{
  filter_descriptor_route::<S, F>().route_layer(ServiceApLayer::new(state.clone(), namespace, "read"))
}

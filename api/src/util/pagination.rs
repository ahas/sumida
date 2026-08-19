use axum::response::IntoResponse;
use sea_orm::{EntityTrait, IntoSimpleExpr, Order, QueryOrder, QueryTrait, Select};
use serde::{Deserialize, Deserializer, Serialize};

use crate::util::filter::FilterSchema;

/// The ceiling on a page. The descriptor endpoint serves it so a client can cap its own request
/// instead of discovering the limit by being silently clamped.
pub(crate) const MAX_LIMIT: u64 = 500;

fn deserialize_order<'de, D>(deserializer: D) -> Result<Order, D::Error>
where
  D: Deserializer<'de>,
{
  let s: String = String::deserialize(deserializer)?.to_lowercase();

  match s.as_str() {
    "desc" => Ok(Order::Desc),
    _ => Ok(Order::Asc),
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Pagination {
  pub page: u64,
  pub limit: u64,
  pub sort: Option<String>,
  #[serde(deserialize_with = "deserialize_order")]
  pub order: Order,
}

impl Pagination {
  pub fn paginate<E>(&self, mut query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    // page starts at 1. Use saturating so that even ?page=0 does not underflow u64 (panic / huge offset).
    let offset = self.page.saturating_sub(1) * self.limit;

    QueryTrait::query(&mut query).offset(offset).limit(std::cmp::min(self.limit, MAX_LIMIT));

    query
  }

  pub fn order_by<E, F, C>(&self, query: Select<E>, col_getter: F) -> Select<E>
  where
    E: EntityTrait,
    F: FnOnce(&str) -> C,
    C: IntoSimpleExpr + Sized,
  {
    query.order_by(col_getter(self.sort.as_deref().unwrap_or("")), self.order.clone())
  }

  /// Orders by `sort` resolved through the resource's own declaration, falling back to the key that
  /// declaration states as its default.
  ///
  /// A `sort` naming a field the resource does not declare, or declares but does not mark sortable,
  /// is not an error and not a column either — it falls back, exactly as a hand-written match arm
  /// did. What it can no longer do is order by something the descriptor never offered.
  ///
  /// Direction is the request's `order` in both cases; the declaration states a default key, not a
  /// default direction.
  pub fn order_by_schema<F, E>(&self, query: Select<E>) -> Select<E>
  where
    F: FilterSchema,
    E: EntityTrait,
  {
    let column = self
      .sort
      .as_deref()
      .and_then(F::sort_column)
      .or_else(|| F::sort_column(F::default_sort()));

    match column {
      Some(column) => query.order_by(column, self.order.clone()),
      // `FilterSchema::validate` runs where the resource mounts its routes and refuses a default
      // sort that cannot resolve, so reaching this is a wiring failure rather than a bad request.
      // Ordering is dropped rather than guessed: a wrong `ORDER BY` under `LIMIT` silently returns
      // the wrong page.
      None => {
        tracing::error!("filter: default sort `{}` did not resolve to a column", F::default_sort());
        query
      }
    }
  }
}

impl Default for Pagination {
  fn default() -> Self {
    Self {
      page: 1,
      limit: MAX_LIMIT,
      sort: None,
      order: Order::Asc,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct List<T> {
  items: Vec<T>,
  count: u64,
}

impl<T> List<T> {
  pub fn new(items: Vec<T>, count: u64) -> Self {
    Self { items, count }
  }
}

impl<T> IntoResponse for List<T>
where
  T: Serialize,
{
  fn into_response(self) -> axum::response::Response {
    axum::Json(self).into_response()
  }
}

impl<T> From<(Vec<T>, u64)> for List<T> {
  fn from(value: (Vec<T>, u64)) -> Self {
    Self::new(value.0, value.1)
  }
}

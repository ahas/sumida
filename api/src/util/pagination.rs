use axum::response::IntoResponse;
use sea_orm::{EntityTrait, IntoSimpleExpr, Order, QueryOrder, QueryTrait, Select};
use serde::{Deserialize, Deserializer, Serialize};

const MAX_LIMIT: u64 = 500;

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
    let offset = std::cmp::max((self.page - 1) * self.limit, 0);

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
  fn new(items: Vec<T>, count: u64) -> Self {
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

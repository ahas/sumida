use std::fmt;
use std::marker::PhantomData;

use axum::response::IntoResponse;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::{ColumnTrait, EntityTrait, IntoSimpleExpr, Order, QueryOrder, QueryTrait, Select};
use serde::de::Visitor;
use serde::de::{self, MapAccess};
use serde::{Deserialize, Deserializer, Serialize};

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
    let offset = (self.page - 1) * self.limit;

    QueryTrait::query(&mut query).offset(offset).limit(self.limit);

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
      limit: 500,
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

pub fn wrap_ilike_col<C, S>(col: C, text: S) -> SimpleExpr
where
  C: ColumnTrait,
  S: AsRef<str>,
{
  Expr::col(col).ilike(format!("%{text}%", text = text.as_ref()))
}

pub fn wrap_ilike<S>(col: Expr, text: S) -> SimpleExpr
where
  S: AsRef<str>,
{
  col.ilike(format!("%{text}%", text = text.as_ref()))
}

pub fn i32_or_struct<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
  T: Deserialize<'de> + From<i32>,
  D: Deserializer<'de>,
{
  struct I32OrStruct<T>(PhantomData<fn() -> T>);

  impl<'de, T> Visitor<'de> for I32OrStruct<T>
  where
    T: Deserialize<'de> + From<i32>,
  {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
      formatter.write_str("an integer or a map representing the struct")
    }

    fn visit_i32<E>(self, value: i32) -> Result<T, E>
    where
      E: de::Error,
    {
      Ok(T::from(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<T, E>
    where
      E: de::Error,
    {
      let id: i32 = value.parse().map_err(E::custom)?;
      Ok(T::from(id))
    }

    fn visit_map<M>(self, map: M) -> Result<T, M::Error>
    where
      M: MapAccess<'de>,
    {
      Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
    }
  }

  deserializer.deserialize_any(I32OrStruct(PhantomData))
}

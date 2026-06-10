use std::fmt;
use std::marker::PhantomData;

use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::{ColumnTrait, DbErr};
use serde::{Deserialize, Deserializer, de};

pub fn is_serialization_failure(error: &DbErr) -> bool {
  // Check for PostgreSQL's serialization failure error code 40001.
  match error {
    DbErr::Query(err) => err.to_string().contains("could not serialize access"),
    DbErr::Exec(err) => err.to_string().contains("could not serialize access"),
    _ => false,
  }
}

pub fn wrap_ilike<S>(col: Expr, text: S) -> SimpleExpr
where
  S: AsRef<str>,
{
  // Treat LIKE wildcards (%, _) and the escape character (\) as literals.
  // Without escaping, %/_ in user input would be interpreted as patterns and cause over-matching.
  let escaped = text.as_ref().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
  col.ilike(format!("%{escaped}%"))
}

pub fn tsquery<C, S>(col: C, search: S) -> SimpleExpr
where
  C: ColumnTrait,
  S: AsRef<str>,
{
  let col_name = format!(r#""{}""#, col.to_string());

  Expr::cust_with_values(format!(r#"{col_name} @@ plainto_tsquery('simple', $1)"#), [search.as_ref()])
}

pub fn i32_or_struct<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
  T: Deserialize<'de> + From<i32>,
  D: Deserializer<'de>,
{
  struct I32OrStruct<T>(PhantomData<fn() -> T>);

  impl<'de, T> de::Visitor<'de> for I32OrStruct<T>
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
      M: de::MapAccess<'de>,
    {
      Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
    }
  }

  deserializer.deserialize_any(I32OrStruct(PhantomData))
}

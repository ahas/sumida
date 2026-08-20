use std::fmt;
use std::marker::PhantomData;

use sea_orm::sea_query::extension::postgres::{PgBinOper, PgExpr};
use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::{DbErr, RuntimeErr, sqlx};
use serde::{Deserialize, Deserializer, de};

/// The SQLSTATE the database attached to this error, if it came from the database at all.
fn sqlstate(error: &DbErr) -> Option<String> {
  let runtime = match error {
    DbErr::Query(err) | DbErr::Exec(err) => err,
    _ => return None,
  };

  match runtime {
    RuntimeErr::SqlxError(sqlx::Error::Database(db)) => db.code().map(|code| code.into_owned()),
    _ => None,
  }
}

/// Whether the transaction failed for a reason that goes away on its own if the whole transaction
/// is run again.
///
/// Two SQLSTATEs say that. `40001` is a serialization failure: the database could not order this
/// transaction against a concurrent one. `40P01` is a deadlock: two transactions each held what the
/// other was waiting for, and this is the one the database chose to kill. Neither says anything is
/// wrong with the statement, and in both the transaction has already been rolled back — the only
/// answer is to run it again.
///
/// The decision reads the SQLSTATE rather than the message text. The text is localised by the
/// server's `lc_messages`, so matching on it stops working the moment the server is not answering
/// in English, and it stops silently: the retry never fires and the caller sees the failure instead.
pub fn is_retryable_conflict(error: &DbErr) -> bool {
  matches!(sqlstate(error).as_deref(), Some("40001" | "40P01"))
}

/// Treat LIKE wildcards (%, _) and the escape character (\) as literals.
/// Without escaping, %/_ in user input would be interpreted as patterns and cause over-matching.
fn escape_like(text: &str) -> String {
  text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// `ILIKE '%text%'` — the text anywhere inside the expression.
pub fn wrap_ilike<C, S>(col: C, text: S) -> SimpleExpr
where
  C: Into<SimpleExpr>,
  S: AsRef<str>,
{
  col.into().ilike(format!("%{}%", escape_like(text.as_ref())))
}

/// `ILIKE 'text%'` — the expression begins with the text.
pub fn starts_with_ilike<C, S>(col: C, text: S) -> SimpleExpr
where
  C: Into<SimpleExpr>,
  S: AsRef<str>,
{
  col.into().ilike(format!("{}%", escape_like(text.as_ref())))
}

/// `ILIKE '%text'` — the expression ends with the text.
pub fn ends_with_ilike<C, S>(col: C, text: S) -> SimpleExpr
where
  C: Into<SimpleExpr>,
  S: AsRef<str>,
{
  col.into().ilike(format!("%{}", escape_like(text.as_ref())))
}

/// `<tsvector> @@ plainto_tsquery('simple', $1)`.
///
/// The expression is the tsvector the GIN index sits on, so a full-text match reads the index
/// rather than the text it was built from. The term binds as a parameter.
pub fn tsquery<C, S>(col: C, search: S) -> SimpleExpr
where
  C: Into<SimpleExpr>,
  S: AsRef<str>,
{
  col.into().binary(
    PgBinOper::Matches,
    Expr::cust_with_values(r#"plainto_tsquery('simple', $1)"#, [search.as_ref()]),
  )
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

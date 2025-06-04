pub mod dto;

use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, QueryTrait, Set,
  TransactionTrait,
};

use crate::core::db;
use crate::core::error::SumidaError;
use crate::entity::prelude::*;
use crate::util::crud::{search, Pagination};

use self::dto::*;

pub async fn list(
  filter: MailFormFilter,
  pagination: Pagination,
) -> Result<(Vec<MailFormListItem>, u64), SumidaError> {
  use crate::entity::mail_forms::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = MailForms::find()
    .apply_if(filter.sender, |q, v| search(q, Column::Sender, v))
    .apply_if(filter.subject, |q, v| search(q, Column::Subject, v));
  let count = query.clone().select_only().column(Column::Kind).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "sender" => Column::Sender,
    "subject" => Column::Subject,
    _ => Column::Kind,
  });
  let query = pagination.paginate(query);
  let items = query.into_model::<MailFormListItem>().all(&txn).await?;

  txn.commit().await?;

  Ok((items, count))
}

pub async fn get<S>(kind: S) -> Result<Option<GetMailForm>, SumidaError>
where
  S: AsRef<str>,
{
  let db_conn = db::get_connection();
  Ok(MailForms::find_by_id(kind.as_ref()).into_model::<GetMailForm>().one(db_conn).await?)
}

pub async fn create(
  data: CreateMailForm,
) -> Result<InsertResult<crate::entity::mail_forms::ActiveModel>, SumidaError> {
  let db_conn = db::get_connection();
  Ok(MailForms::insert(data.into_active_model()).exec(db_conn).await?)
}

pub async fn update<S>(
  kind: S,
  data: UpdateMailForm,
) -> Result<crate::entity::mail_forms::Model, SumidaError>
where
  S: AsRef<str>,
{
  let mut model = data.into_active_model();
  model.kind = Set(kind.as_ref().to_string());

  let db_conn = db::get_connection();
  Ok(MailForms::update(model).exec(db_conn).await?)
}

pub async fn delete<S>(kind: S) -> Result<DeleteResult, SumidaError>
where
  S: AsRef<str>,
{
  let db_conn = db::get_connection();
  Ok(MailForms::delete_by_id(kind.as_ref()).exec(db_conn).await?)
}

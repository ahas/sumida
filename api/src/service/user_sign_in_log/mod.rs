pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{DeleteResult, IntoSimpleExpr, PaginatorTrait, QueryFilter, QuerySelect, QueryTrait};
use sea_orm::{EntityTrait, TransactionTrait};

use crate::util::crud::{Pagination, wrap_ilike_col};

use self::dto::*;

pub async fn list(
  filter: UserSignInLogFilter,
  pagination: Pagination,
) -> Result<(Vec<UserSignInLogListItem>, u64), SumidaError> {
  use entity::user_sign_in_logs::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let mut query = UserSignInLogs::find() //
    .apply_if(filter.username, |q, v| q.filter(wrap_ilike_col(Column::User, v)));
  let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

  query = pagination.order_by(query, |col| match col {
    "user" => Column::User.into_simple_expr(),
    _ => Column::CreatedAt.into_simple_expr(),
  });
  query = pagination.paginate(query);
  let items = query.into_model::<UserSignInLogListItem>().all(&txn).await?;

  txn.commit().await?;
  Ok((items, count))
}

pub async fn get(id: i32) -> Result<Option<GetUserSignInLog>, SumidaError> {
  let db_conn = db::get_connection();

  let item = UserSignInLogs::find_by_id(id).into_model::<GetUserSignInLog>().one(db_conn).await?;

  Ok(item)
}

pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
  let db_conn = db::get_connection();
  Ok(UserSignInLogs::delete_by_id(id).exec(db_conn).await?)
}

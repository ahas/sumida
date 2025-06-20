pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{DeleteResult, IntoSimpleExpr, JoinType, PaginatorTrait, QuerySelect, RelationTrait};
use sea_orm::{EntityTrait, TransactionTrait};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserLogService;

impl UserLogService {
  pub async fn list(
    filter: UserLogFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserLogListItem>, u64), SumidaError> {
    use entity::user_logs::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;
    let user_alias = Alias::new("user");

    let mut query = filter.apply(
      UserLogs::find()
        .select_only()
        .expr(Expr::col((UserLogs, Asterisk)))
        .expr_as(Expr::col((user_alias.clone(), "username")), "user_username")
        .expr_as(Expr::col((user_alias.clone(), "full_name")), "user_full_name")
        .join_as(JoinType::LeftJoin, entity::user_logs::Relation::User.def(), user_alias.clone()),
    );
    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "user" => Column::User.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserLogListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetUserLog>, SumidaError> {
    let db_conn = db::get_connection();
    let user_alias = Alias::new("user");

    let item = UserLogs::find_by_id(id)
      .select_only()
      .expr(Expr::col((UserLogs, Asterisk)))
      .expr_as(Expr::col((user_alias.clone(), "username")), "user_username")
      .expr_as(Expr::col((user_alias.clone(), "full_name")), "user_full_name")
      .join_as(JoinType::LeftJoin, entity::user_logs::Relation::User.def(), user_alias.clone())
      .into_model::<GetUserLog>()
      .one(db_conn)
      .await?;

    Ok(item)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(UserLogs::delete_by_id(id).exec(db_conn).await?)
  }
}

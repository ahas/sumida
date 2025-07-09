pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{DeleteResult, IntoSimpleExpr, JoinType, PaginatorTrait, QuerySelect, RelationTrait, Select};
use sea_orm::{EntityTrait, TransactionTrait};

use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserLogService;

impl UserLogService {
  pub fn select(query: Select<entity::user_logs::Entity>) -> Select<entity::user_logs::Entity> {
    let user_alias = Alias::new("user");
    let col_user_id = Expr::col((user_alias.clone(), "id"));
    let col_username = Expr::col((user_alias.clone(), "username"));
    let col_full_name = Expr::col((user_alias.clone(), "full_name"));

    query
      .select_only()
      .expr(Expr::col((entity::user_logs::Entity, Asterisk)))
      .expr_as(col_user_id, "user_id")
      .expr_as(col_username, "user_username")
      .expr_as(col_full_name, "user_full_name")
      .join_as(JoinType::LeftJoin, entity::user_logs::Relation::User.def(), user_alias.clone())
  }

  pub async fn list(
    filter: UserLogFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserLogListItem>, u64), SumidaError> {
    use entity::user_logs::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;
    let col_user_id = Expr::col("user_id");
    let col_username = Expr::col("user_username");
    let col_full_name = Expr::col("user_full_name");

    let mut query = filter.apply(Self::select(UserLogs::find()));
    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "userId" => col_user_id.into_simple_expr(),
      "username" => col_username.into_simple_expr(),
      "name" => col_full_name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserLogListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(id: i32) -> Result<Option<GetUserLog>, SumidaError> {
    Ok(Self::select(UserLogs::find_by_id(id)).into_model::<GetUserLog>().one(db::get_connection()).await?)
  }

  pub async fn delete(id: i32) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(UserLogs::delete_by_id(id).exec(db_conn).await?)
  }
}

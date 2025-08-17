pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::{Alias, Asterisk};
use sea_orm::{DeleteResult, IntoSimpleExpr, JoinType, PaginatorTrait, QuerySelect, RelationTrait, Select};
use sea_orm::{EntityTrait, TransactionTrait};

use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;

use self::dto::*;

pub struct UserLogService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> UserLogService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: UserLogFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserLogListItem>, u64), SumidaError> {
    use entity::user_logs::Column;

    let db = self.state.db();
    let col_user_id = Expr::col("user_id");
    let col_username = Expr::col("user_username");
    let col_full_name = Expr::col("user_full_name");

    let mut query = filter.apply(self.select(UserLogs::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "userId" => col_user_id.into_simple_expr(),
      "username" => col_username.into_simple_expr(),
      "name" => col_full_name.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserLogListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetUserLog>, SumidaError> {
    Ok(self.select(UserLogs::find_by_id(id)).into_model::<GetUserLog>().one(self.state.db()).await?)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self
      .ap()
      .action("delete")
      .try_access(self.select(UserLogs::find_by_id(id)), &txn)
      .await?;

    let result = UserLogs::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, UserLogs> for UserLogService<S> {
  fn default_query_with(query: Select<UserLogs>) -> Select<UserLogs> {
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

  fn ap(&self) -> ApCondition {
    use entity::user_logs::Column;

    ap::ns("user_log")
      .who(&self.session)
      .object_ref(Expr::col((UserLogs, Column::Id)).cast_as("varchar"))
  }

  fn select(&self, query: sea_orm::Select<UserLogs>) -> sea_orm::Select<UserLogs> {
    self
      .ap()
      .action("read")
      .apply(Self::default_query_with(query))
  }
}

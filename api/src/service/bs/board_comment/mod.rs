pub mod dto;

use self::dto::*;
use crate::SumidaState;
use crate::util::pagination::Pagination;
use entity::bs::board_comments::Column;
use entity::bs::board_comments::Entity as BoardComments;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardCommentService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardCommentService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardCommentFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardCommentListItem>, u64), SumidaError> {
    let txn = self.state.db().begin().await?;

    let mut query =
      filter.apply(BoardComments::find().select_only().expr(Expr::col((BoardComments, Asterisk))));

    let count = query.clone().select_only().column(Column::Id).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      _ => Column::Id.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardCommentListItem>().all(&txn).await?;

    txn.commit().await?;
    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardComment>, SumidaError> {
    Ok(BoardComments::find_by_id(id).into_partial_model::<GetBoardComment>().one(self.state.db()).await?)
  }

  pub async fn create(
    &self,
    data: CreateBoardComment,
  ) -> Result<InsertResult<entity::bs::board_comments::ActiveModel>, SumidaError> {
    Ok(BoardComments::insert(data.into_active_model()).exec(self.state.db()).await?)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardComment,
  ) -> Result<entity::bs::board_comments::Model, SumidaError> {
    let mut am = data.into_active_model();
    am.id = Set(id);
    Ok(BoardComments::update(am).exec(self.state.db()).await?)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(BoardComments::delete_by_id(id).exec(self.state.db()).await?)
  }
}

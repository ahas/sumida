pub mod dto;

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;
use entity::bs::board_comments::Column;
use entity::bs::board_comments::Entity as BoardComments;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::IntoActiveModel;
use sea_orm::IntoSimpleExpr;
use sea_orm::PaginatorTrait;
use sea_orm::Select;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{ActiveValue::Set, DeleteResult, EntityTrait, InsertResult, QuerySelect, TransactionTrait};

pub struct BoardCommentService<S: SumidaState> {
  state: S,
  #[allow(dead_code)]
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

    let mut query = filter.apply(self.select(BoardComments::find()));

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
    use crate::layer::board_ap::helpers as board_helpers;
    
    let txn = self.state.db().begin().await?;
    
    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_comment(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "read", &txn).await?;
    
    let result = self
      .select(BoardComments::find_by_id(id))
      .into_partial_model::<GetBoardComment>()
      .one(&txn)
      .await?;
    
    txn.commit().await?;
    
    Ok(result)
  }

  pub async fn create(
    &self,
    data: CreateBoardComment,
  ) -> Result<InsertResult<entity::bs::board_comments::ActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Check write permission by finding the post and getting board_id
    let post = BoardPosts::find_by_id(data.post_id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), data.post_id.to_string()))?;

    let condition = self.ap().action("write").object(format!("board:{}", post.board_id));
    if !condition.check_allowed(&txn).await {
      return Err(SumidaError::NoAccessPermission);
    }

    let result = BoardComments::insert(data.into_active_model()).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardComment,
  ) -> Result<entity::bs::board_comments::Model, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;
    
    let txn = self.state.db().begin().await?;
    
    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_comment(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "write", &txn).await?;
    
    // Then check comment-specific permission
    self.try_access("update", BoardComments::find_by_id(id), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardComments::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;
    
    let txn = self.state.db().begin().await?;
    
    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_comment(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "moderate", &txn).await?;
    
    // Then check comment-specific permission
    self.try_access("delete", BoardComments::find_by_id(id), &txn).await?;
    
    let result = BoardComments::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardComments> for BoardCommentService<S> {
  fn default_query_with(query: Select<BoardComments>) -> Select<BoardComments> {
    query.select_only().expr(Expr::col((BoardComments, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    ap::ns("board").who(&self.session)
  }
}

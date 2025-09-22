pub mod dto;

use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApCondition, ApGuard};
use crate::util::pagination::Pagination;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::{Asterisk, Expr};
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, IntoSimpleExpr, JoinType, PaginatorTrait,
  QuerySelect, RelationTrait, Select, TransactionTrait,
};

use self::dto::*;

pub struct BoardPostService<S: SumidaState> {
  state: S,
  #[allow(dead_code)]
  session: UserSession,
}

impl<S: SumidaState> BoardPostService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub fn query_with_views(&self, query: Select<BoardPosts>) -> Select<BoardPosts> {
    let views_alias = Alias::new("views");
    query
      .select_only()
      .expr(Expr::col((BoardPosts, Asterisk)))
      .expr(Expr::count(Expr::col((views_alias.clone(), "id"))))
      .join_as(JoinType::LeftJoin, entity::bs::board_posts::Relation::Views.def(), views_alias.clone())
  }

  pub async fn list(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostListItem>, u64), SumidaError> {
    use entity::bs::board_posts::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.query_with_views(self.select(BoardPosts::find())));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "title" => Column::Title.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_partial_model::<BoardPostListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;

    let txn = self.state.db().begin().await?;

    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_post(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "read", &txn).await?;

    // Then get the post
    let result = self
      .query_with_views(self.select(BoardPosts::find_by_id(id)))
      .into_partial_model::<GetBoardPost>()
      .one(&txn)
      .await?;

    txn.commit().await?;

    Ok(result)
  }

  pub async fn view(&self, session: &UserSession, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let post = self
      .query_with_views(self.select(BoardPosts::find_by_id(id)))
      .into_partial_model::<GetBoardPost>()
      .one(&txn)
      .await?;

    let mut view_am = entity::bs::board_post_views::ActiveModel {
      post_id: Set(id),
      ..Default::default()
    };

    if session.profile.is_authorized {
      view_am.user_id = Set(Some(session.profile.id));
    } else {
      view_am.ip_address = Set(Some(session.env.ip_address.clone()));
    }

    BoardPostViews::insert(view_am).exec(&txn).await?;

    txn.commit().await?;

    Ok(post)
  }

  pub async fn create(
    &self,
    data: CreateBoardPost,
  ) -> Result<InsertResult<entity::bs::board_posts::ActiveModel>, SumidaError> {
    // Check write permission for the board
    use entity::bs::boards;
    let txn = self.state.db().begin().await?;

    // Verify board exists and user has write permission
    let _board = boards::Entity::find_by_id(data.board_id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), data.board_id.to_string()))?;

    // Use ap condition to check board write permission
    let condition = self.ap().action("write").object(format!("board:{}", data.board_id));
    if !condition.check_allowed(&txn).await {
      return Err(SumidaError::NoAccessPermission);
    }

    let result = BoardPosts::insert(data.into_active_model()).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPost,
  ) -> Result<entity::bs::board_posts::Model, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;

    let txn = self.state.db().begin().await?;

    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_post(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "write", &txn).await?;

    // Then check post-specific permission
    self.try_access("update", BoardPosts::find_by_id(id), &txn).await?;

    let mut am = data.into_active_model();
    am.id = Set(id);

    let result = BoardPosts::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;

    let txn = self.state.db().begin().await?;

    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_post(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "moderate", &txn).await?;

    // Then check post-specific permission
    self.try_access("delete", BoardPosts::find_by_id(id), &txn).await?;

    let result = BoardPosts::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;
    Ok(result)
  }

  pub async fn update_with_permission(
    &self,
    id: i32,
    data: UpdateBoardPost,
  ) -> Result<entity::bs::board_posts::Model, SumidaError> {
    use crate::layer::board_ap::helpers as board_helpers;
    use crate::util::post_history::PostHistoryManager;

    let txn = self.state.db().begin().await?;

    // Get board_id and check hierarchical permission
    let board_id = board_helpers::get_board_id_from_post(id, &txn).await?;
    board_helpers::check_hierarchical_read_permission(&self.session, board_id, "write", &txn).await?;

    // Then check post-specific permission
    self.try_access("update", BoardPosts::find_by_id(id), &txn).await?;

    // Get current post for history tracking
    let current_post = BoardPosts::find_by_id(id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), id.to_string()))?;

    // Check if history is enabled for this board and content is being updated
    if !data.content.is_none() {
      let history_manager = PostHistoryManager::from_board_id(current_post.board_id, &txn).await?;

      if history_manager.is_history_enabled() {
        // Save current content to history before update
        if let Some(current_content) = &current_post.content {
          history_manager
            .save_history(id, &current_content.to_string(), self.session.profile.id, &txn)
            .await?;
        }
      }
    }

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardPosts::update(am).exec(&txn).await?;
    txn.commit().await?;

    Ok(item)
  }
}

impl<S: SumidaState> ApiService<ApCondition, BoardPosts> for BoardPostService<S> {
  fn default_query_with(query: Select<BoardPosts>) -> Select<BoardPosts> {
    query.select_only().expr(Expr::col((BoardPosts, Asterisk)))
  }

  fn ap(&self) -> ApCondition {
    use entity::bs::board_posts::Column;

    ap::ns("board")
      .who(&self.session)
      .object_ref(Expr::col((BoardPosts, Column::BoardId)).cast_as("varchar"))
  }
}

pub mod dto;

use crate::SumidaState;
use crate::service::ApiService;
use crate::service::prelude::BoardFileListItem;
use crate::util::ap::{self, ApChain, ApGuard};
use crate::util::pagination::Pagination;
use entity::bs::board_posts::BoardPostStatus;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Asterisk, Expr};
use sea_orm::{
  ColumnTrait, ConnectionTrait, DeleteResult, EntityTrait, InsertResult, IntoSimpleExpr, JoinType,
  PaginatorTrait, QueryFilter, QuerySelect, RelationTrait, Select, TransactionTrait,
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

  pub async fn list(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostListItem>, u64), SumidaError> {
    use entity::bs::board_posts::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(BoardPosts::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "title" => Column::Title.into_simple_expr(),
      _ => Column::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let items = query.into_model::<BoardPostListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Then get the post
    let mut result = self.select(BoardPosts::find_by_id(id)).into_model::<GetBoardPost>().one(&txn).await?;

    // Load attached files together
    if let Some(ref mut result) = result {
      result.attached_files = BoardFiles::find()
        .filter(entity::bs::board_files::Column::PostId.eq(id))
        .into_partial_model::<BoardFileListItem>()
        .all(&txn)
        .await?;
    }

    txn.commit().await?;

    Ok(result)
  }

  pub async fn view(&self, session: &UserSession, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let Some(post) = self.select(BoardPosts::find_by_id(id)).into_model::<GetBoardPost>().one(&txn).await?
    else {
      return Err(SumidaError::NoData("BoardPost".into(), id.to_string()));
    };

    if session.profile.is_authorized {
      let user_id = session.profile.id;
      let not_viewed = BoardPostViews::find()
        .filter(entity::bs::board_post_views::Column::PostId.eq(post.id))
        .filter(entity::bs::board_post_views::Column::UserId.eq(user_id))
        .one(&txn)
        .await?
        .is_none();

      if not_viewed {
        let view_am = entity::bs::board_post_views::ActiveModel {
          post_id: Set(id),
          user_id: Set(Some(user_id)),
          ..Default::default()
        };

        BoardPostViews::insert(view_am).exec(&txn).await?;
      }
    }

    txn.commit().await?;

    Ok(Some(post))
  }

  pub async fn create(
    &self,
    data: CreateBoardPost,
  ) -> Result<InsertResult<entity::bs::board_posts::ActiveModel>, SumidaError> {
    // Check write permission for the board
    let txn = self.state.db().begin().await?;

    let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".into(), data.board_id.to_string()));
    };
    ap::chain()
      .push_if(board.category_id, |x| ap::ns("board_category").action("create").object(x))
      .push(ap::ns("board").action("create").object(board.id))
      .who(&self.session)
      .action("write")
      .try_check_allowed(&txn)
      .await?;

    // Store attached file IDs for later processing
    let attached_file_ids = data.attached_file_ids.clone();

    let mut am = data.into_active_model();
    am.user_id = Set(Some(self.session.profile.id));

    let result = BoardPosts::insert(am).exec(&txn).await?;
    let post_id = result.last_insert_id;

    // Associate attached files with the created post
    self.update_files(post_id, attached_file_ids.unwrap_or_default(), &txn).await?;

    txn.commit().await?;

    Ok(result)
  }

  pub async fn create_draft(
    &self,
    data: CreateBoardDraft,
  ) -> Result<InsertResult<entity::bs::board_posts::ActiveModel>, SumidaError> {
    // Check write permission for the board
    let txn = self.state.db().begin().await?;

    let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".into(), data.board_id.to_string()));
    };
    ap::chain()
      .push_if(board.category_id, |x| ap::ns("board_category").action("create").object(x))
      .push(ap::ns("board").action("create").object(board.id))
      .who(&self.session)
      .action("write")
      .try_check_allowed(&txn)
      .await?;

    let attached_file_ids = data.attached_file_ids.clone();

    let mut am = data.into_active_model();
    am.user_id = Set(Some(self.session.profile.id));
    am.status = Set(BoardPostStatus::Draft);

    let result = BoardPosts::insert(am).exec(&txn).await?;
    let post_id = result.last_insert_id;

    self.update_files(post_id, attached_file_ids.unwrap_or_default(), &txn).await?;

    txn.commit().await?;

    Ok(result)
  }

  async fn update_files<'db, C: ConnectionTrait>(
    &self,
    post_id: i32,
    file_ids: Vec<i32>,
    db: &'db C,
  ) -> Result<(), SumidaError> {
    for file_id in file_ids {
      let file_am = entity::bs::board_files::ActiveModel {
        id: Set(file_id),
        post_id: Set(Some(post_id)),
        is_temporary: Set(false),
        expires_at: Set(None),
        ..Default::default()
      };

      entity::bs::board_files::Entity::update(file_am).exec(db).await?;
    }

    Ok(())
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPost,
  ) -> Result<entity::bs::board_posts::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    self.try_access("update", BoardPosts::find_by_id(id), &txn).await?;

    let attached_file_ids = data.attached_file_ids.clone();

    let mut am = data.into_active_model();
    am.id = Set(id);

    let item = BoardPosts::update(am).exec(&txn).await?;

    if let Some(file_ids) = attached_file_ids {
      for file_id in file_ids {
        let file_am = entity::bs::board_files::ActiveModel {
          id: Set(file_id),
          post_id: Set(Some(id)),
          is_temporary: Set(false),
          expires_at: Set(None),
          ..Default::default()
        };

        entity::bs::board_files::Entity::update(file_am).exec(&txn).await?;
      }
    }

    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", BoardPosts::find_by_id(id), &txn).await?;
    let result = BoardPosts::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApChain, BoardPosts> for BoardPostService<S> {
  fn default_query_with(query: Select<BoardPosts>) -> Select<BoardPosts> {
    let user_alias = Alias::new("user");
    let user_id = Expr::col((user_alias.clone(), "id"));
    let user_name = Expr::col((user_alias.clone(), "full_name"));
    let user_username = Expr::col((user_alias.clone(), "username"));

    let board_alias = Alias::new("board");
    let board_id = Expr::col((board_alias.clone(), "id"));
    let board_name = Expr::col((board_alias.clone(), "name"));
    let board_category_id = Expr::col((board_alias.clone(), "category_id"));

    query
      .select_only()
      .expr(Expr::col((BoardPosts, Asterisk)))
      .expr(Expr::count(entity::bs::board_post_views::Column::Id.into_expr()))
      .expr_as(user_id.clone(), "user_id")
      .expr_as(user_name, "user_full_name")
      .expr_as(user_username, "user_username")
      .expr_as(board_id.clone(), "board_id")
      .expr_as(board_name, "board_name")
      .expr_as(board_category_id, "board_category_id")
      .join_as(JoinType::LeftJoin, entity::bs::board_posts::Relation::User.def(), user_alias)
      .join_as(JoinType::LeftJoin, entity::bs::board_posts::Relation::Board.def(), board_alias.clone())
      .join(JoinType::LeftJoin, entity::bs::boards::Relation::Category.def().from_alias(board_alias))
      .join(JoinType::LeftJoin, entity::bs::board_posts::Relation::Views.def())
      .group_by(entity::bs::board_posts::Column::Id)
      .group_by(board_id)
      .group_by(user_id)
  }

  fn ap(&self) -> ApChain {
    use entity::bs::board_posts::Column;

    let board_category_id = Expr::col((Alias::new("board"), "category_id")).cast_as("VARCHAR");

    ap::chain()
      .push(ap::ns("board_category").expr(board_category_id))
      .push(ap::ns("board").join(BoardPosts, Column::BoardId))
      .who(&self.session)
  }
}

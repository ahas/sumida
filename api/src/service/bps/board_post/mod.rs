pub mod dto;
pub mod util;

use std::collections::HashMap;

use entity::bps::board_posts::{BoardPostKind, BoardPostStatus};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::sea_query::Expr;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  DeleteResult,
  EntityTrait,
  InsertResult,
  IntoActiveModel,
  IntoSimpleExpr,
  Order,
  PaginatorTrait,
  QueryFilter,
  QueryOrder,
  QuerySelect,
  RelationTrait,
  Select,
  TransactionTrait,
};
use sea_orm_crud::Nullable;

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::util::ap::{self, ApChain, ApGuard};
use crate::util::pagination::Pagination;
use crate::util::user::hash_password;

pub struct BoardPostService<S: SumidaState> {
  state:   S,
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
    let search_text = filter.search.clone().unwrap_or_else(|| filter.text.clone().unwrap_or_default());
    let with_snippet = filter.with_snippet;
    let query = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find()),
    );
    let query = if with_snippet {
      util::with_fts(query, search_text, &filter)
    } else {
      query.column_as(Expr::cust("NULL::tsvector"), "snippet")
    };

    let db = self.state.db();
    let mut query = filter.apply(query);
    if filter.with_replies.unwrap_or(false) {
      let kind = filter.kind.clone().unwrap_or(BoardPostKind::Article);

      match kind {
        BoardPostKind::Article => {
          query = query.filter(BoardPostsColumn::ParentId.is_null());
        }
        BoardPostKind::Comment => {
          query = query.column_as(BoardPostsColumn::Content, "content")
        }
        _ => {}
      }
    }

    let count = query.clone().count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "title" => BoardPostsColumn::Title.into_simple_expr(),
      _ => BoardPostsColumn::CreatedAt.into_simple_expr(),
    });
    query = pagination.paginate(query);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if filter.with_replies.unwrap_or(false) && !items.is_empty() {
      let reply_kind = match filter.kind {
        Some(BoardPostKind::Comment) => BoardPostKind::Comment,
        _ => BoardPostKind::Reply,
      };
      let mut replies_by_parent = self
        .load_replies_tree(&items.iter().map(|item| item.common.id).collect::<Vec<_>>(), reply_kind, db)
        .await?;

      self.attach_replies(&mut items, &mut replies_by_parent);
    }

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Then get the post
    let mut post = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find()).column_as(BoardPostsColumn::Content, "content"),
    )
    .filter(BoardPostsColumn::Id.eq(id))
    .into_model::<GetBoardPost>()
    .one(&txn)
    .await?;

    // Load attached files together
    post = util::load_attached_files(post, id, &txn).await?;

    if let Some(ref mut post) = post {
      let mut replies_by_parent =
        self.load_replies_tree(&[post.common.id], BoardPostKind::Reply, &txn).await?;
      let mut replies = replies_by_parent.remove(&post.common.id).unwrap_or_default();
      self.attach_replies(&mut replies, &mut replies_by_parent);
      post.replies = replies;
    }

    txn.commit().await?;

    Ok(post)
  }

  pub async fn view(
    &self,
    session: &UserSession,
    dto: ViewBoardPost,
  ) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let board_post_query = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find()).column_as(BoardPostsColumn::Content, "content"),
    );

    // Find post record
    let Some(post) = board_post_query.clone().one(&txn).await? else {
      return Err(SumidaError::NoData("BoardPost".to_string(), dto.id.to_string()));
    };

    // Verify post password if it exists
    if let Some(ref post_password) = post.password {
      if !dto.verify_password(post_password) {
        return Err(SumidaError::NoAccessPermission);
      }
    }

    // Find board to get global visibility configs
    let Some(board) = Boards::find_by_id(post.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".to_string(), post.board_id.to_string()));
    };
    let user_id = if session.profile.is_authorized { Some(session.profile.id) } else { None };

    // Check board record/content visibility limitations
    // Board configs override post configs when the permission level is lower
    if !util::check_post_visible(
      user_id,
      post.user_id,
      session.profile.is_admin,
      board.content_visibility,
      post.content_visibility,
    )? {
      return Err(SumidaError::NoAccessPermission);
    }

    // Increase `views` column value
    if session.profile.is_authorized {
      util::increase_view_count_if_not_viewed(post.id, post.views, session.profile.id, &txn).await?;
    }

    // Get a board post item with files
    let mut post = board_post_query.into_model::<GetBoardPost>().one(&txn).await?;
    post = util::load_attached_files(post, dto.id, &txn).await?;

    txn.commit().await?;

    Ok(post)
  }

  pub async fn create(
    &self,
    data: CreateBoardPost,
  ) -> Result<InsertResult<BoardPostsActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".into(), data.board_id.to_string()));
    };

    ap::chain()
      .push_if(board.group_id, |x| ap::ns("board_group").action("create").object(x))
      .push(ap::ns("board").action("create").object(board.id))
      .who(&self.session)
      .action("create")
      .try_check_allowed(&txn)
      .await?;

    // Store attached file IDs for later processing
    let attached_file_ids = data.attached_file_ids.clone();
    let plain_text = data.parse_plain_text()?;
    let password = data.password.clone();

    let mut am = data.into_active_model();
    am.user_id = Set(Some(self.session.profile.id));
    am.password = if let Nullable::Value(password) = password {
      Set(Some(hash_password(password).map_err(|err| SumidaError::Unknown(500, err.to_string()))?))
    } else {
      NotSet
    };

    let result = BoardPosts::insert(am).exec(&txn).await?;
    let post_id = result.last_insert_id;

    // Update `text_tsv`(tsvector) column
    BoardPosts::update_text(post_id, plain_text, &txn).await?;

    // Associate attached files with the created post
    util::link_files(board.id, post_id, attached_file_ids.unwrap_or_default(), &txn).await?;

    ap::edit()
      .namespace("board_post")
      .object(post_id)
      .who(&self.session)
      .effect("update")
      .effect("delete")
      .create(&txn)
      .await?;

    txn.commit().await?;

    Ok(result)
  }

  // pub async fn create_draft(
  //   &self,
  //   data: CreateBoardDraft,
  // ) -> Result<InsertResult<BoardPostsActiveModel>, SumidaError> {
  //   let txn = self.state.db().begin().await?;
  //
  //   let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
  //     return Err(SumidaError::NoData("Board".into(), data.board_id.to_string()));
  //   };
  //   ap::chain()
  //     .push_if(board.group_id, |x| ap::ns("board_group").action("update").object(x))
  //     .push(ap::ns("board").action("update").object(board.id))
  //     .who(&self.session)
  //     .action("update")
  //     .try_check_allowed(&txn)
  //     .await?;
  //
  //   let attached_file_ids = data.attached_file_ids.clone();
  //   let plain_text = data.parse_plain_text()?;
  //
  //   let mut am = data.into_active_model();
  //   am.user_id = Set(Some(self.session.profile.id));
  //   am.status = Set(BoardPostStatus::Draft);
  //
  //   let result = BoardPosts::insert(am).exec(&txn).await?;
  //   let post_id = result.last_insert_id;
  //
  //   // Update `text_tsv`(tsvector) column
  //   BoardPosts::update_text(post_id, plain_text, &txn).await?;
  //
  //   // Associate attached files with the created post
  //   util::link_files(post_id, attached_file_ids.unwrap_or_default(), &txn).await?;
  //
  //   txn.commit().await?;
  //
  //   Ok(result)
  // }

  pub async fn update(&self, id: i32, data: UpdateBoardPost) -> Result<BoardPostsModel, SumidaError> {
    let txn = self.state.db().begin().await?;

    self.try_access("update", BoardPosts::find_by_id(id), &txn).await?;

    let post = BoardPosts::find_by_id(id).one(&txn).await?.unwrap();
    let board = Boards::find_by_id(post.board_id).one(&txn).await?.unwrap();

    if board.is_versioned {
      let mut am = post.clone().into_active_model();
      am.id = NotSet;
      am.parent_id = Set(Some(id));
      am.created_at = NotSet;
      am.updated_at = NotSet;
      am.scheduled_at = NotSet;
      am.deleted_at = NotSet;
      am.status = Set(BoardPostStatus::Archived);
      BoardPosts::insert(am).exec(&txn).await?;
    }

    let attached_file_ids = data.attached_file_ids.clone();
    let detached_file_ids = data.detached_file_ids.clone();
    let plain_text = data.parse_plain_text(post.content_type)?;
    let password = data.password.clone();

    let mut am = data.into_active_model();
    am.id = Set(id);
    am.password = if let Nullable::Value(password) = password {
      Set(Some(hash_password(password).map_err(|err| SumidaError::Unknown(500, err.to_string()))?))
    } else {
      NotSet
    };

    let item = BoardPosts::update(am).exec(&txn).await?;
    BoardPosts::update_text(id, plain_text, &txn).await?;

    if let Some(file_ids) = attached_file_ids {
      util::link_files(board.id, id, file_ids, &txn).await?;
    }

    if let Some(file_ids) = detached_file_ids {
      util::unlink_files(id, file_ids, &txn).await?;
    }

    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", BoardPosts::find_by_id(id), &txn).await?;
    let result = BoardPosts::delete_by_id(id).exec(&txn).await?;

    ap::edit().namespace("board_post").object(id).delete(&txn).await?;

    txn.commit().await?;

    Ok(result)
  }

  async fn load_replies_tree<C: ConnectionTrait>(
    &self,
    parent_ids: &[i32],
    reply_kind: BoardPostKind,
    db: &C,
  ) -> Result<HashMap<i32, Vec<BoardPostListItem>>, SumidaError> {
    let mut replies_by_parent: HashMap<i32, Vec<BoardPostListItem>> = HashMap::new();
    let mut current_parent_ids = parent_ids.to_vec();

    while !current_parent_ids.is_empty() {
      let replies = util::with_conditional_post_columns(
        self.session.profile.get_id(),
        self.session.profile.is_admin,
        self
          .select(BoardPosts::find())
          .filter(BoardPostsColumn::Kind.eq(reply_kind.clone()))
          .filter(BoardPostsColumn::ParentId.is_in(current_parent_ids.clone()))
          .order_by(BoardPostsColumn::CreatedAt, Order::Asc)
          .column_as(Expr::cust("NULL::tsvector"), "snippet"),
      )
      .into_model::<BoardPostListItem>()
      .all(db)
      .await?;

      if replies.is_empty() {
        break;
      }

      current_parent_ids.clear();

      for reply in replies {
        if let Some(parent_id) = reply.common.parent_id {
          current_parent_ids.push(reply.common.id);
          replies_by_parent.entry(parent_id).or_default().push(reply);
        }
      }
    }

    Ok(replies_by_parent)
  }

  fn attach_replies(
    &self,
    items: &mut [BoardPostListItem],
    replies_by_parent: &mut HashMap<i32, Vec<BoardPostListItem>>,
  ) {
    for item in items {
      if let Some(mut replies) = replies_by_parent.remove(&item.common.id) {
        self.attach_replies(&mut replies, replies_by_parent);
        item.replies = Some(replies);
      }
    }
  }
}

impl<S: SumidaState> ApiService<ApChain, BoardPosts> for BoardPostService<S> {
  fn default_query_with(query: Select<BoardPosts>) -> Select<BoardPosts> {
    use sea_orm::JoinType::LeftJoin;

    query
      .select_only()
      // BoardPostCommon
      .column_as(BoardPostsColumn::Id, "id")
      .column_as(BoardPostsColumn::ParentId, "parent_id")
      .column_as(BoardPostsColumn::Title, "title")
      .column_as(BoardPostsColumn::Kind, "kind")
      .column_as(BoardPostsColumn::Status, "status")
      .column_as(BoardPostsColumn::ContentType, "content_type")
      .column_as(BoardPostsColumn::CompressionType, "compression_type")
      .column_as(BoardPostsColumn::Revision, "reivision")
      .column_as(BoardPostsColumn::Tags, "tags")
      .column_as(BoardPostsColumn::PublishedAt, "published_at")
      .column_as(BoardPostsColumn::ScheduledAt, "scheduled_at")
      .column_as(BoardPostsColumn::CreatedAt, "created_at")
      .column_as(BoardPostsColumn::UpdatedAt, "updated_at")
      // BoardPostUser
      .column_as(UsersColumn::Id, "user_id")
      .column_as(UsersColumn::Username, "user_username")
      .column_as(UsersColumn::FullName, "user_full_name")
      // BoardPostBoard
      .column_as(BoardsColumn::Id, "board_id")
      .column_as(BoardsColumn::Name, "board_name")
      .column_as(BoardsColumn::GroupId, "board_group_id")
      // BoardPostTopic
      .column_as(BoardTopicsColumn::Id, "topic_id")
      .column_as(BoardTopicsColumn::Name, "topic_name")
      // Joins
      .join(LeftJoin, BoardPostsRelation::User.def())
      .join(LeftJoin, BoardPostsRelation::Board.def())
      .join(LeftJoin, BoardPostsRelation::Topic.def())
      .join(LeftJoin, BoardsRelation::Group.def())
  }

  fn ap(&self) -> ApChain {
    ap::chain()
      .push(ap::ns("board_group").column(BoardsColumn::GroupId))
      .push(ap::ns("board").column(BoardPostsColumn::Id))
      .who(&self.session)
  }
}

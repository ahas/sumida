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
  FromQueryResult,
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
use crate::util::pagination::{List, Pagination};
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

  fn apply_pagination(&self, query: Select<BoardPosts>, pagination: Pagination) -> Select<BoardPosts> {
    let query = pagination.order_by(query, |col| match col {
      "title" => BoardPostsColumn::Title.into_simple_expr(),
      _ => BoardPostsColumn::CreatedAt.into_simple_expr(),
    });

    pagination.paginate(query)
  }

  pub async fn list(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter)?;
    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let items = query.into_model::<BoardPostListItem>().all(db).await?;

    Ok(List::new(items, count))
  }

  pub fn get_list_query(&self, filter: BoardPostFilter) -> Result<Select<BoardPosts>, SumidaError> {
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

    Ok(filter.apply(query))
  }

  pub async fn get_notices(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Notice));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let items = query.into_model::<BoardPostListItem>().all(db).await?;

    Ok(List::new(items, count))
  }

  pub async fn get_articles(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Notice));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(self.select(BoardPosts::find()), &item_ids, BoardPostKind::Article, db)
        .await?;

      self.attach_list_item_replies(&mut items, &mut replies_by_parent);
    }

    Ok(List::new(items, count))
  }

  pub async fn get_comments(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Comment));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(
          self.select(BoardPosts::find()).column_as(BoardPostsColumn::Content, "content"),
          &item_ids,
          BoardPostKind::Comment,
          db,
        )
        .await?;

      self.attach_list_item_replies(&mut items, &mut replies_by_parent);
    }

    Ok(List::new(items, count))
  }

  pub async fn get_replies(
    &self,
    filter: BoardPostFilter,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Reply));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(self.select(BoardPosts::find()), &item_ids, BoardPostKind::Reply, db)
        .await?;

      self.attach_list_item_replies(&mut items, &mut replies_by_parent);
    }

    Ok(List::new(items, count))
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

    txn.commit().await?;

    Ok(post)
  }

  pub async fn view(&self, id: i32, dto: ViewBoardPost) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let board_post_query = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find_by_id(id)).column_as(BoardPostsColumn::Content, "content"),
    );

    // Find post record
    let Some(post) = board_post_query.clone().one(&txn).await? else {
      return Err(SumidaError::NoData("BoardPost".to_string(), id.to_string()));
    };

    // Verify post password if it exists
    if !self.session.profile.is_admin {
      if let Some(ref post_password) = post.password {
        if !dto.verify_password(post_password) {
          return Err(SumidaError::NoAccessPermission);
        }
      }
    }

    // Find board to get global visibility configs
    let Some(board) = Boards::find_by_id(post.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".to_string(), post.board_id.to_string()));
    };
    let user_id = if self.session.profile.is_authorized { Some(self.session.profile.id) } else { None };

    // Check board record/content visibility limitations
    // Board configs override post configs when the permission level is lower
    if !util::check_post_visible(
      user_id,
      post.user_id,
      self.session.profile.is_admin,
      board.content_visibility,
      post.content_visibility,
    )? {
      return Err(SumidaError::NoAccessPermission);
    }

    // Increase `views` column value
    if self.session.profile.is_authorized {
      util::increase_view_count_if_not_viewed(post.id, post.views, self.session.profile.id, &txn).await?;
    }

    // Get a board post item with files
    let mut post = board_post_query.into_model::<GetBoardPost>().one(&txn).await?;
    post = util::load_attached_files(post, id, &txn).await?;

    if let Some(post) = post {
      let post_ids = [post.common.id];
      let query = self.select(BoardPosts::find()).column_as(BoardPostsColumn::Content, "content");
      let mut comments =
        self.load_replies_tree(query.clone(), &post_ids, BoardPostKind::Comment, &txn).await?;
      let mut replies = self.load_replies_tree(query, &post_ids, BoardPostKind::Reply, &txn).await?;

      let mut posts = [post];
      self.attach_view_children(&mut posts, &mut comments, &mut replies);
      txn.commit().await?;

      return Ok(posts.into_iter().next());
    }

    Err(SumidaError::NoData("Post".to_string(), id.to_string()))
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
    let attached_file_uuids = data.attached_file_uuids.clone();
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
    util::link_files(board.id, post_id, attached_file_uuids.unwrap_or_default(), &txn).await?;

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

    let attached_file_uuids = data.attached_file_uuids.clone();
    let detached_file_uuids = data.detached_file_uuids.clone();
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

    if let Some(file_uuids) = attached_file_uuids {
      util::link_files(board.id, id, file_uuids, &txn).await?;
    }

    if let Some(file_uuids) = detached_file_uuids {
      util::unlink_files(id, file_uuids, &txn).await?;
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

  async fn load_replies_tree<T: Reply + FromQueryResult, C: ConnectionTrait>(
    &self,
    query: Select<BoardPosts>,
    parent_ids: &[i32],
    reply_kind: BoardPostKind,
    db: &C,
  ) -> Result<HashMap<i32, Vec<T>>, SumidaError> {
    let mut replies_by_parent: HashMap<i32, Vec<T>> = HashMap::new();
    let mut current_parent_ids = parent_ids.to_vec();

    while !current_parent_ids.is_empty() {
      let replies = util::with_conditional_post_columns(
        self.session.profile.get_id(),
        self.session.profile.is_admin,
        query
          .clone()
          .filter(BoardPostsColumn::Kind.eq(reply_kind.clone()))
          .filter(BoardPostsColumn::ParentId.is_in(current_parent_ids.clone()))
          .order_by(BoardPostsColumn::CreatedAt, Order::Asc)
          .column_as(Expr::cust("NULL::tsvector"), "snippet"),
      )
      .into_model::<T>()
      .all(db)
      .await?;

      if replies.is_empty() {
        break;
      }

      current_parent_ids.clear();

      for reply in replies {
        if let Some(parent_id) = reply.get_parent_id() {
          current_parent_ids.push(reply.get_id());
          replies_by_parent.entry(parent_id).or_default().push(reply);
        }
      }
    }

    Ok(replies_by_parent)
  }

  fn attach_list_item_replies(
    &self,
    items: &mut [BoardPostListItem],
    replies_by_parent: &mut HashMap<i32, Vec<BoardPostListItem>>,
  ) {
    for item in items {
      if let Some(mut replies) = replies_by_parent.remove(&item.common.id) {
        self.attach_list_item_replies(&mut replies, replies_by_parent);
        item.replies = Some(replies);
      }
    }
  }

  fn attach_view_children(
    &self,
    views: &mut [GetBoardPost],
    comments_by_parent: &mut HashMap<i32, Vec<GetBoardPost>>,
    replies_by_parent: &mut HashMap<i32, Vec<GetBoardPost>>,
  ) {
    for view in views {
      if let Some(mut comments) = comments_by_parent.remove(&view.common.id) {
        self.attach_view_children(&mut comments, comments_by_parent, replies_by_parent);
        view.comments = Some(comments);
      }

      if let Some(mut replies) = replies_by_parent.remove(&view.common.id) {
        self.attach_view_children(&mut replies, comments_by_parent, replies_by_parent);
        view.replies = Some(replies);
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
      .column_as(BoardPostsColumn::ContentVisibility, "content_visibility")
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

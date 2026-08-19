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
  ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, InsertResult, IntoActiveModel,
  Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait, Select,
  TransactionTrait,
};
use sea_orm_crud::Nullable;

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::service::auth::helper::verify_password;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::{List, Pagination};
use crate::util::user::hash_password;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace board_post service = board_post`).
const RESOURCE: &str = "board_post";

pub struct BoardPostService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

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
      // Soft-deleted posts — and posts under a soft-deleted board — are never returned.
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .filter(BoardsColumn::DeletedAt.is_null())
  }

  /// 행 스코프는 `board_post#view` 술어 하나 — 작성자 본인(`author`,
  /// `board_posts.user_id` 에서 파생), 게시판의 열람 권한(`board->view`, 그 안에 그룹과의
  /// 교집합이 들어 있다), 그리고 차단(`blocked`)의 빼기가 전부 그 안에 있다.
  ///
  /// 종전에는 `ApChain` 이 `board_group AND board` 를 호출자 쪽에서 조립했다. 교집합은 규칙에
  /// 그대로 남았지만(`board#edit` 의 `group->edit & editor_or_own_here`), 이제 조립하는 호출자가
  /// 없으므로 잊을 수도 없다.
  fn select(&self, query: Select<BoardPosts>) -> Result<Select<BoardPosts>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), Self::default_query_with(query))?)
  }

  /// 정렬 키는 `BoardPostFilter` 가 선언한 필드 목록에서 풀린다 — 손으로 적은 match 가 아니라
  /// `/board-posts/filters` 가 내보내는 그 목록이라, 클라이언트에 알려 준 것과 서버가 실제로
  /// 정렬하는 것이 어긋날 수 없다. 선언에 없는 키는 기본 정렬로 떨어진다.
  fn apply_pagination(&self, query: Select<BoardPosts>, pagination: Pagination) -> Select<BoardPosts> {
    pagination.paginate(pagination.order_by_schema::<BoardPostFilter, _>(query))
  }

  pub async fn list(
    &self,
    filter: BoardPostFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter, &conditions)?;
    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let items = query.into_model::<BoardPostListItem>().all(db).await?;

    Ok(List::new(items, count))
  }

  /// The list query: the read scope first, the flat parameters inside it, and the condition tree
  /// added last as one more conjunct. Every list path shares it, so what narrows one narrows all
  /// five and the count is taken from the same query.
  pub fn get_list_query(
    &self,
    filter: BoardPostFilter,
    conditions: &ConditionQuery,
  ) -> Result<Select<BoardPosts>, SumidaError> {
    let search_text = filter.search.clone().unwrap_or_else(|| filter.text.clone().unwrap_or_default());
    let with_snippet = filter.with_snippet;
    let query = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find())?,
    );
    let query = if with_snippet {
      util::with_fts(query, search_text, &filter)
    } else {
      query.column_as(Expr::cust("NULL::tsvector"), "snippet")
    };

    let query = filter.apply(query);

    // Hide posts whose content the requester can't see (title + snippet) — admins exempt.
    let query = if self.session.profile.is_admin {
      query
    } else {
      query.filter(util::content_visible_filter(self.session.profile.get_id()))
    };

    // Last, and only ever as one more `AND`. An `OR` among the tree's own nodes is a conjunct of
    // this WHERE, never an alternative to the scope above it.
    conditions.apply::<BoardPostFilter, _>(query)
  }

  pub async fn get_notices(
    &self,
    filter: BoardPostFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter, &conditions)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Notice));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let items = query.into_model::<BoardPostListItem>().all(db).await?;

    Ok(List::new(items, count))
  }

  pub async fn get_articles(
    &self,
    filter: BoardPostFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter, &conditions)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Article));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(self.select(BoardPosts::find())?, &item_ids, BoardPostKind::Article, db)
        .await?;

      self.attach_list_item_replies(&mut items, &mut replies_by_parent);
    }

    Ok(List::new(items, count))
  }

  pub async fn get_comments(
    &self,
    filter: BoardPostFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter, &conditions)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Comment));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(
          self.select(BoardPosts::find())?.column_as(BoardPostsColumn::Content, "content"),
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
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<List<BoardPostListItem>, SumidaError> {
    let db = self.state.db();
    let mut query = self.get_list_query(filter, &conditions)?;
    query = query.filter(BoardPostsColumn::Kind.eq(BoardPostKind::Reply));

    let count = query.clone().count(db).await?;

    query = self.apply_pagination(query, pagination);

    let mut items = query.into_model::<BoardPostListItem>().all(db).await?;

    if !items.is_empty() {
      let item_ids = items.iter().map(|x| x.common.id).collect::<Vec<_>>();
      let mut replies_by_parent = self
        .load_replies_tree(self.select(BoardPosts::find())?, &item_ids, BoardPostKind::Reply, db)
        .await?;

      self.attach_list_item_replies(&mut items, &mut replies_by_parent);
    }

    Ok(List::new(items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Per-board read permission + existence: the AP-filtered query yields None when access is denied.
    let post = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find())?.column_as(BoardPostsColumn::Content, "content"),
    )
    .filter(BoardPostsColumn::Id.eq(id))
    .into_model::<GetBoardPost>()
    .one(&txn)
    .await?;

    let Some(post) = post else {
      return Ok(None);
    };

    // Password + content-visibility gate, mirroring `view`. `get` carries no password input, so a
    // password-protected post is only reachable here by its author or an admin.
    let Some(model) = BoardPosts::find_by_id(id)
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .one(&txn)
      .await?
    else {
      return Ok(None);
    };
    self.guard_post_read(&model, None, &txn).await?;

    // Load attached files together
    let post = util::load_attached_files(Some(post), id, &txn).await?;

    txn.commit().await?;

    Ok(post)
  }

  /// Password + content-visibility gate shared by `get` and `view`.
  /// A password-protected post requires the matching password unless the requester is the author or an admin;
  /// content visibility (board config overriding the post when stricter) is then enforced via [`util::check_post_visible`].
  pub(crate) async fn guard_post_read<C: ConnectionTrait>(
    &self,
    post: &BoardPostsModel,
    password: Option<&str>,
    txn: &C,
  ) -> Result<(), SumidaError> {
    if !self.session.profile.is_admin && post.password.is_some() {
      let is_author = self.session.profile.is_authorized && post.user_id == Some(self.session.profile.id);
      if !is_author {
        let post_password = post.password.as_deref().unwrap_or_default();
        let ok = password.map(|p| verify_password(p, post_password)).unwrap_or(false);
        if !ok {
          return Err(SumidaError::NoAccessPermission);
        }
      }
    }

    let board = Boards::find_by_id(post.board_id)
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".to_string(), post.board_id.to_string()))?;
    let user_id = if self.session.profile.is_authorized { Some(self.session.profile.id) } else { None };

    if !util::check_post_visible(
      user_id,
      post.user_id,
      self.session.profile.is_admin,
      board.content_visibility,
      post.content_visibility.clone(),
    )? {
      return Err(SumidaError::NoAccessPermission);
    }

    Ok(())
  }

  pub async fn view(&self, id: i32, dto: ViewBoardPost) -> Result<Option<GetBoardPost>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let board_post_query = util::with_conditional_post_columns(
      self.session.profile.get_id(),
      self.session.profile.is_admin,
      self.select(BoardPosts::find_by_id(id))?.column_as(BoardPostsColumn::Content, "content"),
    );

    // Existence + per-board read permission (the AP-filtered query yields None when access is denied).
    if board_post_query.clone().into_model::<GetBoardPost>().one(&txn).await?.is_none() {
      return Err(SumidaError::NoData("BoardPost".to_string(), id.to_string()));
    }

    // Raw model carries the fields the password + visibility gate needs.
    let Some(post) = BoardPosts::find_by_id(id)
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .one(&txn)
      .await?
    else {
      return Err(SumidaError::NoData("BoardPost".to_string(), id.to_string()));
    };

    // Password + content-visibility gate (a missing/wrong password is rejected for protected posts).
    self.guard_post_read(&post, dto.password.as_deref(), &txn).await?;

    // Increase `views` column value
    if self.session.profile.is_authorized {
      util::increase_view_count_if_not_viewed(post.id, post.views, self.session.profile.id, &txn).await?;
    }

    // Get a board post item with files
    let mut post = board_post_query.into_model::<GetBoardPost>().one(&txn).await?;
    post = util::load_attached_files(post, id, &txn).await?;

    if let Some(post) = post {
      let post_ids = [post.common.id];
      let query = self.select(BoardPosts::find())?.column_as(BoardPostsColumn::Content, "content");
      let mut comments =
        self.load_replies_tree(query.clone(), &post_ids, BoardPostKind::Comment, &txn).await?;
      let mut replies = self.load_replies_tree(query, &post_ids, BoardPostKind::Reply, &txn).await?;

      let mut posts = [post];
      self.attach_view_children(&mut posts, &mut comments, &mut replies);

      let Some(mut post) = posts.into_iter().next() else {
        return Ok(None);
      };

      if post.comments.is_none() {
        post.comments = Some(Vec::new());
      }

      if post.replies.is_none() {
        post.replies = Some(Vec::new());
      }

      txn.commit().await?;

      return Ok(Some(post));
    }

    Err(SumidaError::NoData("Post".to_string(), id.to_string()))
  }

  /// List the users who viewed a post, gated by the board's `viewer_visibility`.
  pub async fn list_viewers(
    &self,
    post_id: i32,
    pagination: Pagination,
  ) -> Result<List<BoardPostViewerItem>, SumidaError> {
    use sea_orm::JoinType::LeftJoin;

    let txn = self.state.db().begin().await?;

    let post = BoardPosts::find_by_id(post_id)
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".to_string(), post_id.to_string()))?;
    let board = Boards::find_by_id(post.board_id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".to_string(), post.board_id.to_string()))?;

    self.check_viewer_visibility(&post, &board.viewer_visibility)?;

    let base = BoardPostViews::find()
      .filter(BoardPostViewsColumn::PostId.eq(post_id))
      .join(LeftJoin, BoardPostViewsRelation::User.def());
    let count = base.clone().count(&txn).await?;

    let items = base
      .select_only()
      .column_as(BoardPostViewsColumn::UserId, "user_id")
      .column_as(UsersColumn::Username, "username")
      .column_as(UsersColumn::FullName, "name")
      .column_as(BoardPostViewsColumn::CreatedAt, "viewed_at")
      .order_by(BoardPostViewsColumn::CreatedAt, Order::Desc);
    let items = pagination.paginate(items).into_model::<BoardPostViewerItem>().all(&txn).await?;

    txn.commit().await?;

    Ok(List::new(items, count))
  }

  /// Who may see the viewer list, per the board's `viewer_visibility`.
  fn check_viewer_visibility(
    &self,
    post: &BoardPostsModel,
    visibility: &entity::bps::boards::BoardPostRecordVisibility,
  ) -> Result<(), SumidaError> {
    use entity::bps::boards::BoardPostRecordVisibility::*;

    if self.session.profile.is_admin {
      return Ok(());
    }
    let ok = match visibility {
      Public => true,
      MembersOnly => self.session.profile.is_authorized,
      AuthorOnly => self.session.profile.is_authorized && post.user_id == Some(self.session.profile.id),
      Private => false,
    };
    if ok { Ok(()) } else { Err(SumidaError::NoAccessPermission) }
  }

  pub async fn create(
    &self,
    data: CreateBoardPost,
  ) -> Result<InsertResult<BoardPostsActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(board) = Boards::find_by_id(data.board_id).one(&txn).await? else {
      return Err(SumidaError::NoData("Board".into(), data.board_id.to_string()));
    };

    // `board#create = edit`, 그리고 `board#edit` 안에 `group->edit & editor_or_own_here` 라는
    // 그룹과의 교집합이 이미 들어 있다. 종전의 `ap::chain()` 이 하던 조립이 규칙으로 옮겨간 것이며,
    // 그룹이 없는 게시판(`boards.group_id IS NULL`)에서 그 갈래가 죽는 것도 그대로다.
    Authz::try_check("board", board.id, "create", &self.subject(), &txn).await?;

    // Store attached file IDs for later processing
    let attached_file_uuids = data.attached_file_uuids.clone();
    let content_type = data.content_type.clone();
    let content_for_files: Option<String> = data.content.clone().into();
    util::validate_lengths(Some(&data.title), content_for_files.as_deref())?;
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

    // Associate attached files (explicit + inline references in the body) with the created post.
    util::claim_post_files(
      board.id,
      post_id,
      attached_file_uuids.unwrap_or_default(),
      content_for_files.as_deref(),
      &content_type,
      &txn,
    )
    .await?;

    // 작성자 소유권을 정책 행으로 따로 심지 않는다. `board_post#author` 는 바로 위에서 `Set` 한
    // `board_posts.user_id` 에서 파생되므로, 저장된 행과 어긋날 수 있는 두 번째 사본이 아예 없다.
    txn.commit().await?;

    Ok(result)
  }

  /// Permission check for editing/deleting a post.
  ///
  /// 종전에는 세 번의 `effects()` 결과를 호출자가 직접 조립했다:
  ///
  /// ```text
  /// allowed = (group_allow && board_allow) || owner_allow
  /// denied  = group_deny  || board_deny   || owner_deny
  /// result  = allowed && !denied
  /// ```
  ///
  /// 그 산술이 통째로 `board_post#edit` 안으로 들어갔다 — `board->edit` 이 그룹과의 교집합을
  /// 이미 품고 있고, 차단은 합집합 **바깥**에서 빠지므로 작성자 소유권으로 우회할 수 없다.
  /// 조립하는 호출자가 없으니 다음 호출자가 빼먹을 수도 없다.
  async fn authorize_post_mutation<C: sea_orm::ConnectionTrait>(
    &self,
    action: &str,
    post_id: i32,
    txn: &C,
  ) -> Result<(), SumidaError> {
    let permission = if action == "delete" { "remove" } else { "edit" };
    Authz::try_check(RESOURCE, post_id, permission, &self.subject(), txn).await
  }

  pub async fn update(&self, id: i32, data: UpdateBoardPost) -> Result<BoardPostsModel, SumidaError> {
    let txn = self.state.db().begin().await?;

    let post = BoardPosts::find_by_id(id)
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Post".to_string(), id.to_string()))?;
    let board = Boards::find_by_id(post.board_id)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".to_string(), post.board_id.to_string()))?;

    // Operator permission (board_group AND board) OR author ownership (board_post)
    self.authorize_post_mutation("update", id, &txn).await?;

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

    let content_type = post.content_type.clone();
    // Only reconcile inline references when content is actually present in this PATCH:
    // a value (or explicit null = cleared) is parsed; an undefined field leaves attachments alone.
    let parse_content: Option<String> = if data.content.has_value() {
      data.content.clone().into()
    } else if data.content.is_null() {
      Some(String::new())
    } else {
      None
    };
    let attached_file_uuids = data.attached_file_uuids.clone();
    let detached_file_uuids = data.detached_file_uuids.clone();
    util::validate_lengths(data.title.0.as_deref(), parse_content.as_deref())?;
    let plain_text = data.parse_plain_text(content_type.clone())?;
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

    util::claim_post_files(
      board.id,
      id,
      attached_file_uuids.unwrap_or_default(),
      parse_content.as_deref(),
      &content_type,
      &txn,
    )
    .await?;

    if let Some(file_uuids) = detached_file_uuids {
      util::unclaim_post_files(id, file_uuids, &txn).await?;
    }

    txn.commit().await?;

    Ok(item)
  }

  pub async fn delete(&self, id: i32) -> Result<(), SumidaError> {
    let txn = self.state.db().begin().await?;

    // 존재 확인은 그대로 하고, 게시판 행은 더 이상 읽지 않는다. 그룹/게시판 갈래는 규칙이
    // `board_post#remove` 안에서 스스로 따라간다.
    BoardPosts::find_by_id(id)
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Post".to_string(), id.to_string()))?;

    // Operator permission (board_group AND board) OR author ownership (board_post)
    self.authorize_post_mutation("delete", id, &txn).await?;

    // Soft delete cascades to the comment/reply subtree (the FK cascade no longer fires on a soft delete).
    let ids = util::collect_descendant_post_ids(id, &txn).await?;

    // Free each post's attached files, then mark the whole subtree deleted.
    for post_id in &ids {
      util::release_all_post_files(*post_id, &txn).await?;
    }

    BoardPosts::update_many()
      .set(BoardPostsActiveModel {
        status: Set(BoardPostStatus::Deleted),
        deleted_at: Set(Some(chrono::Utc::now().fixed_offset())),
        ..Default::default()
      })
      .filter(BoardPostsColumn::Id.is_in(ids.clone()))
      .filter(BoardPostsColumn::DeletedAt.is_null())
      .exec(&txn)
      .await?;

    txn.commit().await?;

    Ok(())
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


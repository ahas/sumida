pub mod dto;

use entity::bps::boards::BoardPostRecordVisibility;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QueryFilter,
  QuerySelect, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::prelude::BoardPostService;
use crate::util::filter::{ConditionQuery, listed};
use crate::util::pagination::Pagination;

type ReactionEntity = entity::bps::board_post_reactions::Entity;
use entity::bps::board_post_reactions::Column as ReactionColumn;

pub struct BoardPostReactionService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostReactionService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Resolve the board that owns `post_id`, enforcing the caller's read access to the post first.
  /// Returns the post's board so the caller can apply `reaction_visibility`.
  async fn post_board<C: sea_orm::ConnectionTrait>(
    &self,
    post_id: i32,
    txn: &C,
  ) -> Result<(entity::bps::board_posts::Model, entity::bps::boards::Model), SumidaError> {
    let post = BoardPosts::find_by_id(post_id)
      .filter(entity::bps::board_posts::Column::DeletedAt.is_null())
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), post_id.to_string()))?;

    // Reacting to / reading reactions on a post requires being able to read the post itself.
    BoardPostService::new(self.state.clone(), self.session.clone())
      .guard_post_read(&post, None, txn)
      .await?;

    let board = Boards::find_by_id(post.board_id)
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("Board".into(), post.board_id.to_string()))?;

    Ok((post, board))
  }

  /// Whether the caller may *see* reactions on `post` given the board's `reaction_visibility`.
  /// For `Private`, visibility is limited to the caller's own reactions (enforced by the query filter).
  fn can_view_reactions(
    &self,
    post: &entity::bps::board_posts::Model,
    visibility: &BoardPostRecordVisibility,
  ) -> Result<(), SumidaError> {
    if self.session.profile.is_admin {
      return Ok(());
    }
    match visibility {
      BoardPostRecordVisibility::Public => Ok(()),
      BoardPostRecordVisibility::MembersOnly => {
        if self.session.profile.is_authorized {
          Ok(())
        } else {
          Err(SumidaError::NoAccessPermission)
        }
      }
      BoardPostRecordVisibility::AuthorOnly => {
        if self.session.profile.is_authorized && post.user_id == Some(self.session.profile.id) {
          Ok(())
        } else {
          Err(SumidaError::NoAccessPermission)
        }
      }
      // `Private` is allowed but the list/get query restricts rows to the caller's own reactions.
      BoardPostRecordVisibility::Private => Ok(()),
    }
  }

  /// The read scope first — the post the caller named, plus the board's reaction visibility — and
  /// the declared fields inside it. `post_id` is answered above the query and can refuse; the
  /// declared keys can only narrow what that refusal already let through.
  pub async fn list(
    &self,
    filter: BoardPostReactionFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostReactionListItem>, u64), SumidaError> {
    let txn = self.state.db().begin().await?;

    // Reactions are always scoped to a single post so visibility can be enforced.
    let post_id = filter.post_id.ok_or_else(|| SumidaError::BadRequest("post_id is required".into()))?;
    let (post, board) = self.post_board(post_id, &txn).await?;
    self.can_view_reactions(&post, &board.reaction_visibility)?;

    let private_to_self = !self.session.profile.is_admin
      && matches!(board.reaction_visibility, BoardPostRecordVisibility::Private);

    let mut query = ReactionEntity::find().filter(ReactionColumn::PostId.eq(post_id));
    if private_to_self {
      // The caller's own rows, pinned before any declared key is read: `user_id` narrows inside
      // this, never in place of it.
      query = query.filter(ReactionColumn::UserId.eq(self.session.profile.id));
    }
    if let Some(value) = filter.value.as_ref() {
      query = query.filter(ReactionColumn::Value.eq(value));
    }
    if let Some(user_id) = listed(filter.user_id.as_ref()) {
      query = query.filter(user_id.into_simple_expr(ReactionColumn::UserId));
    }

    // Last, and only ever as one more `AND`.
    let mut query = conditions.apply::<BoardPostReactionFilter, _>(query)?;

    let count = query.clone().select_only().column(ReactionColumn::Id).count(&txn).await?;

    query = pagination.order_by_schema::<BoardPostReactionFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<BoardPostReactionListItem>().all(&txn).await?;
    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPostReaction>, SumidaError> {
    let txn = self.state.db().begin().await?;

    let Some(reaction) = ReactionEntity::find_by_id(id).one(&txn).await? else {
      return Ok(None);
    };

    let (post, board) = self.post_board(reaction.post_id, &txn).await?;
    self.can_view_reactions(&post, &board.reaction_visibility)?;

    // For `Private` boards a non-admin may only see their own reaction.
    if !self.session.profile.is_admin
      && matches!(board.reaction_visibility, BoardPostRecordVisibility::Private)
      && reaction.user_id != Some(self.session.profile.id)
    {
      return Err(SumidaError::NoAccessPermission);
    }

    let result = GetBoardPostReaction {
      id: reaction.id,
      post_id: reaction.post_id,
      user_id: reaction.user_id,
      value: reaction.value,
      created_at: reaction.created_at,
    };
    txn.commit().await?;

    Ok(Some(result))
  }

  pub async fn create(
    &self,
    data: CreateBoardPostReaction,
  ) -> Result<InsertResult<entity::bps::board_post_reactions::ActiveModel>, SumidaError> {
    // Reacting requires authentication; the reacting user is always the session user (never client-supplied).
    if !self.session.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }
    let user_id = self.session.profile.id;

    let txn = self.state.db().begin().await?;

    // Confirms the post exists and the caller may read it.
    self.post_board(data.post_id, &txn).await?;

    // Idempotent: one (post, user, value) reaction at most.
    if let Some(existing) = ReactionEntity::find()
      .filter(ReactionColumn::PostId.eq(data.post_id))
      .filter(ReactionColumn::UserId.eq(user_id))
      .filter(ReactionColumn::Value.eq(data.value.clone()))
      .one(&txn)
      .await?
    {
      txn.commit().await?;
      return Ok(InsertResult {
        last_insert_id: existing.id,
      });
    }

    let am = entity::bps::board_post_reactions::ActiveModel {
      id: NotSet,
      post_id: Set(data.post_id),
      user_id: Set(Some(user_id)),
      value: Set(data.value),
      ..Default::default()
    };

    let result = ReactionEntity::insert(am).exec(&txn).await?;
    refresh_reactions_aggregate(data.post_id, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPostReaction,
  ) -> Result<entity::bps::board_post_reactions::Model, SumidaError> {
    let txn = self.state.db().begin().await?;

    let reaction = self.owned_reaction(id, &txn).await?;
    let post_id = reaction.post_id;

    // Only the reaction's own `value` is mutable; post_id/user_id are immutable.
    let mut am = reaction.into_active_model();
    if let Some(value) = data.value.0 {
      am.value = Set(value);
    }

    let result = ReactionEntity::update(am).exec(&txn).await?;
    refresh_reactions_aggregate(post_id, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;

    let reaction = self.owned_reaction(id, &txn).await?;

    let result = ReactionEntity::delete_by_id(id).exec(&txn).await?;
    refresh_reactions_aggregate(reaction.post_id, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  /// Load a reaction the caller is allowed to mutate (its author, or an admin).
  async fn owned_reaction<C: sea_orm::ConnectionTrait>(
    &self,
    id: i32,
    txn: &C,
  ) -> Result<entity::bps::board_post_reactions::Model, SumidaError> {
    let reaction = ReactionEntity::find_by_id(id)
      .one(txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPostReaction".into(), id.to_string()))?;

    let is_owner = self.session.profile.is_authorized && reaction.user_id == Some(self.session.profile.id);
    if !is_owner && !self.session.profile.is_admin {
      return Err(SumidaError::NoAccessPermission);
    }

    Ok(reaction)
  }
}

/// Recompute and persist `board_posts.reactions` as a `{ value: count }` map for the given post,
/// so the post payload reflects current reactions without a separate aggregation query on read.
async fn refresh_reactions_aggregate<C: sea_orm::ConnectionTrait>(
  post_id: i32,
  db: &C,
) -> Result<(), SumidaError> {
  use sea_orm::sea_query::{Expr, Func};

  let rows = ReactionEntity::find()
    .select_only()
    .column(ReactionColumn::Value)
    .expr_as(Func::count(Expr::col(ReactionColumn::Id)), "count")
    .filter(ReactionColumn::PostId.eq(post_id))
    .group_by(ReactionColumn::Value)
    .into_tuple::<(String, i64)>()
    .all(db)
    .await?;

  let map: serde_json::Map<String, serde_json::Value> =
    rows.into_iter().map(|(value, count)| (value, serde_json::Value::from(count))).collect();
  let reactions = if map.is_empty() { None } else { Some(serde_json::Value::Object(map)) };

  BoardPosts::update_many()
    .set(entity::bps::board_posts::ActiveModel {
      reactions: Set(reactions),
      ..Default::default()
    })
    .filter(BoardPostsColumn::Id.eq(post_id))
    .exec(db)
    .await?;

  Ok(())
}

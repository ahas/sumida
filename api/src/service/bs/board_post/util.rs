use entity::bs::boards::BoardPostContentVisibility;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::sea_query::{Alias, Expr, Func};
use sea_orm::{
  ColumnTrait, ConnectionTrait, DbErr, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationTrait,
  Select, Set,
};

use crate::service::prelude::{BoardFileListItem, BoardPostFilter, GetBoardPost};

pub fn with_conditional_post_columns(
  user_id: Option<i32>,
  is_admin: bool,
  mut query: Select<BoardPosts>,
) -> Select<BoardPosts> {
  use BoardPostsColumn::{Reactions, Views};

  const VIEWS_COL: &str = "views";
  const REACTIONS_COL: &str = "reactions";

  fn add_column<T: Into<sea_orm::sea_query::Value>>(
    user_id: Option<i32>,
    query: Select<BoardPosts>,
    col: BoardPostsColumn,
    col_alias: &str,
    default_value: T,
  ) -> Select<BoardPosts> {
    match user_id {
      Some(user_id) => {
        let case_expr = Expr::case(
          //
          Expr::col(BoardPostsColumn::UserId).eq(user_id),
          Expr::col(col),
        )
        .finally(Expr::val(default_value));
        query.expr_as(case_expr, col_alias)
      }
      None => query.column_as(Expr::val(default_value), col_alias),
    }
  }

  if is_admin {
    query = query.column_as(Views, VIEWS_COL).column_as(Reactions, REACTIONS_COL)
  } else {
    query = add_column(user_id, query, Views, VIEWS_COL, None::<i32>);
    query = add_column(user_id, query, Reactions, REACTIONS_COL, None::<serde_json::Value>);
  }

  query
}

pub fn with_fts(
  query: Select<BoardPosts>,
  search_text: String,
  filter: &BoardPostFilter,
) -> Select<BoardPosts> {
  use crate::util::board::TsHeadlineExpr;

  if search_text.len() > 0 {
    TsHeadlineExpr::new(BoardPostsColumn::Text, &search_text)
      .start_offset(filter.snippet_offset)
      .delimiters(&filter.snippet_start_sel, &filter.snippet_stop_sel)
      .max_words(filter.snippet_max_words)
      .min_words(filter.snippet_min_words)
      .max_fragments(filter.snippet_max_fragments)
      .short_word(filter.snippet_short_word)
      .max_length(filter.snippet_length)
      .highlight_all(filter.snippet_highlight_all)
      .apply(query)
  } else if let Some(snippet_length) = filter.snippet_length {
    query.column_as(
      Expr::expr(Func::cust("left").arg(Expr::col(Alias::new("text"))).arg(snippet_length as i32)),
      "snippet",
    )
  } else {
    query.expr(Expr::col(Alias::new("text")))
  }
}

pub fn check_post_visible(
  user_id: Option<i32>,
  post_user_id: Option<i32>,
  is_admin: bool,
  board_content_visibility: BoardPostContentVisibility,
  post_content_visibility: BoardPostContentVisibility,
) -> Result<bool, SumidaError> {
  let content_visibility = if board_content_visibility < post_content_visibility {
    board_content_visibility
  } else {
    post_content_visibility
  };

  return match content_visibility {
    BoardPostContentVisibility::Public => Ok(true),
    BoardPostContentVisibility::UserOnly => Ok(user_id.is_some()),
    BoardPostContentVisibility::Private => {
      if is_admin {
        Ok(true)
      } else {
        let user_id = user_id.unwrap_or(-1);
        let post_user_id = post_user_id.unwrap_or(-2);

        Ok(user_id == post_user_id)
      }
    }
  };
}

pub async fn load_attached_files<'db, C: ConnectionTrait>(
  mut post: Option<GetBoardPost>,
  id: i32,
  db: &'db C,
) -> Result<Option<GetBoardPost>, DbErr> {
  if let Some(ref mut post) = post {
    post.attached_files = BoardFiles::find()
      .filter(BoardFilesColumn::PostId.eq(id))
      .join(JoinType::LeftJoin, BoardFilesRelation::File.def())
      .join(JoinType::LeftJoin, FilesRelation::User.def())
      .into_partial_model::<BoardFileListItem>()
      .all(db)
      .await?;
  }

  Ok(post)
}

pub async fn increase_view_count_if_not_viewed<'db, C: ConnectionTrait>(
  post_id: i32,
  last_views: i32,
  user_id: i32,
  db: &'db C,
) -> Result<i32, DbErr> {
  let not_viewed = BoardPostViews::find()
    .filter(BoardPostViewsColumn::PostId.eq(post_id))
    .filter(BoardPostViewsColumn::UserId.eq(user_id))
    .one(db)
    .await?
    .is_none();
  let next_views = if not_viewed { last_views } else { last_views + 1 };

  if not_viewed {
    let view_am = BoardPostViewsActiveModel {
      post_id: Set(post_id),
      user_id: Set(Some(user_id)),
      ..Default::default()
    };

    BoardPostViews::insert(view_am).exec(db).await?;
    BoardPosts::update(BoardPostsActiveModel {
      views: Set(next_views),
      ..Default::default()
    })
    .exec(db)
    .await?;
  }

  Ok(next_views)
}

use entity::bps::board_posts::BoardPostContentType;
use entity::bps::boards::BoardPostContentVisibility;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::sea_query::{Alias, Expr};
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  DbErr,
  EntityTrait,
  JoinType,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  RelationTrait,
  Select,
  Set,
};
use sea_orm_crud::Nullable;
use uuid::Uuid;

use crate::service::prelude::{BoardFileListItem, BoardPostFilter, GetBoardPost};
use crate::util::board::{HtmlContent, MarkdownContent};
use crate::util::file_lifecycle;
use crate::util::file_validation::FileValidator;

/// Max post title length (varchar bound) and max content body size (guards against unbounded bodies / DoS).
pub const MAX_TITLE_LEN: usize = 255;
pub const MAX_CONTENT_BYTES: usize = 1_000_000;

/// Reject over-long title/content before persisting.
pub fn validate_lengths(title: Option<&str>, content: Option<&str>) -> Result<(), SumidaError> {
  if let Some(title) = title
    && title.chars().count() > MAX_TITLE_LEN
  {
    return Err(SumidaError::BadRequest(format!("title exceeds {MAX_TITLE_LEN} characters")));
  }
  if let Some(content) = content
    && content.len() > MAX_CONTENT_BYTES
  {
    return Err(SumidaError::BadRequest(format!("content exceeds {MAX_CONTENT_BYTES} bytes")));
  }
  Ok(())
}

pub fn parse_plain_text(
  content_type: BoardPostContentType,
  content: Nullable<String>,
) -> Result<String, SumidaError> {
  let content: Option<String> = content.into();
  let Some(content) = content else {
    return Ok("".to_string());
  };

  Ok(match content_type {
    BoardPostContentType::Html => HtmlContent(&content).to_plain_text(),
    BoardPostContentType::Markdown => MarkdownContent(&content).to_plain_text(),
    BoardPostContentType::Plain => content.clone(),
  })
}

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
    // Non-owners don't see the real view count; default to 0 (the DTO `views` is non-nullable).
    query = add_column(user_id, query, Views, VIEWS_COL, 0i32);
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

  if !search_text.is_empty() {
    TsHeadlineExpr::new(BoardPostsColumn::Text, &search_text)
      .start_offset(filter.snippet_offset)
      .max_words(filter.snippet_max_words)
      .min_words(filter.snippet_min_words)
      .max_fragments(filter.snippet_max_fragments)
      .short_word(filter.snippet_short_word)
      .max_length(filter.snippet_length)
      .highlight_all(filter.snippet_highlight_all)
      .apply(query)
  } else if let Some(snippet_length) = filter.snippet_length {
    // Cut first and escape after, so the cut never splits an entity.
    query.column_as(
      Expr::cust_with_values(
        crate::util::board::html_escaped_sql("left(\"board_posts\".\"text\", $1)"),
        [snippet_length as i32],
      ),
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
  // The effective visibility is the MORE restrictive of board and post (Public < MembersOnly < Private):
  // a board can tighten a post's visibility but never loosen it.
  let content_visibility = board_content_visibility.max(post_content_visibility);

  match content_visibility {
    BoardPostContentVisibility::Public => Ok(true),
    BoardPostContentVisibility::MembersOnly => Ok(user_id.is_some()),
    BoardPostContentVisibility::Private => {
      if is_admin {
        Ok(true)
      } else {
        let user_id = user_id.unwrap_or(-1);
        let post_user_id = post_user_id.unwrap_or(-2);

        Ok(user_id == post_user_id)
      }
    }
  }
}

/// Row-level visibility predicate for list/search so private (and members-only) posts are not
/// exposed — title and FTS snippet alike — to users who can't see their content. Admins skip this.
/// Mirrors [`check_post_visible`]: effective = the stricter of board and post visibility.
pub fn content_visible_filter(user_id: Option<i32>) -> sea_orm::Condition {
  use sea_orm::Condition;

  // public=0 < members-only=1 < private=2; effective is the larger (stricter) of board and post.
  let effective = "GREATEST(\
      CASE board_posts.content_visibility WHEN 'private' THEN 2 WHEN 'members-only' THEN 1 ELSE 0 END, \
      CASE boards.content_visibility WHEN 'private' THEN 2 WHEN 'members-only' THEN 1 ELSE 0 END)";

  match user_id {
    // Unauthenticated: only fully public posts.
    None => Condition::all().add(Expr::cust(format!("{effective} = 0"))),
    // Authenticated: public + members-only for anyone; private only for its author.
    Some(uid) => Condition::any()
      .add(Expr::cust(format!("{effective} <= 1")))
      .add(Expr::cust_with_values("board_posts.user_id = $1", [uid])),
  }
}

pub async fn load_attached_files<C: ConnectionTrait>(
  mut post: Option<GetBoardPost>,
  id: i32,
  db: &C,
) -> Result<Option<GetBoardPost>, DbErr> {
  if let Some(ref mut post) = post {
    let mut files = BoardFiles::find()
      .filter(BoardFilesColumn::PostId.eq(id))
      .join(JoinType::LeftJoin, BoardFilesRelation::File.def())
      .join(JoinType::LeftJoin, FilesRelation::User.def())
      .into_partial_model::<BoardFileListItem>()
      .all(db)
      .await?;
    // Expose the access-gated read-only path, not the raw storage URI.
    for file in &mut files {
      file.file.uri = Some(crate::service::fs::dto::inline_uri(file.file.uuid));
    }
    post.attached_files = files;
  }

  Ok(post)
}

/// Extract file uuids referenced inline in post content (tiptap `<img src>`, `<a href>`, media tags).
/// Markdown is rendered to HTML first; plain text carries no references.
pub fn extract_referenced_file_uuids(content: &str, content_type: &BoardPostContentType) -> Vec<Uuid> {
  use scraper::{Html, Selector};

  let html = match content_type {
    BoardPostContentType::Html => content.to_string(),
    BoardPostContentType::Markdown => markdown::to_html(content),
    BoardPostContentType::Plain => return Vec::new(),
  };

  let doc = Html::parse_fragment(&html);
  let Ok(selector) = Selector::parse("img, a, source, video, audio") else {
    return Vec::new();
  };

  let mut uuids: Vec<Uuid> = Vec::new();
  for el in doc.select(&selector) {
    for attr in ["src", "href"] {
      if let Some(url) = el.value().attr(attr)
        && let Some(uuid) = uuid_from_url(url)
        && !uuids.contains(&uuid)
      {
        uuids.push(uuid);
      }
    }
  }

  uuids
}

/// Pull the first uuid-looking token out of a URL (path segment or query value, extension stripped).
fn uuid_from_url(url: &str) -> Option<Uuid> {
  url
    .split(['/', '?', '=', '&', '#', '"', '\'', ' '])
    .filter_map(|seg| seg.split('.').next())
    .find_map(|token| Uuid::parse_str(token).ok())
}

/// Attach the given files plus any referenced inline in `content` to `post_id`, enforcing the
/// board's file policy and `max_file_count`. Pending claims are promoted; generic files get a fresh
/// attached link.
///
/// Only `uploader`'s own unattached files are attached. An explicitly listed file someone else owns
/// refuses the whole write; a file someone else owns that the content merely links to is left alone,
/// since linking to a file is not taking it. A file newly attached is held to this board's size and
/// type policy, whichever route it was uploaded through.
pub async fn claim_post_files<C: ConnectionTrait>(
  board_id: i32,
  post_id: i32,
  uploader: i32,
  explicit: Vec<Uuid>,
  content: Option<&str>,
  content_type: &BoardPostContentType,
  db: &C,
) -> Result<(), SumidaError> {
  let mut uuids = explicit;
  if let Some(content) = content {
    for uuid in extract_referenced_file_uuids(content, content_type) {
      if !uuids.contains(&uuid) && file_lifecycle::is_uploaded_by(uuid, uploader, db).await? {
        uuids.push(uuid);
      }
    }
  }
  uuids.sort();
  uuids.dedup();

  let config = FileValidator::from_board_id(board_id, db).await?.get_config().clone();
  for uuid in uuids {
    if let Some(file) = file_lifecycle::claim_file(post_id, uuid, uploader, db).await? {
      let extension = std::path::Path::new(&file.original_name).extension().and_then(|ext| ext.to_str());
      FileValidator::new(config.clone())
        .validate_size(file.size.max(0) as usize)?
        .validate_extension(extension)?;
    }
  }

  // Enforce the board's max_file_count over the resulting attached set.
  if config.max_file_count != 0 {
    let total = BoardFiles::find().filter(BoardFilesColumn::PostId.eq(post_id)).count(db).await? as usize;
    if total > config.max_file_count {
      return Err(SumidaError::TooManyFiles {
        current_count: total,
        max_count:     config.max_file_count,
      });
    }
  }

  Ok(())
}

/// Detach specific files from a post (decrementing refs / re-arming the TTL when they hit 0).
pub async fn unclaim_post_files<C: ConnectionTrait>(
  post_id: i32,
  file_uuids: Vec<Uuid>,
  db: &C,
) -> Result<(), SumidaError> {
  for uuid in file_uuids {
    file_lifecycle::unclaim_file(post_id, uuid, db).await?;
  }

  Ok(())
}

/// Collect a post id plus all of its descendants (the comment/reply subtree via `parent_id`),
/// so a soft delete can cascade to replies that an FK cascade does not reach.
pub async fn collect_descendant_post_ids<C: ConnectionTrait>(
  root_id: i32,
  db: &C,
) -> Result<Vec<i32>, sea_orm::DbErr> {
  use sea_orm::{DbBackend, Statement};

  let sql = r#"
    WITH RECURSIVE descendants AS (
      SELECT id FROM board_posts WHERE id = $1
      UNION ALL
      SELECT bp.id FROM board_posts bp JOIN descendants d ON bp.parent_id = d.id
    )
    SELECT id FROM descendants
  "#;

  let rows = db
    .query_all(Statement::from_sql_and_values(DbBackend::Postgres, sql, [root_id.into()]))
    .await?;

  rows.iter().map(|row| row.try_get::<i32>("", "id")).collect()
}

/// Release every file attached to a post (used before deleting the post) so refs are decremented
/// and the files become reapable rather than orphaned by the FK cascade.
pub async fn release_all_post_files<C: ConnectionTrait>(post_id: i32, db: &C) -> Result<(), SumidaError> {
  let uuids = BoardFiles::find()
    .filter(BoardFilesColumn::PostId.eq(post_id))
    .select_only()
    .column(BoardFilesColumn::FileUuid)
    .into_tuple::<Uuid>()
    .all(db)
    .await?;

  for uuid in uuids {
    file_lifecycle::unclaim_file(post_id, uuid, db).await?;
  }

  Ok(())
}

pub async fn increase_view_count_if_not_viewed<C: ConnectionTrait>(
  post_id: i32,
  last_views: i32,
  user_id: i32,
  db: &C,
) -> Result<i32, DbErr> {
  use sea_orm::sea_query::OnConflict;

  // Race-safe "view once per user": rely on the unique (post_id, user_id) index. The marker insert
  // does nothing on conflict; only a freshly inserted row bumps the count (atomically, no lost update).
  let inserted = match BoardPostViews::insert(BoardPostViewsActiveModel {
    post_id: Set(post_id),
    user_id: Set(Some(user_id)),
    ..Default::default()
  })
  .on_conflict(
    OnConflict::columns([BoardPostViewsColumn::PostId, BoardPostViewsColumn::UserId])
      .do_nothing()
      .to_owned(),
  )
  .exec(db)
  .await
  {
    Ok(_) => true,
    Err(DbErr::RecordNotInserted) => false,
    Err(err) => return Err(err),
  };

  if inserted {
    BoardPosts::update_many()
      .col_expr(BoardPostsColumn::Views, Expr::col(BoardPostsColumn::Views).add(1))
      .filter(BoardPostsColumn::Id.eq(post_id))
      .exec(db)
      .await?;
  }

  Ok(last_views + i32::from(inserted))
}

#[cfg(test)]
mod tests {
  use super::*;

  const UUID_A: &str = "550e8400-e29b-41d4-a716-446655440000";
  const UUID_B: &str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";

  fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
  }

  #[test]
  fn extracts_inline_image_and_link_from_html() {
    let html = format!(
      r#"<p>hi</p><img src="/api/files/{UUID_A}/inline" alt="x"><a href="/files/{UUID_B}/download">f</a>"#
    );
    let got = extract_referenced_file_uuids(&html, &BoardPostContentType::Html);
    assert_eq!(got, vec![uuid(UUID_A), uuid(UUID_B)]);
  }

  #[test]
  fn extracts_from_markdown_with_extension() {
    let md = format!("![pic](/files/{UUID_A}.png)");
    let got = extract_referenced_file_uuids(&md, &BoardPostContentType::Markdown);
    assert_eq!(got, vec![uuid(UUID_A)]);
  }

  #[test]
  fn dedups_repeated_references() {
    let html = format!(r#"<img src="/files/{UUID_A}/inline"><img src="/files/{UUID_A}.png">"#);
    let got = extract_referenced_file_uuids(&html, &BoardPostContentType::Html);
    assert_eq!(got, vec![uuid(UUID_A)]);
  }

  #[test]
  fn plain_text_has_no_references() {
    let text = format!("see file {UUID_A} please");
    assert!(extract_referenced_file_uuids(&text, &BoardPostContentType::Plain).is_empty());
  }

  #[test]
  fn ignores_non_uuid_urls() {
    let html = r#"<img src="/static/logo.png"><a href="https://example.com/page?x=1">l</a>"#;
    assert!(extract_referenced_file_uuids(html, &BoardPostContentType::Html).is_empty());
  }
}

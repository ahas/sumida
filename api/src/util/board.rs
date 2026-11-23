use sea_orm::JoinType;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, Func, Query};

pub struct HtmlContent<'a>(pub &'a str);

impl<'a> HtmlContent<'a> {
  pub fn to_plain_text(&self) -> String {
    use scraper::Html;

    let fragment = Html::parse_fragment(self.0);

    fragment.root_element().text().collect::<Vec<_>>().join(" ")
  }
}

pub struct MarkdownContent<'a>(pub &'a str);

impl<'a> MarkdownContent<'a> {
  pub fn to_plain_text(&self) -> String {
    use scraper::Html;

    let html = markdown::to_html(self.0);
    let fragment = Html::parse_fragment(&html);

    fragment.root_element().text().collect::<Vec<_>>().join(" ")
  }
}

/// The marks a search snippet wraps its hits in. The server's alone: the snippet is HTML a client
/// renders, so no caller chooses what goes into it.
pub const SNIPPET_START_SEL: &str = "<mark>";
pub const SNIPPET_STOP_SEL: &str = "</mark>";

/// `column` as SQL text with the five HTML-significant characters escaped, so markup a post's plain
/// text carries (entity-decoded from its body) comes out of a snippet as text, never as tags.
pub fn html_escaped_sql(column: &str) -> String {
  let mut sql = column.to_string();
  for (from, to) in [("&", "&amp;"), ("<", "&lt;"), (">", "&gt;"), ("\"", "&quot;"), ("''", "&#39;")] {
    sql = format!("replace({sql}, '{from}', '{to}')");
  }
  sql
}

#[derive(Debug, Clone)]
pub struct TsHeadlineExpr {
  config:        String,
  column:        String,
  keyword:       String,
  start_offset:  usize,
  max_fragments: usize,
  max_length:    Option<usize>,
  max_words:     usize,
  min_words:     usize,
  short_word:    usize,
  highlight_all: bool,
}

impl TsHeadlineExpr {
  pub fn new<C>(column: C, keyword: &str) -> Self
  where
    C: ColumnTrait,
  {
    Self {
      config:        "simple".into(),
      column:        column.to_string(),
      keyword:       keyword.into(),
      start_offset:  0,
      max_fragments: 1,
      max_length:    None,
      max_words:     20,
      min_words:     5,
      short_word:    3,
      highlight_all: false,
    }
  }

  pub fn config(mut self, config: &str) -> Self {
    self.config = config.to_string();
    self
  }
  pub fn start_offset(mut self, n: usize) -> Self {
    self.start_offset = n;
    self
  }
  pub fn max_fragments(mut self, n: usize) -> Self {
    self.max_fragments = n;
    self
  }
  pub fn max_length(mut self, n: Option<usize>) -> Self {
    self.max_length = n;
    self
  }
  pub fn max_words(mut self, n: usize) -> Self {
    self.max_words = n;
    self
  }
  pub fn min_words(mut self, n: usize) -> Self {
    self.min_words = n;
    self
  }
  pub fn short_word(mut self, n: usize) -> Self {
    self.short_word = n;
    self
  }
  pub fn highlight_all(mut self, b: bool) -> Self {
    self.highlight_all = b;
    self
  }

  pub fn apply<E: EntityTrait>(self, mut query: Select<E>) -> Select<E> {
    // The options string is bound as a parameter ($2). The marks are fixed and the rest are numbers;
    // config is fixed to "simple" and column is a ColumnTrait.
    let options = format!(
      "StartSel={start}, StopSel={stop}, MaxWords={max_words}, MinWords={min_words}, \
       ShortWord={short_word}, MaxFragments={max_fragments}, HighlightAll={highlight_all}",
      start = SNIPPET_START_SEL,
      stop = SNIPPET_STOP_SEL,
      max_words = self.max_words,
      min_words = self.min_words,
      short_word = self.short_word,
      max_fragments = self.max_fragments,
      highlight_all = if self.highlight_all { "TRUE" } else { "FALSE" }
    );

    // The document is headlined already escaped: the only markup in the result is the marks.
    let sql = format!(
      "ts_headline('{config}', {document}, plainto_tsquery('{config}', $1), $2)",
      config = self.config,
      document = html_escaped_sql(&self.column),
    );

    query.query().join_lateral(
      JoinType::LeftJoin,
      Query::select()
        .expr_as(Expr::cust_with_values(sql, [self.keyword.clone(), options]), "snippet")
        .take(),
      Alias::new("H"),
      Expr::cust("TRUE"),
    );

    let highlighted_snippet = Expr::col((Alias::new("H"), "snippet"));
    let pos = Func::cust("strpos").arg(highlighted_snippet.clone()).arg(SNIPPET_START_SEL);

    if let Some(max_length) = self.max_length {
      query.query().expr_as(
        Func::cust("substring")
          .arg(highlighted_snippet)
          .arg(Func::cust("greatest").arg(Expr::expr(pos).sub(self.start_offset as i32)).arg(0))
          .arg(max_length as i32),
        "snippet",
      );
    } else {
      query.query().expr_as(highlighted_snippet, "snippet");
    }

    query
  }
}

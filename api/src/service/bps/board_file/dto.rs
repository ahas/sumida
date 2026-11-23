use async_trait::async_trait;
use axum::Json;
use axum::extract::Multipart;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::bps::board_files::*;
use entity::fs::files::Column as FileColumn;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{EntityTrait, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::service::fs::dto::{
  FileListItem,
  FileUploadData,
  FileUploadOptions,
  FileUploadTrait,
  GetFile,
  UploadFile,
};
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BoardFileFilter {
  /// The attachment's file uuid — `board_files.file_uuid`, which is what identifies the row.
  #[cfg_attr(feature = "openapi", param(inline))]
  pub id:      Option<OneOrMany<Uuid>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub post_id: Option<OneOrMany<i32>>,
}

impl BoardFileFilter {
  pub fn apply<E: EntityTrait>(&self, query: Select<E>) -> Select<E> {
    query
      .apply_if(listed(self.id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::FileUuid)))
      .apply_if(listed(self.post_id.as_ref()), |q, v| q.filter(v.into_simple_expr(Column::PostId)))
  }
}

/// What `/board-files` accepts, declared once.
///
/// An attachment is a join row over a file, so every key the page shows and orders by stands on a
/// column of the joined file. The list already joins it, which is what lets one expression serve
/// the value the row shows, the filter and the ordering alike.
const BOARD_FILE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("id", FieldKind::Uuid, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/files", "name"),
  FieldSpec::new("post_id", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_relation("/board-posts", "title")
    .with_facet(),
  FieldSpec::sort_only("dir", FieldKind::Text),
  FieldSpec::sort_only("name", FieldKind::Text),
  FieldSpec::sort_only("size", FieldKind::Integer),
  FieldSpec::sort_only("mime_type", FieldKind::Text),
  FieldSpec::sort_only("status", FieldKind::Enum).with_variants(&[
    "pending",
    "temporary",
    "active",
    "archived",
    "deleted",
    "error",
  ]),
  FieldSpec::sort_only("created_at", FieldKind::DateTime),
];

impl FilterSchema for BoardFileFilter {
  fn fields() -> &'static [FieldSpec] {
    BOARD_FILE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "id" => Column::FileUuid.into_simple_expr(),
      "post_id" => Column::PostId.into_simple_expr(),
      "dir" => FileColumn::Dir.into_simple_expr(),
      "name" => FileColumn::Name.into_simple_expr(),
      "size" => FileColumn::Size.into_simple_expr(),
      "mime_type" => FileColumn::MimeType.into_simple_expr(),
      "status" => FileColumn::Status.into_simple_expr(),
      "created_at" => FileColumn::CreatedAt.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "created_at"
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardFiles", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileListItem {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub file:    FileListItem,
  pub post_id: i32,
  // pub path:          String,
  // pub original_name: String,
  // pub uri:           Option<String>,
  // pub size:          i64,
  // pub mime_type:     String,
  // pub status:        String,
  // pub is_temporary:  bool,
  // pub expires_at:    Option<DateTimeUtc>,
  // pub created_at:    DateTimeUtc,
  // pub updated_at:    DateTimeUtc,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "BoardFiles", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetBoardFile {
  #[sea_orm(nested)]
  #[serde(flatten)]
  pub file:    GetFile,
  pub post_id: i32,
  // pub id:            i32,
  // pub user_id:       Option<i32>,
  // pub path:          String,
  // pub original_name: String,
  // pub uri:           Option<String>,
  // pub size:          i64,
  // pub mime_type:     String,
  // pub hash:          Option<String>,
  // pub status:        String,
  // pub is_temporary:  bool,
  // pub expires_at:    Option<DateTimeUtc>,
  // pub created_at:    DateTimeUtc,
  // pub updated_at:    DateTimeUtc,
}

impl IntoResponse for GetBoardFile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadBoardFile {
  pub board_id: i32,
  pub file:     FileUploadData,
  pub options:  FileUploadOptions,
}

#[async_trait]
impl FileUploadTrait for UploadBoardFile {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions) {
    (self.file, self.options)
  }

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError> {
    let mut board_id: Option<i32> = None;
    let mut file_data: Option<FileUploadData> = None;
    let mut options: Option<FileUploadOptions> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|err| SumidaError::BadRequest(format!("Multipart error: {err}")))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
        "board_id" => {
          let text = field
            .text()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Invalid board_id: {err}")))?;
          board_id = Some(
            text
              .trim()
              .parse::<i32>()
              .map_err(|err| SumidaError::BadRequest(format!("Invalid board_id: {err}")))?,
          );
        }
        "metadata" => {
          let text = field
            .text()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Invalid metadata: {err}")))?;
          options = Some(
            serde_json::from_str(&text)
              .map_err(|err| SumidaError::BadRequest(format!("Invalid metadata JSON: {err}")))?,
          );
        }
        "file" => {
          let filename = field.file_name().unwrap_or("unknown").to_string();
          let data = field
            .bytes()
            .await
            .map_err(|err| SumidaError::BadRequest(format!("Failed to read file: {err}")))?;
          // The part's declared Content-Type is the client's word and is not read: the type is the
          // server's reading of the bytes.
          let mime_type = crate::util::file_validation::resolve_mime_type(&filename, &data);

          file_data = Some(FileUploadData {
            original_name: filename,
            bytes: data.to_vec(),
            mime_type,
          });
        }
        _ => {}
      }
    }

    Ok(Self {
      board_id: board_id.ok_or_else(|| SumidaError::BadRequest("Missing field: board_id".into()))?,
      file:     file_data.ok_or_else(|| SumidaError::BadRequest("Missing field: file".into()))?,
      options:  options.ok_or_else(|| SumidaError::BadRequest("Missing field: metadata".into()))?,
    })
  }
}

impl From<UploadBoardFile> for UploadFile {
  fn from(value: UploadBoardFile) -> Self {
    UploadFile {
      file:    value.file,
      options: value.options,
    }
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileCreated {
  pub id:      i32,
  pub post_id: i32,
}

impl IntoResponse for BoardFileCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(json!(self))).into_response()
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileUpdated;

impl IntoResponse for BoardFileUpdated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::OK, Json(json!(self))).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBoardFile {
  #[serde(default)]
  pub hard:      bool,
  pub post_id:   i32,
  pub file_uuid: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardFileDeleted;

impl IntoResponse for BoardFileDeleted {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::OK, Json(json!(self))).into_response()
  }
}

#[cfg(test)]
mod tests {
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    BoardFileFilter::validate().expect("the board file filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let filter: BoardFileFilter =
      serde_urlencoded::from_str("id=8b2f5d3e-0f2c-4f9a-9f6b-3f1a2c4d5e6f&post_id=1")
        .expect("the flat filter parses");

    assert!(filter.id.is_some());
    assert!(filter.post_id.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: BoardFileFilter = serde_urlencoded::from_str(
      "id=8b2f5d3e-0f2c-4f9a-9f6b-3f1a2c4d5e6f,1c9e3a70-2b44-4d8e-8f11-77a0c2e5b913&post_id=1,2",
    )
    .expect("a list parses");

    assert_eq!(filter.id.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.post_id.as_ref().map(OneOrMany::len), Some(2));

    let filter: BoardFileFilter =
      serde_urlencoded::from_str("id=&post_id=").expect("an empty value parses");

    assert!(listed(filter.id.as_ref()).is_none());
    assert!(listed(filter.post_id.as_ref()).is_none());
  }

  #[test]
  fn an_unreadable_identifier_is_refused() {
    assert!(serde_urlencoded::from_str::<BoardFileFilter>("id=not-a-uuid").is_err());
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["file_uuid", "postId", "user_id", "hash", "uri"] {
      assert!(BoardFileFilter::field(spelling).is_none(), "{spelling}");
      assert!(BoardFileFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(BoardFileFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["dir", "name", "size", "mime_type", "status", "created_at"] {
      assert!(BoardFileFilter::sort_column(key).is_some(), "{key}");
    }
    // Declared filterable, but not sortable.
    assert!(BoardFileFilter::sort_column("id").is_none());
    assert!(BoardFileFilter::sort_column("post_id").is_none());
  }

  /// One caller for the rule compiler. Which tuples it would match is never read — what these
  /// assert is the shape of the statement, not the rows it returns.
  fn caller() -> SubjectSet {
    SubjectSet::from_parts(vec!["user:7".to_owned(), "role:3#member".to_owned()], vec![7])
  }

  /// The read scope the list is built from. The projection and the joins the service adds are left
  /// out on purpose: what is under test is how the scope and the condition tree meet in the
  /// `WHERE` clause, since a filter may only narrow an already-authorized scope
  /// (`docs/list-filters.md`).
  fn scope() -> Select<Entity> {
    Authz::filter(super::super::RESOURCE, "view", &caller(), Entity::find())
      .expect("the read scope compiles")
  }

  fn where_of(query: Select<Entity>) -> String {
    use sea_orm::QueryTrait;
    use sea_orm::sea_query::PostgresQueryBuilder;

    let sql = query.as_query().to_string(PostgresQueryBuilder);

    sql.split_once(" WHERE ").expect("a WHERE clause").1.to_owned()
  }

  /// The tree as the one condition a list adds to a query it has already scoped.
  fn compiled(raw: &str) -> Condition {
    ConditionTree::parse(raw)
      .expect("the tree parses")
      .compile::<BoardFileFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"post_id","op":"in","value":[1,2]},
      {"type":"field","key":"id","op":"eq","value":"0195b0a1-0000-7000-8000-000000000000"}
    ]}"#;

    let bare = where_of(Entity::find().filter(compiled(TREE)));
    let scoped = where_of(scope().filter(compiled(TREE)));

    assert!(bare.contains(" OR "), "{bare}");
    assert!(scoped.contains(&where_of(scope())), "the scope must survive whole: {scoped}");
    assert!(
      scoped.ends_with(&format!(" AND ({bare})")),
      "the tree must be one more conjunct: {scoped}"
    );
  }

  /// `status` is declared as an enum but takes no operator, so no leaf can name it: there is no
  /// bound value for a cast to land on. Ordering by it is the whole of what the key offers.
  #[test]
  fn the_sort_only_status_takes_no_leaf() {
    let status = BoardFileFilter::field("status").expect("`status` is declared");

    assert!(status.ops.is_empty());
    assert!(BoardFileFilter::sort_column("status").is_some());

    let tree = ConditionTree::parse(r#"{"type":"field","key":"status","op":"eq","value":"active"}"#)
      .expect("the tree parses");

    assert!(tree.compile::<BoardFileFilter>().is_err());
  }
}

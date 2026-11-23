use async_trait::async_trait;
use axum::Json;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::fs::files::FileStatus;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::prelude::*;
use sea_orm::sea_query::{Alias, SimpleExpr};
use sea_orm::{ActiveEnum, Condition, IntoSimpleExpr, QueryTrait};
use sea_orm_crud::{Nullable, OneOrMany};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::SumidaState;
use crate::service::grants::ApForUser;
use crate::util::db::wrap_ilike;
use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op, listed};
use crate::util::storage::StaticFile;

/// Read-only, access-gated path exposed to clients instead of the raw internal storage URI.
/// The `/files/{uuid}/inline` endpoint enforces visibility (post content-visibility / `file:read`).
pub fn inline_uri(uuid: Uuid) -> String {
  format!("/files/{uuid}/inline")
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FileFilter {
  pub user:      Option<String>,
  pub dir:       Option<String>,
  pub name:      Option<String>,
  pub uri:       Option<String>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub status:    Option<OneOrMany<FileStatus>>,
  pub mime_type: Option<String>,
}

impl FileFilter {
  pub fn apply<E>(&self, query: Select<E>) -> Select<E>
  where
    E: EntityTrait,
  {
    use entity::fs::files::Column;

    query
      .apply_if(self.user.as_ref(), |q, v| {
        q.filter(
          Condition::any()
            .add(wrap_ilike(entity::users::Column::Username.into_expr(), v))
            .add(wrap_ilike(Expr::col((Users, Alias::new("full_name"))), v)),
        )
      })
      .apply_if(self.dir.as_ref(), |q, v| q.filter(wrap_ilike(Column::Dir.into_expr(), v)))
      .apply_if(self.name.as_ref(), |q, v| q.filter(wrap_ilike(Column::Name.into_expr(), v)))
      .apply_if(self.uri.as_ref(), |q, v| q.filter(wrap_ilike(Column::Uri.into_expr(), v)))
      .apply_if(listed(self.status.as_ref()), |q, v| {
        q.filter(Column::Status.is_in(v.iter().map(|s| s.clone().into_value())))
      })
      .apply_if(self.mime_type.as_ref(), |q, v| q.filter(wrap_ilike(Column::MimeType.into_expr(), v)))
  }
}

/// What `/files` accepts, declared once.
///
/// `user` widens the uploader's username and full name into one predicate, so it reduces to no
/// single expression and the resource builds it. Everything else stands on one column of the file.
const FILE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("user", FieldKind::Text, MatchStrategy::Custom, &[Op::Contains]),
  FieldSpec::new("dir", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains])
    .with_sort()
    .with_facet(),
  FieldSpec::new("name", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("uri", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains]).with_sort(),
  FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["pending", "temporary", "active", "archived", "deleted", "error"])
    .with_sort()
    .with_facet(),
  FieldSpec::new("mime_type", FieldKind::Text, MatchStrategy::Substring, &[Op::Contains])
    .with_sort()
    .with_facet(),
  FieldSpec::sort_only("uuid", FieldKind::Uuid),
];

impl FilterSchema for FileFilter {
  fn fields() -> &'static [FieldSpec] {
    FILE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    use entity::fs::files::Column;

    Some(match key {
      "dir" => Column::Dir.into_simple_expr(),
      "name" => Column::Name.into_simple_expr(),
      "uri" => Column::Uri.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "mime_type" => Column::MimeType.into_simple_expr(),
      "uuid" => Column::Uuid.into_simple_expr(),
      // `user` spans the uploader's username and full name, so it has no single column of its own.
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "uuid"
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Users", from_query_result)]
pub struct FileUser {
  pub id:       i32,
  pub username: String,
  #[sea_orm(from_col = "full_name")]
  pub name:     String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Files", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct FileListItem {
  pub uuid:          Uuid,
  #[sea_orm(nested)]
  pub user:          FileUser,
  pub original_name: String,
  pub dir:           String,
  pub name:          String,
  pub uri:           Option<String>,
  pub size:          i64,
  pub mime_type:     String,
  pub status:        String,
  pub tags:          Option<Vec<String>>,
  pub refs:          i32,
  pub expires_at:    Option<DateTimeUtc>,
  pub created_at:    DateTimeUtc,
  pub updated_at:    DateTimeUtc,
  pub deleted_at:    Option<DateTimeUtc>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "Files", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct GetFile {
  pub uuid:          Uuid,
  #[sea_orm(nested)]
  pub user:          FileUser,
  pub original_name: String,
  pub dir:           String,
  pub name:          String,
  pub uri:           Option<String>,
  pub size:          i64,
  pub mime_type:     String,
  pub hash:          Option<String>,
  pub tags:          Option<Vec<String>>,
  pub status:        String,
  pub refs:          i32,
  pub expires_at:    Option<DateTimeUtc>,
  pub created_at:    DateTimeUtc,
  pub updated_at:    DateTimeUtc,
  pub deleted_at:    Option<DateTimeUtc>,

  #[sea_orm(skip)]
  pub access_policies: Option<Vec<ApForUser>>,
}

impl IntoResponse for GetFile {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploadOptions {
  #[serde(default)]
  pub tags:       Option<Vec<String>>,
  #[serde(default)]
  pub dir:        String,
  #[serde(default)]
  pub expires_at: Nullable<DateTimeUtc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploadData {
  pub original_name: String,
  pub bytes:         Vec<u8>,
  pub mime_type:     String,
}

impl FileUploadData {
  pub fn hash(&self) -> String {
    blake3::hash(&self.bytes).to_hex().to_string()
  }

  pub fn get_extension(&self) -> Option<String> {
    std::path::Path::new(&self.original_name)
      .extension()
      .and_then(|ext| ext.to_str())
      .map(|x| x.to_owned())
  }
}

#[async_trait]
pub trait FileUploadTrait: Sized {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions);

  async fn from_request<S: SumidaState>(state: S, request: Request) -> Result<Self, SumidaError> {
    use axum::http::header;

    let content_type = request
      .headers()
      .get(header::CONTENT_TYPE)
      .and_then(|ct| ct.to_str().ok())
      .unwrap_or("");
    if content_type.starts_with("multipart/form-data") {
      let mut multipart = Multipart::from_request(request, &state)
        .await
        .map_err(|_| SumidaError::BadRequest("Invalid multipart data".into()))?;

      Self::from_multipart(&mut multipart).await
    } else {
      Err(SumidaError::BadRequest("Only multipart/form-data is supported for file uploads".into()))
    }
  }

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError>;

  fn into_static_file(self) -> StaticFile {
    let (file, options) = self.into_parts();
    let file_extension = file.get_extension();
    let file_uuid = Uuid::new_v4();
    let unique_filename = if let Some(ref ext) = file_extension {
      format!("{}.{}", file_uuid, ext)
    } else {
      file_uuid.to_string()
    };

    StaticFile {
      name:          unique_filename,
      dir:           options.dir,
      original_name: file.original_name,
      mime_type:     file.mime_type,
      bytes:         file.bytes,
      extension:     file_extension,
    }
  }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
  pub file:    FileUploadData,
  pub options: FileUploadOptions,
}

#[async_trait]
impl FileUploadTrait for UploadFile {
  fn into_parts(self) -> (FileUploadData, FileUploadOptions) {
    (self.file, self.options)
  }

  async fn from_multipart(multipart: &mut Multipart) -> Result<Self, SumidaError> {
    let mut file_data: Option<FileUploadData> = None;
    let mut options: Option<FileUploadOptions> = None;

    while let Some(field) = multipart
      .next_field()
      .await
      .map_err(|err| SumidaError::BadRequest(format!("Multipart error: {err}")))?
    {
      let name = field.name().unwrap_or("").to_string();

      match name.as_str() {
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

    let Some(file_data) = file_data else {
      return Err(SumidaError::FieldNotProvided("file".to_string()));
    };
    let Some(options) = options else {
      return Err(SumidaError::FieldNotProvided("metadata".to_string()));
    };

    Ok(Self {
      file: file_data,
      options,
    })
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileUploaded {
  pub uuid: Uuid,
}

impl IntoResponse for FileUploaded {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFile {
  #[serde(default)]
  pub hard: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDeleted;

impl IntoResponse for FileDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStats {
  pub total_files:  i64,
  pub total_size:   i64,
  pub by_mime_type: Vec<FileMimeTypeStats>,
  pub by_status:    Vec<FileStatusStats>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMimeTypeStats {
  pub mime_type:  String,
  pub count:      i64,
  pub total_size: i64,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatusStats {
  pub status: String,
  pub count:  i64,
}

#[cfg(test)]
mod filter_tests {
  use entity::fs::files::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::ConditionTree;

  #[test]
  fn the_declaration_is_sound() {
    FileFilter::validate().expect("the file filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "user=a&dir=b&name=c&uri=d&status=active&mime_type=image/png";
    let filter: FileFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.user.is_some());
    assert!(filter.dir.is_some());
    assert!(filter.name.is_some());
    assert!(filter.uri.is_some());
    assert!(filter.status.is_some());
    assert!(filter.mime_type.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: FileFilter = serde_urlencoded::from_str("status=active,archived").expect("a list parses");
    assert_eq!(filter.status.as_ref().map(OneOrMany::len), Some(2));

    let filter: FileFilter = serde_urlencoded::from_str("status=").expect("an empty value parses");
    assert!(listed(filter.status.as_ref()).is_none());
  }

  #[test]
  fn an_unknown_enum_variant_is_refused() {
    assert!(serde_urlencoded::from_str::<FileFilter>("status=shredded").is_err());
    assert!(serde_urlencoded::from_str::<FileFilter>("status=active,shredded").is_err());
  }

  /// Every variant the flat parameter parses is a variant the declaration names, so the two paths
  /// cannot disagree about what `status` accepts.
  #[test]
  fn the_declared_variants_are_the_ones_the_parameter_parses() {
    let status = FileFilter::field("status").expect("`status` is declared");

    for variant in status.variants {
      let filter: FileFilter = serde_urlencoded::from_str(&format!("status={variant}"))
        .unwrap_or_else(|_| panic!("`{variant}` must parse"));
      assert!(filter.status.is_some(), "{variant}");
    }
  }

  /// Nothing is answered by a refusal before this list runs, so there is no scope-selecting
  /// parameter to withhold. Every key the resource never declared stays unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["user_id", "hash", "mimeType", "tags", "refs", "deleted_at"] {
      assert!(FileFilter::field(spelling).is_none(), "{spelling}");
      assert!(FileFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(FileFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["dir", "name", "uri", "status", "mime_type", "uuid"] {
      assert!(FileFilter::sort_column(key).is_some(), "{key}");
    }
    // Spans two columns, so it neither resolves nor sorts.
    assert!(FileFilter::sort_column("user").is_none());
    assert!(FileFilter::column("user").is_none());
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
    Authz::filter("file", "view", &caller(), Entity::find()).expect("the read scope compiles")
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
      .compile::<FileFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"name","op":"contains","value":"report"},
      {"type":"field","key":"status","op":"eq","value":"active"}
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

  /// The column stores text, so the bound value needs no cast. A key backed by a database enum
  /// names its type in the declaration instead, and the cast lands in the predicate.
  #[test]
  fn a_text_backed_enum_binds_without_a_cast() {
    use sea_orm::QueryFilter;

    let predicate = where_of(
      Entity::find().filter(compiled(r#"{"type":"field","key":"status","op":"eq","value":"active"}"#)),
    );

    assert!(FileFilter::enum_type("status").is_none());
    assert!(predicate.contains("'active'"), "{predicate}");
    assert!(!predicate.contains("CAST("), "{predicate}");
  }
}

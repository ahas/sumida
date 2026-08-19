pub mod dto;
pub mod util;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ColumnTrait,
  DeleteResult,
  EntityTrait,
  JoinType,
  PaginatorTrait,
  QueryFilter,
  QuerySelect,
  RelationTrait,
  Select,
  TransactionTrait,
};
use uuid::Uuid;

use self::dto::*;
use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::service::fs::dto::{DeleteFile, FileUploaded};
use crate::service::fs::file::FileService;
use crate::util::file_validation::FileValidator;
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// 이 서비스가 판정하는 정의(`schema.sz`의 `namespace board_file service = board_file`).
const RESOURCE: &str = "board_file";

pub struct BoardFileService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> BoardFileService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// 호출자의 인가 주체 집합. 보안 API 키는 키 자신이 곧 정체성이라 소유자의
  /// user/role/organization 으로 펼쳐지지 않는다.
  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// 행 스코프는 `board_file#view` 술어 하나 — 첨부는 자기 글(`post->view`)이나 원본
  /// 파일(`file->view`) 중 하나로 열린다. 종전의 `join(BoardFiles, Column::PostId)` 는 첨부의
  /// 객체를 글 id 로 잡고 있었는데, 정의는 `board_files.file_uuid` 를 키로 삼으므로 그 어긋남도
  /// 함께 사라진다.
  fn select(&self, query: Select<BoardFiles>) -> Result<Select<BoardFiles>, SumidaError> {
    Ok(Authz::filter(RESOURCE, "view", &self.subject(), query)?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `BoardFileFilter` 's own declaration.
  pub async fn list(
    &self,
    filter: BoardFileFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BoardFileListItem>, u64), SumidaError> {
    use entity::bps::board_files::Column;

    let db = self.state.db();

    // Apply the board_file read ReBAC filter via self.select(...).
    // Without this, board_file:read permission alone would let one read attachments across all organizations.
    let mut query = conditions.apply::<BoardFileFilter, _>(
      filter.apply(util::with_file_columns(
        self
          .select(BoardFiles::find())?
          .join(JoinType::InnerJoin, entity::bps::board_files::Relation::File.def())
          .select_only()
          .column_as(Column::FileUuid, "id")
          .column_as(Column::PostId, "post_id"),
      )),
    )?;

    let count = query.clone().count(db).await?;

    query = pagination.order_by_schema::<BoardFileFilter, _>(query);
    query = pagination.paginate(query);

    let mut items = query.into_model::<BoardFileListItem>().all(db).await?;
    for item in &mut items {
      item.file.uri = Some(crate::service::fs::dto::inline_uri(item.file.uuid));
    }

    Ok((items, count))
  }

  pub async fn get(&self, post_id: i32, file_uuid: Uuid) -> Result<Option<GetBoardFile>, SumidaError> {
    use entity::bps::board_files::Column;
    use entity::fs::files::Column as FileColumn;

    let db = self.state.db();

    let result = util::with_file_columns(
      self
        .select(BoardFiles::find())?
        .filter(Column::PostId.eq(post_id))
        .filter(Column::FileUuid.eq(file_uuid))
        .join(JoinType::InnerJoin, entity::bps::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::PostId, "post_id")
        .column_as(Column::FileUuid, "file_uuid")
        .column_as(FileColumn::Hash, "hash"),
    )
    .into_model::<GetBoardFile>()
    .one(db)
    .await?;

    Ok(result.map(|mut r| {
      r.file.uri = Some(crate::service::fs::dto::inline_uri(r.file.uuid));
      r
    }))
  }

  pub async fn upload(&self, data: UploadBoardFile) -> Result<FileUploaded, SumidaError> {
    let db = self.state.db();

    let Some(board) = Boards::find_by_id(data.board_id).one(db).await? else {
      return Err(SumidaError::NoData("Board".to_string(), data.board_id.to_string()));
    };

    FileValidator::from_board_id(board.id, db)
      .await?
      .validate_upload_enabled()?
      .validate_extension(data.file.get_extension().as_deref())?
      .validate_size(data.file.bytes.len())?
      .validate_mime_type(&data.file.mime_type)?
      .validate_content(&data.file.bytes)?;

    // Persist the file, then register it as a *pending* board claim (post_id = NULL) with a TTL.
    // An upload never attached to a post is later collected by the orphan reaper.
    let result = FileService::new(self.state.clone(), self.session.clone()).upload(data.into()).await?;

    let txn = db.begin().await?;
    crate::util::file_lifecycle::register_pending_board_file(result.uuid, &txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, data: DeleteBoardFile) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;

    // Row-level permission check: reject if there is no delete permission for that post's attachment.
    // Without this check, board_file:delete permission alone would let one delete any post's attachment (and the original file).
    // `board_files` 의 기본 키는 `file_uuid` 하나이고 정의도 그 열을 키로 삼으므로, 종전의
    // 두 열 필터는 첨부 한 건을 정확히 같은 집합으로 지목한다. `post_id` 는 아래 DELETE 에 그대로 남는다.
    Authz::try_check(RESOURCE, data.file_uuid, "remove", &self.subject(), &txn).await?;

    let result = BoardFiles::delete_many()
      .filter(BoardFilesColumn::PostId.eq(data.post_id))
      .filter(BoardFilesColumn::FileUuid.eq(data.file_uuid))
      .exec(&txn)
      .await?;

    txn.commit().await?;

    FileService::new(self.state.clone(), self.session.clone())
      .delete(data.file_uuid, DeleteFile { hard: data.hard })
      .await?;

    Ok(result)
  }

  pub async fn get_files_for_post(&self, post_id: i32) -> Result<Vec<BoardFileListItem>, SumidaError> {
    use entity::bps::board_files::Column;

    let db = self.state.db();

    let items = util::with_file_columns(
      BoardFiles::find()
        .filter(Column::PostId.eq(post_id))
        .join(JoinType::InnerJoin, entity::bps::board_files::Relation::File.def())
        .select_only()
        .column_as(Column::FileUuid, "uuid")
        .column_as(Column::PostId, "post_id"),
    )
    .into_model::<BoardFileListItem>()
    .all(db)
    .await?;

    let mut items = items;
    for item in &mut items {
      item.file.uri = Some(crate::service::fs::dto::inline_uri(item.file.uuid));
    }

    Ok(items)
  }

  pub async fn get_posts_for_file(&self, file_uuid: Uuid) -> Result<Vec<i32>, SumidaError> {
    use entity::bps::board_files::Column;

    let db = self.state.db();

    let post_ids = BoardFiles::find()
      .filter(Column::FileUuid.eq(file_uuid))
      .select_only()
      .column(Column::PostId)
      .into_tuple::<i32>()
      .all(db)
      .await?;

    Ok(post_ids)
  }
}

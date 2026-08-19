pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{
  ColumnTrait, DeleteResult, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, QuerySelect,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::prelude::BoardPostService;
use crate::util::filter::{ConditionQuery, listed};
use crate::util::pagination::Pagination;

type ReportEntity = entity::bps::board_post_reports::Entity;
use entity::bps::board_post_reports::Column as ReportColumn;

pub struct BoardPostReportService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostReportService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Moderator listing: the route gate `board_post_report:read` is the whole access decision, and
  /// the declared keys narrow inside it. The sort key resolves through `BoardPostReportFilter` 's
  /// own declaration.
  pub async fn list(
    &self,
    filter: BoardPostReportFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostReportListItem>, u64), SumidaError> {
    let db = self.state.db();

    let mut query = ReportEntity::find();
    if let Some(post_id) = listed(filter.post_id.as_ref()) {
      query = query.filter(post_id.into_simple_expr(ReportColumn::PostId));
    }
    if let Some(user_id) = listed(filter.user_id.as_ref()) {
      query = query.filter(user_id.into_simple_expr(ReportColumn::UserId));
    }

    // Last, and only ever as one more `AND`.
    let mut query = conditions.apply::<BoardPostReportFilter, _>(query)?;

    let count = query.clone().select_only().column(ReportColumn::Id).count(db).await?;

    query = pagination.order_by_schema::<BoardPostReportFilter, _>(query);
    query = pagination.paginate(query);

    let items = query.into_model::<BoardPostReportListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPostReport>, SumidaError> {
    Ok(
      ReportEntity::find_by_id(id)
        .into_model::<GetBoardPostReport>()
        .one(self.state.db())
        .await?,
    )
  }

  /// File a report. Requires authentication and read access to the reported post. One report per
  /// (post, user): a repeat report updates the reason rather than stacking duplicates.
  pub async fn create(&self, data: CreateBoardPostReport) -> Result<i32, SumidaError> {
    if !self.session.profile.is_authorized {
      return Err(SumidaError::NoCredentials);
    }
    let user_id = self.session.profile.id;

    let txn = self.state.db().begin().await?;

    // The post must exist and be readable by the reporter.
    let post = BoardPosts::find_by_id(data.post_id)
      .filter(entity::bps::board_posts::Column::DeletedAt.is_null())
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("BoardPost".into(), data.post_id.to_string()))?;
    BoardPostService::new(self.state.clone(), self.session.clone())
      .guard_post_read(&post, None, &txn)
      .await?;

    let id = if let Some(existing) = ReportEntity::find()
      .filter(ReportColumn::PostId.eq(data.post_id))
      .filter(ReportColumn::UserId.eq(user_id))
      .one(&txn)
      .await?
    {
      let report_id = existing.id;
      let mut am = existing.into_active_model();
      am.reason = Set(data.reason);
      ReportEntity::update(am).exec(&txn).await?;
      report_id
    } else {
      ReportEntity::insert(entity::bps::board_post_reports::ActiveModel {
        id: NotSet,
        post_id: Set(data.post_id),
        user_id: Set(user_id),
        reason: Set(data.reason),
        ..Default::default()
      })
      .exec(&txn)
      .await?
      .last_insert_id
    };

    txn.commit().await?;

    Ok(id)
  }

  /// Resolve/dismiss a report (moderator; gated by `board_post_report:delete`).
  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    Ok(ReportEntity::delete_by_id(id).exec(self.state.db()).await?)
  }
}

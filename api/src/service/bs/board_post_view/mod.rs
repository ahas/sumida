pub mod dto;

use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ActiveValue::*, DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect,
  TransactionTrait,
};

use crate::SumidaState;
use crate::service::ApiService;
use crate::service::prelude::BoardPostService;
use crate::util::ap::{ApChain, ApGuard};
use crate::util::pagination::Pagination;

use self::dto::*;

pub struct BoardPostViewService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostViewService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardPostViewFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostViewListItem>, u64), SumidaError> {
    use entity::bs::board_post_views::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(entity::bs::board_post_views::Entity::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "createdAt" => Column::CreatedAt,
      _ => Column::CreatedAt,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<BoardPostViewListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPostView>, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("read", entity::bs::board_post_views::Entity::find_by_id(id), &txn).await?;

    let view = entity::bs::board_post_views::Entity::find_by_id(id)
      .into_model::<GetBoardPostView>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(view)
  }

  pub async fn create(
    &self,
    data: CreateBoardPostView,
  ) -> Result<InsertResult<entity::bs::board_post_views::ActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let result = entity::bs::board_post_views::Entity::insert(data.into_active_model()).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPostView,
  ) -> Result<entity::bs::board_post_views::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("update", entity::bs::board_post_views::Entity::find_by_id(id), &txn).await?;

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = entity::bs::board_post_views::Entity::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self.try_access("delete", entity::bs::board_post_views::Entity::find_by_id(id), &txn).await?;

    let result = entity::bs::board_post_views::Entity::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApChain, entity::bs::board_post_views::Entity> for BoardPostViewService<S> {
  fn ap(&self) -> ApChain {
    BoardPostService::new(self.state.clone(), self.session.clone()).ap()
  }

  fn select(
    &self,
    query: sea_orm::Select<entity::bs::board_post_views::Entity>,
  ) -> sea_orm::Select<entity::bs::board_post_views::Entity> {
    self.ap().action("read").apply(query)
  }
}

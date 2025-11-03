pub mod dto;

use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::*;
use sea_orm::{
  DeleteResult, EntityTrait, InsertResult, IntoActiveModel, PaginatorTrait, QuerySelect, TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::ApiService;
use crate::service::prelude::BoardPostService;
use crate::util::ap::{ApChain, ApGuard};
use crate::util::pagination::Pagination;

pub struct BoardPostReactionService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> BoardPostReactionService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn list(
    &self,
    filter: BoardPostReactionFilter,
    pagination: Pagination,
  ) -> Result<(Vec<BoardPostReactionListItem>, u64), SumidaError> {
    use entity::bs::board_post_reactions::Column;

    let db = self.state.db();
    let mut query = filter.apply(self.select(entity::bs::board_post_reactions::Entity::find()));
    let count = query.clone().select_only().column(Column::Id).count(db).await?;

    query = pagination.order_by(query, |col| match col {
      "value" => Column::Value,
      "createdAt" => Column::CreatedAt,
      _ => Column::CreatedAt,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<BoardPostReactionListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i32) -> Result<Option<GetBoardPostReaction>, SumidaError> {
    let txn = self.state.db().begin().await?;
    self
      .try_access("read", entity::bs::board_post_reactions::Entity::find_by_id(id), &txn)
      .await?;

    let reaction = entity::bs::board_post_reactions::Entity::find_by_id(id)
      .into_model::<GetBoardPostReaction>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(reaction)
  }

  pub async fn create(
    &self,
    data: CreateBoardPostReaction,
  ) -> Result<InsertResult<entity::bs::board_post_reactions::ActiveModel>, SumidaError> {
    let txn = self.state.db().begin().await?;
    let result = entity::bs::board_post_reactions::Entity::insert(data.into_active_model())
      .exec(&txn)
      .await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn update(
    &self,
    id: i32,
    data: UpdateBoardPostReaction,
  ) -> Result<entity::bs::board_post_reactions::Model, SumidaError> {
    let txn = self.state.db().begin().await?;
    self
      .try_access("update", entity::bs::board_post_reactions::Entity::find_by_id(id), &txn)
      .await?;

    let mut model = data.into_active_model();
    model.id = Set(id);

    let result = entity::bs::board_post_reactions::Entity::update(model).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }

  pub async fn delete(&self, id: i32) -> Result<DeleteResult, SumidaError> {
    let txn = self.state.db().begin().await?;
    self
      .try_access("delete", entity::bs::board_post_reactions::Entity::find_by_id(id), &txn)
      .await?;

    let result = entity::bs::board_post_reactions::Entity::delete_by_id(id).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl<S: SumidaState> ApiService<ApChain, entity::bs::board_post_reactions::Entity>
  for BoardPostReactionService<S>
{
  fn ap(&self) -> ApChain {
    BoardPostService::new(self.state.clone(), self.session.clone()).ap()
  }

  fn select(
    &self,
    query: sea_orm::Select<entity::bs::board_post_reactions::Entity>,
  ) -> sea_orm::Select<entity::bs::board_post_reactions::Entity> {
    self.ap().action("read").apply(query)
  }
}

pub mod dto;

use chrono::{Duration, Utc};
use sea_orm::{ActiveValue::*, EntityTrait, TransactionTrait};
use sea_orm::{ColumnTrait, DeleteResult, PaginatorTrait, QueryFilter, QuerySelect, QueryTrait};
use uuid::Uuid;

use crate::core::db;
use crate::core::error::SumidaError;
use crate::entity::prelude::*;
use crate::util::crud::{search, Pagination};

use self::dto::*;

pub async fn list(
  filter: UserInvitationFilter,
  pagination: Pagination,
) -> Result<(Vec<UserInvitationListItem>, u64), SumidaError> {
  use crate::entity::user_invitations::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = UserInvitations::find()
    .apply_if(filter.username, |q, v| search(q, Column::Username, v))
    .apply_if(filter.uuid, |q, v| q.filter(Column::Uuid.eq(v)));
  let count = query.clone().select_only().column(Column::Uuid).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "username" => Column::Username,
    "uuid" => Column::Uuid,
    _ => Column::CreatedAt,
  });
  let query = pagination.paginate(query);
  let items = query.into_model::<UserInvitationListItem>().all(&txn).await?;

  txn.commit().await?;

  Ok((items, count))
}

pub async fn get(uuid: Uuid) -> Result<Option<GetUserInvitation>, SumidaError> {
  let db_conn = db::get_connection();

  Ok(
    UserInvitations::find_by_id(uuid)
      .filter(crate::entity::user_invitations::Column::ExpiredAt.gt(Utc::now()))
      .into_model::<GetUserInvitation>()
      .one(db_conn)
      .await?,
  )
}

pub async fn create(username: String) -> Result<crate::entity::user_invitations::Model, SumidaError> {
  let db_conn = db::get_connection();
  let uuid = Uuid::new_v4();
  let expired_at = Utc::now() + Duration::days(30);

  let active_model = crate::entity::user_invitations::ActiveModel {
    username: Set(username.clone()),
    uuid: Set(uuid.clone()),
    expired_at: Set(expired_at.fixed_offset()),
    ..Default::default()
  };

  UserInvitations::insert(active_model).exec(db_conn).await?;

  Ok(UserInvitations::find_by_id(uuid).one(db_conn).await?.unwrap())
}

pub async fn update(username: String) -> Result<crate::entity::user_invitations::Model, SumidaError> {
  let db_conn = db::get_connection();
  let uuid = Uuid::new_v4();
  let expired_at = Utc::now() + Duration::days(30);

  let active_model = crate::entity::user_invitations::ActiveModel {
    username: Unchanged(username.clone()),
    uuid: Set(uuid.clone()),
    expired_at: Set(expired_at.fixed_offset()),
    ..Default::default()
  };

  UserInvitations::update(active_model).exec(db_conn).await?;

  Ok(UserInvitations::find_by_id(uuid).one(db_conn).await?.unwrap())
}

pub async fn delete(uuid: Uuid) -> Result<DeleteResult, SumidaError> {
  let db_conn = db::get_connection();
  Ok(UserInvitations::delete_by_id(uuid).exec(db_conn).await?)
}

pub mod dto;

use chrono::{Duration, Utc};
use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{ActiveValue::*, EntityTrait, TransactionTrait};
use sea_orm::{ColumnTrait, DeleteResult, PaginatorTrait, QueryFilter, QuerySelect, QueryTrait};
use uuid::Uuid;

use crate::util::crud::{Pagination, wrap_ilike_col};

use self::dto::*;

pub async fn list(
  filter: UserInvitationFilter,
  pagination: Pagination,
) -> Result<(Vec<UserInvitationListItem>, u64), SumidaError> {
  use entity::user_invitations::Column;

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let query = UserInvitations::find()
    .apply_if(filter.user, |q, v| q.filter(wrap_ilike_col(Column::User, v)))
    .apply_if(filter.uuid, |q, v| q.filter(Column::Uuid.eq(v)));
  let count = query.clone().select_only().column(Column::Uuid).count(&txn).await?;
  let query = pagination.order_by(query, |col| match col {
    "user" => Column::User,
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
      .filter(entity::user_invitations::Column::ExpiredAt.gt(Utc::now()))
      .into_model::<GetUserInvitation>()
      .one(db_conn)
      .await?,
  )
}

pub async fn create(username: String) -> Result<entity::user_invitations::Model, SumidaError> {
  let db_conn = db::get_connection();
  let uuid = Uuid::new_v4();
  let expired_at = Utc::now() + Duration::days(30);

  let active_model = entity::user_invitations::ActiveModel {
    user: Set(username.clone()),
    uuid: Set(uuid),
    expired_at: Set(expired_at.fixed_offset()),
    ..Default::default()
  };

  UserInvitations::insert(active_model).exec(db_conn).await?;

  UserInvitations::find_by_id(uuid)
    .one(db_conn)
    .await?
    .ok_or(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
}

pub async fn update(username: String) -> Result<entity::user_invitations::Model, SumidaError> {
  let db_conn = db::get_connection();
  let uuid = Uuid::new_v4();
  let expired_at = Utc::now() + Duration::days(30);

  let active_model = entity::user_invitations::ActiveModel {
    user: Unchanged(username.clone()),
    uuid: Set(uuid),
    expired_at: Set(expired_at.fixed_offset()),
    ..Default::default()
  };

  UserInvitations::update(active_model).exec(db_conn).await?;

  UserInvitations::find_by_id(uuid)
    .one(db_conn)
    .await?
    .ok_or(SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
}

pub async fn delete(uuid: Uuid) -> Result<DeleteResult, SumidaError> {
  let db_conn = db::get_connection();
  Ok(UserInvitations::delete_by_id(uuid).exec(db_conn).await?)
}

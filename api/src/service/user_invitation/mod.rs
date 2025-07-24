pub mod dto;

use chrono::{Duration, Utc};
use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{ActiveValue::*, DbErr, EntityTrait, TransactionTrait};
use sea_orm::{ColumnTrait, ConnectionTrait, DeleteResult, PaginatorTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserInvitationService;

impl UserInvitationService {
  pub async fn upsert<C>(user_id: i32, uuid: Uuid, db: &C) -> Result<(), DbErr>
  where
    C: ConnectionTrait,
  {
    let active_model = entity::user_invitations::ActiveModel {
      user_id: Set(user_id),
      uuid: Set(uuid),
      expires_at: Set((Utc::now() + Duration::days(30)).fixed_offset()),
      ..Default::default()
    };

    let exists = UserInvitations::find_by_id(uuid).count(db).await? > 0;

    if exists {
      UserInvitations::update(active_model).exec(db).await?;
    } else {
      UserInvitations::insert(active_model).exec(db).await?;
    }

    Ok(())
  }

  pub async fn list(
    filter: UserInvitationFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserInvitationListItem>, u64), SumidaError> {
    use entity::user_invitations::Column;

    let db_conn = db::get_connection();
    let txn = db_conn.begin().await?;

    let mut query = filter.apply(UserInvitations::find());
    let count = query.clone().select_only().column(Column::Uuid).count(&txn).await?;

    query = pagination.order_by(query, |col| match col {
      "user_id" => Column::UserId,
      "uuid" => Column::Uuid,
      _ => Column::CreatedAt,
    });
    query = pagination.paginate(query);

    let items = query.into_model::<UserInvitationListItem>().all(&txn).await?;

    txn.commit().await?;

    Ok((items, count))
  }

  pub async fn get(uuid: Uuid) -> Result<Option<GetUserInvitation>, SumidaError> {
    let db_conn = db::get_connection();

    Ok(
      UserInvitations::find_by_id(uuid)
        .filter(entity::user_invitations::Column::ExpiresAt.gt(Utc::now()))
        .into_model::<GetUserInvitation>()
        .one(db_conn)
        .await?,
    )
  }

  pub async fn create(user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let db_conn = db::get_connection();
    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Set(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at.fixed_offset()),
      ..Default::default()
    };

    UserInvitations::insert(active_model).exec(db_conn).await?;

    UserInvitations::find_by_id(uuid)
      .one(db_conn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
  }

  pub async fn update(user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let db_conn = db::get_connection();
    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Unchanged(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at.fixed_offset()),
      ..Default::default()
    };

    UserInvitations::update(active_model).exec(db_conn).await?;

    UserInvitations::find_by_id(uuid)
      .one(db_conn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()))
  }

  pub async fn delete(uuid: Uuid) -> Result<DeleteResult, SumidaError> {
    let db_conn = db::get_connection();
    Ok(UserInvitations::delete_by_id(uuid).exec(db_conn).await?)
  }
}

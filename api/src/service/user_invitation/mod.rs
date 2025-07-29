pub mod dto;

use chrono::{Duration, Utc};
use infra::db;
use infra::session::UserContext;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveValue::*, DbErr, EntityTrait, Select, TransactionTrait};
use sea_orm::{ColumnTrait, ConnectionTrait, DeleteResult, PaginatorTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use crate::service::ApiService;
use crate::util::ap::{Ap, ApGuard};
use crate::util::crud::Pagination;

use self::dto::*;

pub struct UserInvitationService {
  user_ctx: UserContext,
}

impl UserInvitationService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

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
    &self,
    filter: UserInvitationFilter,
    pagination: Pagination,
  ) -> Result<(Vec<UserInvitationListItem>, u64), SumidaError> {
    use entity::user_invitations::Column;

    let txn = db::get_connection().begin().await?;

    let mut query = filter.apply(self.select(UserInvitations::find()));
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

  pub async fn get(&self, uuid: Uuid) -> Result<Option<GetUserInvitation>, SumidaError> {
    let txn = db::get_connection().begin().await?;
    let item = self
      .select(UserInvitations::find_by_id(uuid))
      .filter(entity::user_invitations::Column::ExpiresAt.gt(Utc::now()))
      .into_model::<GetUserInvitation>()
      .one(&txn)
      .await?;
    txn.commit().await?;

    Ok(item)
  }

  pub async fn create(&self, user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;

    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Set(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at.fixed_offset()),
      ..Default::default()
    };

    UserInvitations::insert(active_model).exec(&txn).await?;
    let result = UserInvitations::find_by_id(uuid)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));

    txn.commit().await?;

    result
  }

  pub async fn update(&self, user_id: i32) -> Result<entity::user_invitations::Model, SumidaError> {
    let txn = db::get_connection().begin().await?;

    self
      .ap()
      .action("read")
      .check(UserInvitations::find().filter(entity::user_invitations::Column::UserId.eq(user_id)), &txn)
      .await?;

    let uuid = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(30);

    let active_model = entity::user_invitations::ActiveModel {
      user_id: Unchanged(user_id),
      uuid: Set(uuid),
      expires_at: Set(expires_at.fixed_offset()),
      ..Default::default()
    };

    UserInvitations::update(active_model).exec(&txn).await?;

    let result = UserInvitations::find_by_id(uuid)
      .one(&txn)
      .await?
      .ok_or_else(|| SumidaError::NoData("UserInvitation".to_owned(), uuid.to_string()));

    txn.commit().await?;

    result
  }

  pub async fn delete(&self, uuid: Uuid) -> Result<DeleteResult, SumidaError> {
    let txn = db::get_connection().begin().await?;
    self.ap().action("delete").check(UserInvitations::find_by_id(uuid), &txn).await?;

    let result = UserInvitations::delete_by_id(uuid).exec(&txn).await?;
    txn.commit().await?;

    Ok(result)
  }
}

impl ApiService<Ap, UserInvitations> for UserInvitationService {
  fn select(&self, query: Select<UserInvitations>) -> Select<UserInvitations> {
    self.ap().action("read").apply(query)
  }

  fn ap(&self) -> Ap {
    use entity::user_invitations::Column;

    Ap::new("user_invitation")
      .owner(&self.user_ctx)
      .object(Expr::col((UserInvitations, Column::Uuid)).cast_as("varchar"))
  }
}

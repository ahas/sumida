pub mod dto;

use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{
  ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set, TransactionTrait,
};

use crate::core::db;
use crate::core::error::SumidaError;
use crate::entity::prelude::*;
use crate::service::user::mail_form_context::*;

use self::dto::*;

pub async fn send(data: SendUserVerification) -> Result<(), SumidaError> {
  use crate::entity::user_verifications::Column;

  let code: String = {
    let mut rng = rand::rng();
    (0..6).map(|_| rng.random_range(0..10).to_string()).collect()
  };

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let username = data.username.clone();
  let kind = data.kind.clone();
  let method = data.method.clone();

  let mut active_model = data.into_active_model();
  active_model.username = Set(username.clone());
  active_model.kind = Set(kind.clone());
  active_model.code = Set(code.clone());
  active_model.method = Set(method.clone());
  active_model.expired_at = Set((Utc::now() + Duration::minutes(3)).fixed_offset());

  let exists = UserVerifications::find()
    .filter(Column::Username.eq(&username))
    .filter(Column::Kind.eq(&kind))
    .count(db_conn)
    .await?
    > 0;

  if exists {
    UserVerifications::update(active_model).exec(&txn).await?;
  } else {
    UserVerifications::insert(active_model).exec(&txn).await?;
  }

  let Some(user) = Users::find_by_id(&username).one(&txn).await? else {
    txn.rollback().await?;
    return Err(SumidaError::NoUserAccount(username));
  };

  match kind.as_str() {
    "email" => {
      let Some(email) = user.email else {
        txn.rollback().await?;
        return Err(SumidaError::NoUserEmail(user.username));
      };

      MailForms::send("verification", &email, VerificationContext { code }).await?;
    }
    _ => {}
  }

  txn.commit().await?;

  Ok(())
}

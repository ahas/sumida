pub mod dto;

use chrono::{Duration, Utc};
use core::{db, settings};
use entity::{mail_forms::MailFormSendOptions, prelude::*};
use error::SumidaError;
use rand::Rng;
use sea_orm::{
  ColumnTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set, TransactionTrait,
};

use crate::service::user::mail_form_context::*;

use self::dto::*;

pub async fn send(data: SendUserVerification) -> Result<(), SumidaError> {
  use entity::user_verifications::Column;

  let code: String = {
    let mut rng = rand::rng();
    (0..6).map(|_| rng.random_range(0..10).to_string()).collect()
  };

  let db_conn = db::get_connection();
  let txn = db_conn.begin().await?;

  let username = data.user.clone();
  let kind = data.kind.clone();
  let method = data.method.clone();

  let mut active_model = data.into_active_model();
  active_model.user = Set(username.clone());
  active_model.kind = Set(kind.clone());
  active_model.code = Set(code.clone());
  active_model.method = Set(method.clone());
  active_model.expired_at = Set((Utc::now() + Duration::minutes(3)).fixed_offset());

  let exists = UserVerifications::find()
    .filter(Column::User.eq(&username))
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
    return Err(SumidaError::InvalidAccount);
  };

  if method == "email" {
    let Some(email) = user.email else {
      txn.rollback().await?;
      return Err(SumidaError::NoUserEmail(user.username));
    };
    let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
      username: opts.username.clone(),
      password: opts.password.clone(),
      server: opts.server.clone(),
      auth: opts.auth.clone(),
    });

    MailForms::send("verification", &email, VerificationContext { code }, opts, db::get_connection())
      .await?;
  }

  txn.commit().await?;

  Ok(())
}

pub mod dto;

use chrono::{Duration, Utc};
use infra::{db, session::UserContext, settings};
use entity::prelude::*;
use error::SumidaError;
use rand::Rng;
use sea_orm::{
  ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set,
  TransactionTrait,
};

use self::dto::*;

use crate::service::mail_form::MailFormSendOptions;
use crate::service::prelude::MailFormService;
use crate::service::user::mail_form_context::*;

pub struct UserVerificationService {
  pub user_ctx: UserContext,
}

impl UserVerificationService {
  pub fn new(user_ctx: UserContext) -> Self {
    Self { user_ctx }
  }

  pub async fn verify<'db, KindT, CodeT, DbConnT>(
    &self,
    kind: KindT,
    user_id: i32,
    code: CodeT,
    txn: &'db DbConnT,
  ) -> Result<bool, SumidaError>
  where
    KindT: AsRef<str>,
    CodeT: AsRef<str>,
    DbConnT: ConnectionTrait,
  {
    use entity::user_verifications::{ActiveModel, Column, Entity};

    let kind = kind.as_ref();
    let code = code.as_ref();

    let Some(_) = Users::find_by_id(user_id).one(txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    let is_verified = Entity::find()
      .filter(Column::Kind.eq(kind))
      .filter(Column::UserId.eq(user_id))
      .filter(Column::Code.eq(code))
      .count(txn)
      .await?
      > 0;

    if is_verified {
      UserVerifications::delete(ActiveModel {
        kind: Set(kind.to_owned()),
        user_id: Set(user_id),
        code: Set(code.to_owned()),
        ..Default::default()
      })
      .exec(txn)
      .await?;
    }

    Ok(is_verified)
  }

  pub async fn send(&self, data: SendUserVerification) -> Result<(), SumidaError> {
    use entity::user_verifications::Column;

    let code: String = {
      let mut rng = rand::rng();
      (0..6).map(|_| rng.random_range(0..10).to_string()).collect()
    };

    let txn = db::get_connection().begin().await?;

    let user_id = data.user_id;
    let kind = data.kind.clone();
    let method = data.method.clone();

    let mut active_model = data.into_active_model();
    active_model.user_id = Set(user_id);
    active_model.kind = Set(kind.clone());
    active_model.code = Set(code.clone());
    active_model.method = Set(method.clone());
    active_model.expires_at = Set((Utc::now() + Duration::minutes(3)).fixed_offset());

    let exists = UserVerifications::find()
      .filter(Column::UserId.eq(user_id))
      .filter(Column::Kind.eq(&kind))
      .count(&txn)
      .await?
      > 0;

    if exists {
      UserVerifications::update(active_model).exec(&txn).await?;
    } else {
      UserVerifications::insert(active_model).exec(&txn).await?;
    }

    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      txn.rollback().await?;
      return Err(SumidaError::InvalidAccount);
    };

    if method == "email" {
      let Some(email) = user.email else {
        txn.rollback().await?;
        return Err(SumidaError::NoUserEmail(user.id));
      };
      let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
        username: opts.username.clone(),
        password: opts.password.clone(),
        server: opts.server.clone(),
        auth: opts.auth.clone(),
      });

      MailFormService::send(
        "verification",
        &email,
        VerificationContext { code },
        opts,
        db::get_connection(),
      )
      .await?;
    }

    txn.commit().await?;

    Ok(())
  }
}

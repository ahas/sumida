pub mod dto;

use chrono::{Duration, Utc};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use rand::RngExt;
use sea_orm::{
  ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
use crate::service::mail_form::MailFormSendOptions;
use crate::service::prelude::MailFormService;
use crate::service::user::mail_form_context::*;

pub struct UserVerificationService<S: SumidaState> {
  pub state: S,
  pub session: UserSession,
}

impl<S: SumidaState> UserVerificationService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  pub async fn verify<KindT, CodeT, DbConnT>(
    &self,
    kind: KindT,
    user_id: i32,
    code: CodeT,
    txn: &DbConnT,
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
    // Reject expired codes. Without this filter, codes would stay valid forever,
    // exposing them to an unbounded 6-digit (10^6) brute-force with no time constraint.
    let is_verified = Entity::find()
      .filter(Column::Kind.eq(kind))
      .filter(Column::UserId.eq(user_id))
      .filter(Column::Code.eq(code))
      .filter(Column::ExpiresAt.gt(Utc::now()))
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

    let txn = self.state.db().begin().await?;

    let user_id = data.user_id;
    let kind = data.kind.clone();
    let method = data.method.clone();

    // Do not reveal account existence in the response (enumeration prevention).
    // If there is no target, silently succeed without sending a code.
    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      txn.rollback().await?;
      return Ok(());
    };

    // Email bomb prevention: do not resend if a still-valid code remains (3-minute cooldown per user).
    let has_active_code = UserVerifications::find()
      .filter(Column::UserId.eq(user_id))
      .filter(Column::Kind.eq(&kind))
      .filter(Column::ExpiresAt.gt(Utc::now()))
      .count(&txn)
      .await?
      > 0;
    if has_active_code {
      txn.rollback().await?;
      return Ok(());
    }

    let code: String = {
      let mut rng = rand::rng();
      (0..6).map(|_| rng.random_range(0..10).to_string()).collect()
    };

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

    if method == "email" {
      // Do not reveal whether an email is on file either (enumeration prevention). Silently succeed.
      let Some(email) = user.email else {
        txn.rollback().await?;
        return Ok(());
      };
      let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
        username: opts.username.clone(),
        password: opts.password.clone(),
        server: opts.server.clone(),
        auth: opts.auth.clone(),
        port: opts.resolved_port(),
        tls: opts.tls,
      });

      MailFormService::<S>::send(
        "verification",
        &email,
        VerificationContext { code },
        opts,
        self.state.db(),
      )
      .await?;
    }

    txn.commit().await?;

    Ok(())
  }
}

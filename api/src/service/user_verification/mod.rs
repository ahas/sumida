pub mod dto;

use std::sync::LazyLock;

use chrono::{Duration, Utc};
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use rand::RngExt;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
  ColumnTrait,
  ConnectionTrait,
  EntityTrait,
  IntoActiveModel,
  PaginatorTrait,
  QueryFilter,
  Set,
  TransactionTrait,
};

use self::dto::*;
use crate::SumidaState;
#[cfg(feature = "mail-form")]
use crate::service::mail_form::MailFormSendOptions;
#[cfg(feature = "mail-form")]
use crate::service::prelude::MailFormService;
#[cfg(feature = "mail-form")]
use crate::service::user::mail_form_context::*;
use crate::util::throttle::Throttle;

/// How long an issued code stays good.
const CODE_TTL_MINUTES: i64 = 3;

/// The only channel a code can be delivered on. A code for any other is never stored: a code that
/// reaches nobody is one only a guesser can ever spend.
pub const DELIVERABLE_METHOD: &str = "email";

/// Codes asked for, per account and per address.
static ISSUED: LazyLock<Throttle> = LazyLock::new(Throttle::new);
/// Codes presented, per account and per address.
static PRESENTED: LazyLock<Throttle> = LazyLock::new(Throttle::new);
/// Guesses presented against one issued code.
static MISSED: LazyLock<Throttle> = LazyLock::new(Throttle::new);

/// One issued code's own count of presentations. The moment it expires is part of the key, so a
/// code issued anew starts from nothing even on an instance that never saw it issued.
fn code_key(user_id: i32, kind: &str, expires_at: &DateTimeUtc) -> String {
  format!("{user_id}:{kind}:{}", expires_at.timestamp_micros())
}

/// Whether `presented` is the stored code, compared in time that does not depend on where the two
/// first differ.
fn same_code(stored: &str, presented: &str) -> bool {
  stored.len() == presented.len()
    && stored.bytes().zip(presented.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

pub struct UserVerificationService<S: SumidaState> {
  pub state:   S,
  pub session: UserSession,
}

impl<S: SumidaState> UserVerificationService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Counts one more use of a limited door, per account and per address, and refuses once either
  /// count is past `limit`. The account is counted whether or not it exists, so the refusal says
  /// nothing about which accounts do.
  fn within(
    throttle: &Throttle,
    door: &str,
    user_id: i32,
    ip: &str,
    limit: u32,
  ) -> Result<(), SumidaError> {
    let window = settings::get().security.verification.window().to_std().unwrap_or_default();
    let by_account = throttle.admit(&format!("{door}:user:{user_id}"), limit, window);
    let by_address = throttle.admit(&format!("{door}:ip:{ip}"), limit, window);

    if by_account && by_address {
      Ok(())
    } else {
      Err(SumidaError::Busy("too many verification requests; try again later".to_owned()))
    }
  }

  /// Spends a code of `kind` issued to `user_id`, answering whether `code` was it.
  ///
  /// One code may be presented `security.verification.max_failed_attempts` times: the presentation
  /// that reaches that number spends it, right or wrong, so a six-digit code cannot be searched.
  /// Presenting codes at all is limited per account and per address (`attempt_limit` per `window`),
  /// and past that the answer is `Busy` rather than a judgement.
  ///
  /// The count is kept in this process and the spent code is deleted on a connection of its own,
  /// not on `txn`: a caller that rolls its transaction back on a wrong code must not roll the
  /// spending back with it.
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
    use entity::user_verifications::{Column, Entity};

    let kind = kind.as_ref();
    let code = code.as_ref();
    let limits = &settings::get().security.verification;

    Self::within(&PRESENTED, "present", user_id, &self.session.env.ip_address, limits.attempt_limit)?;

    let Some(_) = Users::find_by_id(user_id).one(txn).await? else {
      return Err(SumidaError::InvalidAccount);
    };
    // Reject expired codes. Without this filter, codes would stay valid forever,
    // exposing them to an unbounded 6-digit (10^6) brute-force with no time constraint.
    let Some(issued) = Entity::find()
      .filter(Column::Kind.eq(kind))
      .filter(Column::UserId.eq(user_id))
      .filter(Column::ExpiresAt.gt(Utc::now()))
      .one(txn)
      .await?
    else {
      return Ok(false);
    };

    let key = code_key(user_id, kind, &issued.expires_at);
    let attempt = MISSED.take(&key, Duration::minutes(CODE_TTL_MINUTES).to_std().unwrap_or_default());
    let allowed = limits.max_failed_attempts.max(1);
    let matched = attempt <= allowed && same_code(&issued.code, code);

    // A match spends the code; so does the guess that used up its last chance, right or wrong.
    if matched || attempt >= allowed {
      Self::discard(user_id, kind, &issued.code, self.state.db()).await?;
      MISSED.forget(&key);
    }

    Ok(matched)
  }

  async fn discard<C: ConnectionTrait>(
    user_id: i32,
    kind: &str,
    code: &str,
    db: &C,
  ) -> Result<(), SumidaError> {
    use entity::user_verifications::Column;

    UserVerifications::delete_many()
      .filter(Column::UserId.eq(user_id))
      .filter(Column::Kind.eq(kind))
      .filter(Column::Code.eq(code))
      .exec(db)
      .await?;

    Ok(())
  }

  pub async fn send(&self, data: SendUserVerification) -> Result<(), SumidaError> {
    use entity::user_verifications::Column;

    let user_id = data.user_id;
    let kind = data.kind.clone();
    let method = data.method.clone();

    let limits = &settings::get().security.verification;
    Self::within(&ISSUED, "issue", user_id, &self.session.env.ip_address, limits.issue_limit)?;

    // A code is stored only where it can be delivered. One stored for a channel that sends nothing
    // reaches nobody but a guesser — and its owner, never told, has no reason to change anything.
    if method != DELIVERABLE_METHOD || !cfg!(feature = "mail-form") {
      return Ok(());
    }

    let txn = self.state.db().begin().await?;

    // Do not reveal account existence in the response (enumeration prevention).
    // If there is no target, silently succeed without sending a code.
    let Some(user) = Users::find_by_id(user_id).one(&txn).await? else {
      txn.rollback().await?;
      return Ok(());
    };
    // Do not reveal whether an email is on file either (enumeration prevention). Silently succeed.
    #[cfg_attr(not(feature = "mail-form"), allow(unused_variables))]
    let Some(email) = user.email else {
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
    active_model.expires_at = Set(Utc::now() + Duration::minutes(CODE_TTL_MINUTES));

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

    // The code is stored only together with the mail that carries it: a send that fails rolls the
    // code back with it.
    #[cfg(feature = "mail-form")]
    {
      let opts = settings::get().smtp.as_ref().map(|opts| MailFormSendOptions {
        username: opts.username.clone(),
        password: opts.password.clone(),
        server:   opts.server.clone(),
        auth:     opts.auth.clone(),
        port:     opts.resolved_port(),
        tls:      opts.tls,
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

#[cfg(test)]
mod tests {
  use super::same_code;

  #[test]
  fn the_same_code_matches_and_any_other_does_not() {
    assert!(same_code("123456", "123456"));
    assert!(!same_code("123456", "123457"));
    assert!(!same_code("123456", "12345"));
    assert!(!same_code("123456", ""));
  }
}

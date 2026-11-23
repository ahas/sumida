pub mod coupon;
pub mod credit;
pub mod entitlement;
pub mod maintenance;
pub mod payment;
pub mod plan;
pub mod purchase;
pub mod quota;
pub mod subscription;

use entity::membership::OwnerKind;
use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use serde::Serialize;

use crate::SumidaState;
use crate::authz::Authz;
use crate::authz::subject::SubjectSet;
use crate::authz::write::SubjectKey;
use crate::service::mail_form::{MailFormSendOptions, MailFormService};
use crate::util::random::generate_url_safe_string;

/// Generate a tracking code with a short prefix, e.g. `sub_a1B2c3...`.
pub fn gen_code(prefix: &str) -> String {
  let rand = generate_url_safe_string(12).unwrap_or_else(|_| "0000".to_string());
  format!("{prefix}_{rand}")
}

/// Owner of a personal membership = the current session user.
pub fn personal_owner(session: &UserSession) -> (OwnerKind, i32) {
  (OwnerKind::Personal, session.profile.id)
}

/// Kebab string form used for raw-SQL binds and `org:<resource>` object ids.
pub fn owner_kind_str(kind: &OwnerKind) -> &'static str {
  match kind {
    OwnerKind::Personal => "personal",
    OwnerKind::Organization => "organization",
  }
}

/// Resolve the concrete (owner_kind, owner_id) for a request.
/// - personal: the session user, unless a platform administrator names another account.
/// - organization: the given org id, defaulting to the session user's org.
pub async fn resolve_owner<C: ConnectionTrait>(
  session: &UserSession,
  owner_kind: &OwnerKind,
  owner_id: Option<i32>,
  db: &C,
) -> Result<(OwnerKind, i32), SumidaError> {
  match owner_kind {
    // A normal user can only act for themselves; a platform administrator may target an explicit
    // user (e.g. granting credits to someone). Administration is asked of the rule, never read off
    // the session: a request on a secured API key carries its owner's `is_admin` but none of the
    // owner's authority. Org targeting is always explicit and gated separately by the org-scoped
    // authorization checks.
    OwnerKind::Personal => {
      let id = match owner_id {
        Some(id)
          if id != session.profile.id && Authz::is_superuser(&SubjectSet::of(session), db).await? =>
        {
          id
        }
        _ => session.profile.id,
      };
      Ok((OwnerKind::Personal, id))
    }
    OwnerKind::Organization => {
      Ok((OwnerKind::Organization, owner_id.unwrap_or(session.profile.organization.id)))
    }
  }
}

/// Whether `subject` may let a step go past what its source covers: a wallet capture into debt,
/// a quota capture into overage. The spender never decides that for itself, so a personal owner's
/// overrun is a platform administrator's alone, and an organization's is whoever holds
/// `permission` on it.
pub(crate) async fn authorize_overrun<C: ConnectionTrait>(
  subject: &SubjectSet,
  owner_kind: &OwnerKind,
  owner_id: i32,
  permission: &str,
  db: &C,
) -> Result<(), SumidaError> {
  if Authz::is_superuser(subject, db).await? {
    return Ok(());
  }
  match owner_kind {
    OwnerKind::Personal => Err(SumidaError::NoAccessPermission),
    OwnerKind::Organization => Authz::try_check("organization", owner_id, permission, subject, db).await,
  }
}

/// When a hold placed or extended now stops counting: `expires_in_secs` from now, or the
/// configured lifetime when the caller names none.
pub(crate) fn hold_expiry(
  expires_in_secs: Option<i64>,
) -> Result<sea_orm::prelude::DateTimeUtc, SumidaError> {
  let secs = expires_in_secs.unwrap_or(settings::get().membership.reservation_ttl_secs);
  if secs <= 0 {
    return Err(SumidaError::BadRequest("a hold's lifetime must be positive".into()));
  }
  Ok(chrono::Utc::now() + chrono::Duration::seconds(secs))
}

/// Emails to notify for an owner: the user's own email for a personal owner, or
/// the organization's managers (holders of an `org:subscription` / `org:credit`
/// allow policy on the org) for an organization owner.
pub async fn recipient_emails<C: ConnectionTrait>(
  owner_kind: &OwnerKind,
  owner_id: i32,
  db: &C,
) -> Vec<String> {
  match owner_kind {
    OwnerKind::Personal => Users::find_by_id(owner_id)
      .one(db)
      .await
      .ok()
      .flatten()
      .and_then(|u| u.email)
      .into_iter()
      .collect(),
    OwnerKind::Organization => org_manager_emails(owner_id, db).await.unwrap_or_default(),
  }
}

/// Emails of the users who manage organization `org_id`'s membership.
///
/// 관계 자체를 되짚어 읽고(`subjects_with`) 주체를 사용자 id 로 펼친다(`expand_to_user_ids`).
/// 주체 문자열을 여기서 손으로 파싱하면 아는 접두사만 통과시키게 되고, 모르는 종류의 수신자는
/// 아무 말 없이 사라진다 — 부여는 받았는데 알림만 못 받는 상태가 된다.
pub async fn org_manager_emails<C: ConnectionTrait>(
  org_id: i32,
  db: &C,
) -> Result<Vec<String>, SumidaError> {
  use std::collections::HashSet;

  // 세 관계는 `organization` 의 `scope subscription` / `credit_manager` / 총괄 `manager` 다.
  // 조직의 멤버십을 관리한다는 것이 이 셋 중 하나를 쥐고 있다는 뜻이다.
  let grants = Authz::subjects_with(
    "organization",
    &org_id.to_string(),
    &["subscription_manager", "credit_manager", "manager"],
    db,
  )
  .await?;

  let subjects: Vec<SubjectKey> = grants.into_iter().map(|grant| grant.subject).collect();
  let user_ids = Authz::expand_to_user_ids(&subjects, db).await?;
  if user_ids.is_empty() {
    return Ok(Vec::new());
  }

  let mut emails: HashSet<String> = HashSet::new();
  for user in Users::find().filter(entity::users::Column::Id.is_in(user_ids)).all(db).await? {
    if let Some(email) = user.email {
      emails.insert(email);
    }
  }

  Ok(emails.into_iter().collect())
}

/// Best-effort notification to every recipient (no-op on an empty list).
pub async fn notify_all<S, Ctx, C>(kind: &str, emails: &[String], context: Ctx, db: &C)
where
  S: SumidaState,
  Ctx: Serialize,
  C: ConnectionTrait,
{
  for email in emails {
    notify::<S, _, _>(kind, Some(email.as_str()), &context, db).await;
  }
}

/// Best-effort membership notification. Never fails the caller: it is skipped
/// when SMTP is not configured or no recipient is known, and any send error is
/// logged rather than propagated.
pub async fn notify<S, Ctx, C>(kind: &str, recipient_email: Option<&str>, context: Ctx, db: &C)
where
  S: SumidaState,
  Ctx: Serialize,
  C: ConnectionTrait,
{
  let Some(email) = recipient_email else {
    return;
  };
  let Some(smtp) = settings::get().smtp.as_ref() else {
    return;
  };
  let options = Some(MailFormSendOptions {
    username: smtp.username.clone(),
    password: smtp.password.clone(),
    auth:     smtp.auth.clone(),
    server:   smtp.server.clone(),
    port:     smtp.resolved_port(),
    tls:      smtp.tls,
  });

  if let Err(err) = MailFormService::<S>::send(kind, email, context, options, db).await {
    tracing::warn!("membership notification '{}' skipped/failed: {}", kind, err);
  }
}

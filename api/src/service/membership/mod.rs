pub mod coupon;
pub mod credit;
pub mod entitlement;
pub mod maintenance;
pub mod payment;
pub mod plan;
pub mod purchase;
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
/// - personal: always the session user (clients cannot act for other users).
/// - organization: the given org id, defaulting to the session user's org.
pub fn resolve_owner(
  session: &UserSession,
  owner_kind: &OwnerKind,
  owner_id: Option<i32>,
) -> (OwnerKind, i32) {
  match owner_kind {
    // A normal user can only act for themselves; a system admin may target an
    // explicit user (e.g. granting credits to someone). Org targeting is always
    // explicit and gated separately by the org-scoped authorization checks.
    OwnerKind::Personal => {
      let id = if session.profile.is_admin {
        owner_id.unwrap_or(session.profile.id)
      } else {
        session.profile.id
      };
      (OwnerKind::Personal, id)
    }
    OwnerKind::Organization => {
      (OwnerKind::Organization, owner_id.unwrap_or(session.profile.organization.id))
    }
  }
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
/// 종전에는 `org:subscription` / `org:credit` 정책 행의 `"user"` 문자열을 손으로 파싱했는데,
/// 그 파서는 `api:` 로 시작하는 수신자를 **조용히 버렸다**(설계 §8). 이제 관계 자체를 되짚어
/// 읽고(`subjects_with`) 주체를 사용자 id 로 펼치므로(`expand_to_user_ids`), 키에 부여된
/// 권한도 알림을 받는다.
pub async fn org_manager_emails<C: ConnectionTrait>(
  org_id: i32,
  db: &C,
) -> Result<Vec<String>, SumidaError> {
  use std::collections::HashSet;

  // 세 관계는 `organization` 의 `scope subscription` / `credit_manager` / 총괄 `manager` 다.
  // 종전 정책의 `action = {manage, *}` 가 뜻하던 층이 정확히 이 셋이다.
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
    auth: smtp.auth.clone(),
    server: smtp.server.clone(),
    port: smtp.resolved_port(),
    tls: smtp.tls,
  });

  if let Err(err) = MailFormService::<S>::send(kind, email, context, options, db).await {
    tracing::warn!("membership notification '{}' skipped/failed: {}", kind, err);
  }
}

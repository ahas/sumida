pub mod credit;
pub mod entitlement;
pub mod payment;
pub mod plan;
pub mod purchase;
pub mod subscription;

use entity::membership::OwnerKind;
use infra::session::UserSession;
use infra::settings;
use sea_orm::ConnectionTrait;
use serde::Serialize;

use crate::SumidaState;
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

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
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, FromQueryResult, QueryFilter, Statement};
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

/// Organization ids the session may manage under `namespace`/`action`: every org
/// the caller holds an allow policy on (matching `action` or `*`), plus all of
/// their descendant organizations. Used to widen list visibility so a parent-org
/// manager sees child-org rows (single-row access is already hierarchy-aware via
/// the org-ancestors lateral join).
pub async fn managed_org_ids<C: ConnectionTrait>(
  session: &UserSession,
  namespace: &str,
  action: &str,
  db: &C,
) -> Result<Vec<i32>, SumidaError> {
  // The identities the AP system expands `who(&session)` into.
  let mut identities = vec![
    format!("user:{}", session.profile.id),
    format!("role:{}", session.profile.role.id),
    format!("org:{}", session.profile.organization.id),
  ];
  if let Some(key) = &session.profile.api_key {
    identities.push(format!("api:{key}"));
  }
  if session.profile.is_admin {
    identities.push("user:admin".to_string());
  }

  let policies = AccessPolicies::find()
    .filter(AccessPoliciesColumn::Namespace.eq(namespace))
    .filter(AccessPoliciesColumn::Effect.eq("allow"))
    .filter(AccessPoliciesColumn::User.is_in(identities))
    .all(db)
    .await?;

  let mut seed: Vec<i32> = policies
    .into_iter()
    .filter(|p| p.action.iter().any(|a| a == action || a == "*"))
    .filter_map(|p| p.object.parse::<i32>().ok())
    .collect();
  seed.sort_unstable();
  seed.dedup();
  if seed.is_empty() {
    return Ok(vec![]);
  }

  // Expand seed orgs to all descendants (ints are safe to inline).
  let list = seed.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
  #[derive(FromQueryResult)]
  struct IdRow {
    id: i32,
  }
  let sql = format!(
    r#"WITH RECURSIVE _mng AS (
         SELECT id FROM organizations WHERE id = ANY(ARRAY[{list}]::int[])
         UNION
         SELECT o.id FROM organizations o JOIN _mng m ON o.parent_id = m.id
       )
       SELECT id FROM _mng"#
  );
  let rows = IdRow::find_by_statement(Statement::from_string(db.get_database_backend(), sql))
    .all(db)
    .await?;
  Ok(rows.into_iter().map(|r| r.id).collect())
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

/// Emails of the users who manage organization `org_id`'s membership: expanded
/// from the `user:` / `role:` / `org:` identities on its `org:subscription` /
/// `org:credit` allow policies (matching `manage` or `*`).
pub async fn org_manager_emails<C: ConnectionTrait>(
  org_id: i32,
  db: &C,
) -> Result<Vec<String>, SumidaError> {
  use std::collections::HashSet;

  let policies = AccessPolicies::find()
    .filter(AccessPoliciesColumn::Effect.eq("allow"))
    .filter(AccessPoliciesColumn::Object.eq(org_id.to_string()))
    .filter(AccessPoliciesColumn::Namespace.is_in(["org:subscription".to_string(), "org:credit".to_string()]))
    .all(db)
    .await?;

  let (mut user_ids, mut role_ids, mut org_ids) = (HashSet::new(), HashSet::new(), HashSet::new());
  for p in policies {
    if !p.action.iter().any(|a| a == "manage" || a == "*") {
      continue;
    }
    let u = p.user.as_str();
    if let Some(n) = u.strip_prefix("user:").and_then(|x| x.parse::<i32>().ok()) {
      user_ids.insert(n);
    } else if let Some(n) = u.strip_prefix("role:").and_then(|x| x.parse::<i32>().ok()) {
      role_ids.insert(n);
    } else if let Some(n) = u.strip_prefix("org:").and_then(|x| x.parse::<i32>().ok()) {
      org_ids.insert(n);
    }
  }

  let mut emails: HashSet<String> = HashSet::new();
  for (col, ids) in [
    (entity::users::Column::Id, user_ids),
    (entity::users::Column::RoleId, role_ids),
    (entity::users::Column::OrganizationId, org_ids),
  ] {
    if ids.is_empty() {
      continue;
    }
    for u in Users::find().filter(col.is_in(ids)).all(db).await? {
      if let Some(e) = u.email {
        emails.insert(e);
      }
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

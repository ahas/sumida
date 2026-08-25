use axum::Json;
use axum::response::IntoResponse;
use entity::membership::OwnerKind;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use infra::settings::membership::GateOrder;
use sea_orm::{ConnectionTrait, FromQueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::credit::CreditService;
use super::{owner_kind_str, resolve_owner};
use crate::SumidaState;

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequireActionRequest {
  pub action:          String,
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementOutcome {
  /// How the action was satisfied: `subscription` or `credit`.
  pub via:      String,
  pub consumed: i64,
}

impl IntoResponse for EntitlementOutcome {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

pub struct EntitlementService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> EntitlementService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Try to satisfy `action` from an active subscription, enforcing per-period
  /// quota. `current_period_end` is the authoritative time gate; `allowed_actions`
  /// is a snapshot object `{action: quota|null}` (key present = covered, value =
  /// per-period limit, null = unlimited).
  ///
  /// Returns `true` (and records one use) when covered and within quota; `false`
  /// when not covered or the quota for the current period is exhausted (so the
  /// caller can fall through to credits). The quota check + increment is a single
  /// atomic upsert, so concurrent calls cannot exceed the limit.
  async fn try_consume_subscription<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    db: &C,
  ) -> Result<bool, SumidaError> {
    #[derive(FromQueryResult)]
    struct SubRow {
      id:                   i32,
      current_period_start: sea_orm::prelude::DateTimeWithTimeZone,
      quota:                Option<serde_json::Value>,
    }
    let backend = db.get_database_backend();
    let Some(sub) = SubRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      // Entitled while active in-period, OR past_due inside an open grace window.
      r#"SELECT id, current_period_start, (allowed_actions -> $3) AS quota
         FROM subscriptions
         WHERE owner_kind = $1 AND owner_id = $2
           AND jsonb_exists(allowed_actions, $3)
           AND (
             (status = 'active' AND current_period_end > now())
             OR (status = 'past-due' AND grace_ends_at IS NOT NULL AND grace_ends_at > now())
           )
         ORDER BY current_period_end DESC
         LIMIT 1"#,
      [owner_kind_str(owner_kind).into(), owner_id.into(), action.into()],
    ))
    .one(db)
    .await?
    else {
      return Ok(false);
    };

    // Unlimited (quota value is JSON null / absent number) → covered, no counting.
    let quota = match sub.quota.as_ref().and_then(|v| v.as_i64()) {
      Some(n) => n,
      None => return Ok(true),
    };
    if quota <= 0 {
      return Ok(false);
    }

    // Atomic check-and-increment scoped to this subscription's current period.
    let consumed = db
      .execute(Statement::from_sql_and_values(
        backend,
        r#"INSERT INTO subscription_usage (subscription_id, action, period_start, used)
         VALUES ($1, $2, $3, 1)
         ON CONFLICT (subscription_id, action, period_start)
         DO UPDATE SET used = subscription_usage.used + 1
         WHERE subscription_usage.used < $4"#,
        [sub.id.into(), action.into(), sub.current_period_start.into(), quota.into()],
      ))
      .await?;

    Ok(consumed.rows_affected() > 0)
  }

  /// Hybrid entitlement gate. Tries the active subscription and credit balance
  /// in the configured order; consumes credits when that path is taken.
  /// AP cannot express expiry/quantity, so this time/quantity check lives here.
  pub async fn require_action(
    &self,
    action: &str,
    owner_kind: OwnerKind,
    owner_id: Option<i32>,
    idempotency_key: Option<&str>,
  ) -> Result<EntitlementOutcome, SumidaError> {
    let (owner_kind, owner_id) = resolve_owner(&self.session, &owner_kind, owner_id);
    let credit = CreditService::new(self.state.clone(), self.session.clone());

    let txn = self.state.db().begin().await?;
    credit.authorize_use(&owner_kind, owner_id, &txn).await?;

    let outcome = match settings::get().membership.default_gate_order {
      GateOrder::SubscriptionFirst => {
        if self.try_consume_subscription(&owner_kind, owner_id, action, &txn).await? {
          EntitlementOutcome {
            via:      "subscription".into(),
            consumed: 0,
          }
        } else {
          let out = credit
            .consume_in_txn(&owner_kind, owner_id, action, None, idempotency_key, None, &txn)
            .await?;
          EntitlementOutcome {
            via:      "credit".into(),
            consumed: out.consumed,
          }
        }
      }
      GateOrder::CreditFirst => {
        match credit
          .consume_in_txn(&owner_kind, owner_id, action, None, idempotency_key, None, &txn)
          .await
        {
          Ok(out) => EntitlementOutcome {
            via:      "credit".into(),
            consumed: out.consumed,
          },
          Err(SumidaError::InsufficientCredits { .. })
          | Err(SumidaError::MembershipActionNotAllowed(_)) => {
            if self.try_consume_subscription(&owner_kind, owner_id, action, &txn).await? {
              EntitlementOutcome {
                via:      "subscription".into(),
                consumed: 0,
              }
            } else {
              return Err(SumidaError::NoActiveSubscription);
            }
          }
          Err(e) => return Err(e),
        }
      }
    };

    txn.commit().await?;
    Ok(outcome)
  }
}

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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequireActionRequest {
  pub action: String,
  #[serde(default = "default_owner_kind")]
  pub owner_kind: OwnerKind,
  #[serde(default)]
  pub owner_id: Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementOutcome {
  /// How the action was satisfied: `subscription` or `credit`.
  pub via: String,
  pub consumed: i64,
}

impl IntoResponse for EntitlementOutcome {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

pub struct EntitlementService<S: SumidaState> {
  state: S,
  session: UserSession,
}

impl<S: SumidaState> EntitlementService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  /// Does an active subscription of this owner cover `action`?
  /// `current_period_end` is the authoritative gate (status is a denormalization).
  async fn check_subscription<C: ConnectionTrait>(
    &self,
    owner_kind: &OwnerKind,
    owner_id: i32,
    action: &str,
    db: &C,
  ) -> Result<bool, SumidaError> {
    #[derive(FromQueryResult)]
    struct ExistsRow {
      ok: bool,
    }
    let backend = db.get_database_backend();
    let row = ExistsRow::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"SELECT EXISTS(
           SELECT 1 FROM subscriptions
           WHERE owner_kind = $1 AND owner_id = $2
             AND status = 'active' AND current_period_end > now()
             AND jsonb_exists(allowed_actions, $3)
         ) AS ok"#,
      [owner_kind_str(owner_kind).into(), owner_id.into(), action.into()],
    ))
    .one(db)
    .await?;
    Ok(row.map(|r| r.ok).unwrap_or(false))
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
        if self.check_subscription(&owner_kind, owner_id, action, &txn).await? {
          EntitlementOutcome { via: "subscription".into(), consumed: 0 }
        } else {
          let out = credit
            .consume_in_txn(&owner_kind, owner_id, action, None, idempotency_key, None, &txn)
            .await?;
          EntitlementOutcome { via: "credit".into(), consumed: out.consumed }
        }
      }
      GateOrder::CreditFirst => {
        match credit
          .consume_in_txn(&owner_kind, owner_id, action, None, idempotency_key, None, &txn)
          .await
        {
          Ok(out) => EntitlementOutcome { via: "credit".into(), consumed: out.consumed },
          Err(SumidaError::InsufficientCredits { .. }) | Err(SumidaError::MembershipActionNotAllowed(_)) => {
            if self.check_subscription(&owner_kind, owner_id, action, &txn).await? {
              EntitlementOutcome { via: "subscription".into(), consumed: 0 }
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

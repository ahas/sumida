use axum::Json;
use axum::response::IntoResponse;
use entity::membership::OwnerKind;
use error::SumidaError;
use infra::session::UserSession;
use infra::settings;
use infra::settings::membership::GateOrder;
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};

use super::credit::{Cost, CreditService};
use super::quota::QuotaService;
use super::resolve_owner;
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
  /// What the action spends. Recorded against the subscription's amount quota when it covers
  /// the action and has room, else spent from the wallet in place of the action's fixed cost.
  #[serde(default)]
  pub amount:          Option<i64>,
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

  /// Hybrid entitlement gate. Tries the active subscription (its count quota and, for an
  /// `amount`, its amount quota) and the credit balance in the configured order; consumes credits
  /// when that path is taken. AP cannot express expiry/quantity, so this time/quantity check
  /// lives here.
  pub async fn require_action(
    &self,
    action: &str,
    owner_kind: OwnerKind,
    owner_id: Option<i32>,
    amount: Option<i64>,
    idempotency_key: Option<&str>,
  ) -> Result<EntitlementOutcome, SumidaError> {
    let (owner_kind, owner_id) =
      resolve_owner(&self.session, &owner_kind, owner_id, self.state.db()).await?;
    let credit = CreditService::new(self.state.clone(), self.session.clone());

    let txn = self.state.db().begin().await?;
    credit.authorize_use(&owner_kind, owner_id, &txn).await?;

    let cost = Cost {
      action: Some(action),
      amount,
      plan_code: None,
    };
    let outcome = match settings::get().membership.default_gate_order {
      GateOrder::SubscriptionFirst => {
        if QuotaService::<S>::consume_in_txn(&owner_kind, owner_id, action, amount, idempotency_key, &txn)
          .await?
        {
          EntitlementOutcome {
            via:      "subscription".into(),
            consumed: 0,
          }
        } else {
          let out =
            CreditService::<S>::consume_in_txn(&owner_kind, owner_id, cost, idempotency_key, None, &txn)
              .await?;
          EntitlementOutcome {
            via:      "credit".into(),
            consumed: out.consumed,
          }
        }
      }
      GateOrder::CreditFirst => {
        match CreditService::<S>::consume_in_txn(&owner_kind, owner_id, cost, idempotency_key, None, &txn)
          .await
        {
          Ok(out) => EntitlementOutcome {
            via:      "credit".into(),
            consumed: out.consumed,
          },
          Err(SumidaError::InsufficientCredits { .. })
          | Err(SumidaError::MembershipActionNotAllowed(_)) => {
            if QuotaService::<S>::consume_in_txn(
              &owner_kind,
              owner_id,
              action,
              amount,
              idempotency_key,
              &txn,
            )
            .await?
            {
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

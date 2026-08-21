//! The role form's and the api-key form's wire shape, translated to and from service-tier grants.
//!
//! `RoleForm.vue` and the api-key form post
//! `accessPolicies: [{ namespace, object: ["service"], action, effect }]` and read the same shape
//! back. **That wire contract is fixed by the clients**, so this module translates rather than
//! reshapes.
//!
//! Two shapes the payload can carry and the tuple store cannot, both refused here rather than
//! silently reinterpreted:
//!
//! * **`effect: "deny"`.** A tuple grants; there is no negative tuple and no `effect` column.
//!   Blocking is a property of the *rule* — an exclusion over a `blocked` relation — so a form that
//!   posts one is asking for something no store can hold, and answering `200` would report a block
//!   that does not exist.
//! * **an object other than `"service"`.** These two forms only ever posted the route-gate tier.
//!   A per-row grant to a role is `/relation-tuples`, whose authority check is the object's own
//!   `#grant`; accepting it here would write it against `service:<namespace>#grant` instead, which
//!   is a different authority.

use error::SumidaError;
use serde::{Deserialize, Serialize};

use crate::authz::ServiceGrant;

/// One entry of that payload: `{ namespace, object, action, effect }`.
///
/// **A wire shape and only that.** Nothing reads it out of a `SELECT`: every producer builds it
/// from [`from_service_grants`] or from `platform:root#superuser`. Deriving it from a table again
/// would tie the payload the clients are pinned to back to a storage layout that is free to move.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApForUser {
  pub namespace: String,
  pub object:    Vec<String>,
  pub action:    Vec<String>,
  pub effect:    String,
}

/// The wire payload as service-tier grants.
pub fn to_service_grants(policies: &[ApForUser]) -> Result<Vec<ServiceGrant>, SumidaError> {
  policies
    .iter()
    .map(|policy| {
      if policy.effect != "allow" {
        return Err(SumidaError::BadRequest(format!(
          "`{}`: a grant is the only thing that can be stored; a block is a rule change, not a \
           record (effect `{}`)",
          policy.namespace, policy.effect
        )));
      }
      if policy.object.iter().any(|object| object != "service") {
        return Err(SumidaError::BadRequest(format!(
          "`{}`: this form sets the route-gate tier only; a per-object grant goes through \
           /relation-tuples, which checks that object's own `grant` permission",
          policy.namespace
        )));
      }
      Ok(ServiceGrant {
        namespace: policy.namespace.clone(),
        action:    policy.action.clone(),
      })
    })
    .collect()
}

/// The projection back — one entry per namespace, in the shape the CASL client has always read.
///
/// `object` is the literal `["service"]` the legacy `ARRAY_AGG(DISTINCT ap."object")` produced for
/// these rows, and `effect` is always `"allow"` because a stored grant is the only thing there is.
pub fn from_service_grants(grants: Vec<ServiceGrant>) -> Vec<ApForUser> {
  grants
    .into_iter()
    .map(|grant| ApForUser {
      namespace: grant.namespace,
      object:    vec!["service".to_owned()],
      action:    grant.action,
      effect:    "allow".to_owned(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn policy(namespace: &str, object: &[&str], effect: &str) -> ApForUser {
    ApForUser {
      namespace: namespace.to_owned(),
      object:    object.iter().map(|o| (*o).to_owned()).collect(),
      action:    vec!["read".to_owned()],
      effect:    effect.to_owned(),
    }
  }

  /// What the two forms actually post.
  #[test]
  fn a_service_tier_allow_round_trips() {
    let grants = to_service_grants(&[policy("user", &["service"], "allow")]).expect("accepted");
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].namespace, "user");

    let back = from_service_grants(grants);
    assert_eq!(back[0].object, vec!["service".to_owned()]);
    assert_eq!(back[0].effect, "allow");
  }

  /// A block is a rule change now. Storing it as a grant would be the opposite of what was asked.
  #[test]
  fn a_deny_is_refused_rather_than_stored_as_a_grant() {
    let error = to_service_grants(&[policy("user", &["service"], "deny")]).unwrap_err();
    assert!(matches!(error, SumidaError::BadRequest(_)), "{error:?}");
  }

  /// A per-row grant carries a different authority and must not be written through this form.
  #[test]
  fn an_object_tier_entry_is_refused_and_points_at_relation_tuples() {
    let error = to_service_grants(&[policy("board", &["7"], "allow")]).unwrap_err();
    assert!(
      matches!(&error, SumidaError::BadRequest(message) if message.contains("/relation-tuples")),
      "{error:?}"
    );
  }
}

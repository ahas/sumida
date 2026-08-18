//! What the caller may do — the `/me/permissions` projection (design §8.2, §9.7).
//!
//! `service_grants` answers the *legacy* question: which rows would `/me/access-policies` have
//! shown. This answers the new one, and the difference is the whole reason §9.7 adds a second
//! endpoint rather than reshaping the first.
//!
//! `/me/access-policies` reports **tuples**. `/me/permissions` reports **permissions**, which is
//! not the same set: `service:<namespace>#operator` is one tuple and confers every capability on
//! that namespace, and `platform:root#superuser` is one tuple and confers them on every namespace.
//! A client that received only the tuples would have to re-derive `can_X = X | op_X | operator |
//! superuser` for itself — a second copy of the rule, in TypeScript, drifting from the one in
//! the rule file. So every entry here is **evaluated through the compiled rule**, never assembled
//! from the raw relations.
//!
//! Two tiers, kept apart because §5.2 deliberately made them different grants:
//!
//! * `services` — may the caller reach the API at all. This is what `service_ap!` gates on, and
//!   what the client uses to decide whether a screen exists.
//! * `operates`  — may the caller act on **everyone's** rows, not only their own. Today's
//!   `object = '*'`. A client that conflates the two shows an operator's global list to somebody
//!   who holds only the route gate, and then the server correctly returns their own rows and the
//!   screen looks broken.
//!
//! `services` always contains `operates`, because `can_X = X | operate_X`. That is a property of
//! the rule and not of this file; `one_operator_tuple_reports_every_verb` in
//! `api/tests/authz_endpoints.rs` is what holds it up.

use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use sea_orm::sea_query::{Alias, Query};
use serde::Serialize;

use super::subject::SubjectSet;
use super::{Authz, AuthzError};

/// Which of the two maps a column belongs to. A two-variant enum rather than the string `"services"`
/// so the match below cannot silently fall into the wrong half on a typo.
#[derive(Clone, Copy)]
enum Tier {
  Service,
  Operate,
}

/// The four verbs a `service` object carries. Written out rather than derived from the registry
/// because the rule names them literally (`can_create`…`operate_delete`), and a fifth verb would be
/// a schema change that has to be made here too — deliberately, not by inference.
const ACTIONS: [&str; 4] = ["create", "read", "update", "delete"];

/// The payload of `GET /me/permissions` (design §9.7).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effective {
  /// The identities this request is judged as, canonical form. Carried because the client's
  /// `/relation-tuples` screen needs to show *which* of them a grant arrived through, and because
  /// a support engineer reading a bug report needs to know whether the call was a login or a
  /// secured API key — the two are different subject sets for the same person.
  pub subjects:  Vec<String>,
  /// `platform:root#superuser`. Split out rather than folded into the maps as a `"*"` namespace,
  /// because a client that has to recognise a magic namespace string is a client that can forget
  /// to.
  pub superuser: bool,
  /// namespace → the verbs whose **route gate** the caller passes.
  pub services:  BTreeMap<String, Vec<String>>,
  /// namespace → the verbs the caller holds **over every row**, not only their own.
  pub operates:  BTreeMap<String, Vec<String>>,
}

impl Authz {
  /// Every service-tier permission the caller effectively holds.
  ///
  /// Two queries, not two per namespace. The first asks whether the caller is a platform
  /// administrator, which short-circuits the rest; the second evaluates every remaining permission
  /// as one `SELECT` of boolean expressions, because a `service:` rule is session-constant — it
  /// reads no row of the object under test, so all of them can be answered in one round trip.
  ///
  /// The candidate set is the namespaces the caller holds **any** `service:` tuple on, and that
  /// narrowing is exact rather than a heuristic: every branch of `can_X` and `operate_X` is either
  /// a tuple on `service:<namespace>` with this subject, or `platform:root->superuser` — which is
  /// the branch already answered above. `service`'s relations declare no `user:*`, so there is no
  /// wildcard branch that could reach a namespace with no tuple of the caller's own.
  pub async fn effective<C: ConnectionTrait>(
    subject: &SubjectSet,
    db: &C,
  ) -> Result<Effective, AuthzError> {
    let mut effective = Effective {
      subjects: subject.keys().to_vec(),
      ..Default::default()
    };

    if subject.is_empty() {
      return Ok(effective);
    }

    effective.superuser = Self::is_superuser(subject, db).await?;

    // R1 reaches every namespace, including ones nobody has ever written a tuple for, so the
    // candidate narrowing below cannot express it. The inventory can: `gated` is exactly "a
    // `service:<namespace>` object exists", which is what validation rule 8 keeps complete.
    if effective.superuser {
      let all: Vec<String> = ACTIONS.iter().map(|action| (*action).to_owned()).collect();
      for entry in Self::registry()?.namespaces().iter().filter(|entry| entry.gated) {
        effective.services.insert(entry.namespace.clone(), all.clone());
        effective.operates.insert(entry.namespace.clone(), all.clone());
      }
      return Ok(effective);
    }

    let namespaces: Vec<String> = Self::service_grants(subject, db)
      .await?
      .into_iter()
      .map(|grant| grant.namespace)
      .collect();
    if namespaces.is_empty() {
      return Ok(effective);
    }

    // One column per (namespace, tier, action). The alias is positional because a namespace is a
    // caller-visible string with colons and dashes in it (`board_post`, `app:terminal-session`), and an
    // identifier built out of one is a quoting decision this file has no business making.
    let mut statement = Query::select();
    let mut columns: Vec<(String, Tier, &'static str)> = Vec::new();

    for namespace in &namespaces {
      for action in ACTIONS {
        for (tier, permission) in
          [(Tier::Service, format!("can_{action}")), (Tier::Operate, format!("operate_{action}"))]
        {
          let predicate = Self::object_predicate("service", namespace.as_str(), &permission, subject)?;
          statement.expr_as(predicate, Alias::new(format!("c{}", columns.len())));
          columns.push((namespace.clone(), tier, action));
        }
      }
    }

    let row = db
      .query_one(db.get_database_backend().build(&statement))
      .await?
      .ok_or_else(|| AuthzError::Database("a `SELECT` of constants returned no row".to_owned()))?;

    for (index, (namespace, tier, action)) in columns.iter().enumerate() {
      if !row.try_get::<bool>("", &format!("c{index}"))? {
        continue;
      }
      let map = match tier {
        Tier::Service => &mut effective.services,
        Tier::Operate => &mut effective.operates,
      };
      map.entry(namespace.clone()).or_default().push((*action).to_owned());
    }

    Ok(effective)
  }
}

//! Row admission for a stream, and the memo that keeps it from asking twice.
//!
//! The two call patterns both start from something the request already named — a list query has
//! the caller, a point check has the caller and an id. A stream has neither at the moment it
//! matters: the subscription is opened before the rows it will carry exist. So the shape is
//! split in two. The scope is settled once, when the subscription opens, and every event after
//! that is judged one row at a time by **the same compiled rule the list query would have used**.
//!
//! Judging every event from scratch puts a query on the wire per frame, and a firehose carries the
//! same row over and over. The memo answers the repeats, and it is the only thing here that is not
//! a plain rule evaluation.
//!
//! What the events are, what they carry, and which of them can be judged at all is the subscribing
//! service's business — this module never sees an event.

use std::collections::HashMap;
use std::hash::Hash;

use sea_orm::ConnectionTrait;

use super::subject::SubjectSet;
use super::{Authz, ObjectKey};

/// What one subscription is allowed to see, settled when it opens.
///
/// `All` is the constant branch: a caller that may operate on every row of the kind is asked once
/// and then never again, because no per-row answer can come back different. `Scoped` keeps the
/// caller's subject set and hands it to every row judgement.
pub enum StreamScope {
  All,
  Scoped(SubjectSet),
}

/// Remembers which rows one subscription may carry.
///
/// The verdict is memoised for the life of the connection, so the second event about a row costs
/// nothing. A grant revoked mid-connection is therefore not seen by that connection — the stream
/// is reopened, and putting a query back on the wire for every event costs more than that is
/// worth.
///
/// The memo has no bound. It holds one entry per distinct row the connection has been asked about,
/// so a subscription that can see an unbounded number of rows over one connection is the one that
/// has to bound it.
pub struct RowAdmission<K> {
  definition: &'static str,
  permission: &'static str,
  scope:      StreamScope,
  memo:       HashMap<K, bool>,
}

impl<K: Copy + Eq + Hash + Into<ObjectKey>> RowAdmission<K> {
  /// The definition, the permission and the key type are the subscriber's — this module names
  /// none of them. A stream over a different kind of row, or one judged by a permission other than
  /// reading, is the same machinery with different arguments.
  pub fn new(definition: &'static str, permission: &'static str, scope: StreamScope) -> Self {
    Self {
      definition,
      permission,
      scope,
      memo: HashMap::new(),
    }
  }

  /// Whether this subscription may carry the row, asking the evaluator only the first time.
  ///
  /// A query that fails refuses. The alternative is to let a database error widen a stream, and a
  /// widened stream is the failure nobody notices.
  pub async fn admits<C: ConnectionTrait>(&mut self, id: K, db: &C) -> bool {
    let subject = match &self.scope {
      StreamScope::All => return true,
      StreamScope::Scoped(subject) => subject,
    };

    if let Some(known) = self.memo.get(&id) {
      return *known;
    }

    let allowed = Authz::check(self.definition, id, self.permission, subject, db).await.unwrap_or(false);
    self.memo.insert(id, allowed);
    allowed
  }
}

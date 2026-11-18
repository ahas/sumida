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

use std::collections::{HashMap, VecDeque};
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
/// How many rows one connection can be asked about is known where it is opened, so the bound is an
/// argument rather than a constant. A subscription that reaches a settled set of rows needs none;
/// one that carries a whole feed grows its memo for as long as it stays open, and that one names a
/// bound. At the bound the oldest verdict goes, because the row asked about longest ago is the one
/// least likely to be asked about next.
pub struct RowAdmission<K> {
  definition: &'static str,
  permission: &'static str,
  scope:      StreamScope,
  memo:       HashMap<K, bool>,
  /// Insertion order, kept only when there is a bound to enforce.
  order:      VecDeque<K>,
  cap:        Option<usize>,
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
      order: VecDeque::new(),
      cap: None,
    }
  }

  /// The same admission, remembering at most `cap` verdicts.
  ///
  /// For a subscription whose rows are not enumerable when it opens — a feed over every row of a
  /// kind — where an unbounded memo would grow with the connection.
  pub fn bounded(
    definition: &'static str,
    permission: &'static str,
    scope: StreamScope,
    cap: usize,
  ) -> Self {
    Self {
      cap: Some(cap),
      ..Self::new(definition, permission, scope)
    }
  }

  /// Whether the scope lets every row through, so a caller can skip work it would only discard.
  pub fn admits_all(&self) -> bool {
    matches!(self.scope, StreamScope::All)
  }

  /// Records a verdict, dropping the oldest first when a bound is set and reached.
  ///
  /// Only reached for a key the memo does not hold, so `order` never carries the same key twice.
  fn remember(&mut self, id: K, allowed: bool) {
    if let Some(cap) = self.cap {
      if self.memo.len() >= cap
        && let Some(oldest) = self.order.pop_front()
      {
        self.memo.remove(&oldest);
      }
      self.order.push_back(id);
    }

    self.memo.insert(id, allowed);
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
    self.remember(id, allowed);
    allowed
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 상한에 닿아도 memo 는 비워지지 않는다. 오래된 판정 하나만 빠지고 나머지는 남는다 —
  /// 통째로 비우면 상한을 넘긴 피드가 이후 매 이벤트마다 판정을 다시 하게 된다.
  #[test]
  fn a_full_memo_drops_its_oldest_verdict_and_keeps_the_rest() {
    const CAP: usize = 4;
    let mut admission = RowAdmission::<i32>::bounded("thing", "view", StreamScope::All, CAP);

    for id in 0..=CAP as i32 {
      admission.remember(id, true);
    }

    assert_eq!(admission.memo.len(), CAP);
    assert!(!admission.memo.contains_key(&0), "가장 오래된 판정이 빠진다");
    assert!(admission.memo.contains_key(&1), "그 다음 판정은 남는다");
    assert!(admission.memo.contains_key(&(CAP as i32)), "방금 넣은 판정은 남는다");
  }

  /// 상한이 없으면 아무것도 버리지 않는다 — 닿을 수 있는 행이 정해진 구독의 기본값이다.
  #[test]
  fn an_unbounded_memo_keeps_everything() {
    let mut admission = RowAdmission::<i32>::new("thing", "view", StreamScope::All);

    for id in 0..64 {
      admission.remember(id, true);
    }

    assert_eq!(admission.memo.len(), 64);
  }
}

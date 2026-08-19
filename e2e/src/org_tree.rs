//! Organization-hierarchy fixtures.
//!
//! [`crate::user::TestOrg::create`] makes a single flat organization with no parent, which is
//! enough for "same org / different org" but cannot express the rule the audit is actually about:
//! a manager reaches their own organization **and every descendant, to arbitrary depth**, and
//! neither ancestors nor siblings.
//!
//! Getting that wrong is silent in both directions — an inverted ancestor walk hands child
//! organizations authority over their parents, and a depth-1 test passes either way. So the
//! fixtures here build a tree deep and wide enough that direction and depth both show up, and
//! name each node by the role it plays in a case rather than by id.

use entity::prelude::*;
use sea_orm::{ConnectionTrait, EntityTrait, Set};

use crate::user::{TestOrg, TestUser};

/// A named organization tree.
///
/// The default shape is the smallest one that can distinguish every relationship the rule
/// mentions. Anything shallower cannot tell "descendants" from "the whole table".
///
/// ```text
///                    root
///                   /    \
///              parent      aunt          ← aunt is the ancestor-side control
///             /      \
///        subject     sibling             ← the org under test, and its peer
///           |
///         child                          ← depth 1 below subject
///           |
///      grandchild                        ← depth 2; catches a walk that stops at one level
/// ```
pub struct OrgTree {
  pub root:       i32,
  pub aunt:       i32,
  pub parent:     i32,
  pub subject:    i32,
  pub sibling:    i32,
  pub child:      i32,
  pub grandchild: i32,
}

impl OrgTree {
  /// Builds the tree above. Every node is a real `organizations` row, so the recursive walks
  /// under test run over real data rather than a stub.
  pub async fn create<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    let root = Self::node(db, "root", None).await?;
    let aunt = Self::node(db, "aunt", Some(root)).await?;
    let parent = Self::node(db, "parent", Some(root)).await?;
    let subject = Self::node(db, "subject", Some(parent)).await?;
    let sibling = Self::node(db, "sibling", Some(parent)).await?;
    let child = Self::node(db, "child", Some(subject)).await?;
    let grandchild = Self::node(db, "grandchild", Some(child)).await?;

    Ok(Self {
      root,
      aunt,
      parent,
      subject,
      sibling,
      child,
      grandchild,
    })
  }

  /// The organizations a manager of `subject` must reach: itself and everything below it.
  pub fn subject_and_descendants(&self) -> [i32; 3] {
    [self.subject, self.child, self.grandchild]
  }

  /// The organizations that same manager must **not** reach. Ancestors and siblings are in here
  /// on purpose — an inverted walk shows up as one of these becoming visible.
  pub fn out_of_scope(&self) -> [i32; 3] {
    [self.root, self.parent, self.sibling]
  }

  async fn node<C: ConnectionTrait>(db: &C, name: &str, parent: Option<i32>) -> anyhow::Result<i32> {
    let suffix = uniq();
    Ok(
      Organizations::insert(entity::organizations::ActiveModel {
        name: Set(format!("{name}-{suffix}")),
        parent_id: Set(parent),
        ..Default::default()
      })
      .exec(db)
      .await?
      .last_insert_id,
    )
  }

  /// Re-parents `org` under `new_parent`, for the case where the tree changes while a session is
  /// live. A cached or session-carried org set would keep answering with the old shape; a query
  /// that resolves the hierarchy per request would not.
  pub async fn reparent<C: ConnectionTrait>(
    db: &C,
    org: i32,
    new_parent: Option<i32>,
  ) -> anyhow::Result<()> {
    Organizations::update(entity::organizations::ActiveModel {
      id: Set(org),
      parent_id: Set(new_parent),
      ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
  }

  /// Builds a chain `depth` links long below `from`, returning every node top-down.
  ///
  /// Used to check that a hierarchy walk has no hidden depth limit — a recursive CTE and a
  /// hand-written two-level join agree at depth 2 and diverge at depth 5.
  pub async fn chain<C: ConnectionTrait>(db: &C, from: i32, depth: usize) -> anyhow::Result<Vec<i32>> {
    let mut out = Vec::with_capacity(depth);
    let mut parent = from;
    for level in 0..depth {
      parent = Self::node(db, &format!("d{level}"), Some(parent)).await?;
      out.push(parent);
    }
    Ok(out)
  }
}

/// A member of a given organization, labelled by the part it plays in a case.
pub struct Member {
  pub user: TestUser,
  pub org:  i32,
}

impl Member {
  pub async fn create<C: ConnectionTrait>(db: &C, org: i32) -> anyhow::Result<Self> {
    Ok(Self {
      user: TestUser::create_in_org(db, org).await?,
      org,
    })
  }

  /// An administrator. Kept beside [`Member::create`] so a case can put both tiers in scope
  /// without reaching into two different fixture modules.
  pub async fn create_admin<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
    let user = TestUser::create_admin(db).await?;
    let org = user.model.organization_id;
    Ok(Self { user, org })
  }
}

/// A standalone organization with no relation to a tree, for the "unrelated tenant" control.
pub async fn unrelated_org<C: ConnectionTrait>(db: &C) -> anyhow::Result<i32> {
  Ok(TestOrg::create(db).await?.model.id)
}

fn uniq() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
  format!("{nanos:x}")
}

use core::session::UserContext;
use entity::relation_tuples::Column as RelColumn;
use entity::relation_tuples::{Column as RtCol, Entity as Rt};
use sea_orm::sea_query::Alias;
use sea_orm::{Condition, IdentityOf, JoinType, QuerySelect, prelude::*};

pub struct ReBAC {
  _namespace: Option<String>,
  _object: Option<String>,
  _relation: Option<String>,
}

impl ReBAC {
  pub fn new() -> Self {
    Self {
      _namespace: None,
      _object: None,
      _relation: None,
    }
  }

  pub fn namespace<S: AsRef<str>>(mut self, namespace: S) -> Self {
    self._namespace = Some(namespace.as_ref().to_owned());
    self
  }

  pub fn object<S: AsRef<str>>(mut self, object: S) -> Self {
    self._object = Some(object.as_ref().to_owned());
    self
  }

  pub fn relation<S: AsRef<str>>(mut self, relation: S) -> Self {
    self._relation = Some(relation.as_ref().to_owned());
    self
  }

  pub fn build(self, rt_alias: &Alias, user_ctx: &UserContext) -> Condition {
    let ns = self._namespace.expect("ReBAC: namespace must be set");
    let rel = self._relation.expect("ReBAC: relation must be set");
    let obj = self._object;
    let up = &user_ctx.profile;

    let mut viewer_match =
      Condition::any().add(Expr::col((rt_alias.clone(), RelColumn::User)).eq(format!("user:{}", up.id)));

    viewer_match = viewer_match
      .add(Expr::col((rt_alias.clone(), RelColumn::User)).eq(format!("org:{}", up.organization.id)));

    if up.is_admin {
      viewer_match = viewer_match.add(Expr::col((rt_alias.clone(), RelColumn::User)).eq("user:admin"));
    }

    let lhs = Expr::col((rt_alias.clone(), RelColumn::Id)).is_null();
    let mut rhs = Condition::all()
      .add(Expr::col((rt_alias.clone(), RelColumn::Namespace)).eq(ns))
      .add(Expr::col((rt_alias.clone(), RelColumn::Relation)).eq(rel));

    if let Some(obj) = obj {
      rhs = rhs.add(Expr::col((rt_alias.clone(), RelColumn::Object)).eq(obj));
    }

    rhs = rhs.add(viewer_match);

    Condition::any().add(lhs).add(rhs)
  }

  pub fn left_join_with<E: EntityTrait, I: IdentityOf<E>>(
    query: Select<E>,
    entity: E,
    col_to: I,
    rt_alias: &Alias,
  ) -> Select<E> {
    query.join_as_rev(
      JoinType::LeftJoin,
      Rt::belongs_to(entity).from(RtCol::Object).to(col_to).into(),
      rt_alias.clone(),
    )
  }
}

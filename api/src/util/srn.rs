use core::session::UserContext;
use entity::access_policies::{Column as ApCol, Entity as Ap};
use error::SumidaError;
use sea_orm::ConnectionTrait;
use sea_orm::PaginatorTrait;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::IntoTableRef;
use sea_orm::sea_query::SelectStatement;
use sea_orm::{
  Condition, EntityName, EntityTrait, JoinType, QueryFilter, QuerySelect, RelationDef, Select,
  sea_query::{Expr, SimpleExpr, extension::postgres::PgExpr},
};

pub struct Srn {
  _namespace: Option<String>,
  _action: Option<String>,
  _user_id: Option<i32>,
  _role_id: Option<i32>,
  _org_id: Option<i32>,
  _alias: Option<Alias>,
  _is_admin: bool,
  _parts: Vec<String>,
  _cols: Vec<SimpleExpr>,
  _joins: Vec<(JoinType, RelationDef)>,
}

impl Srn {
  pub fn new<P: AsRef<str>>(pattern: P) -> Self {
    let parts = Self::_extract_pattern_parts(pattern.as_ref());

    Self {
      _parts: parts,
      _namespace: None,
      _action: None,
      _user_id: None,
      _role_id: None,
      _org_id: None,
      _alias: None,
      _is_admin: false,
      _cols: Vec::new(),
      _joins: Vec::new(),
    }
  }

  fn _extract_pattern_parts(pattern: &str) -> Vec<String> {
    pattern.split('/').map(|segment| segment.split(':').next().unwrap().to_string()).collect()
  }

  fn _build_wildcard_exprs(&self) -> Vec<SimpleExpr> {
    let seg_cnt = self._parts.len();
    let mut exprs: Vec<SimpleExpr> = Vec::new();

    let mut cat = String::from("*");

    for _ in 1..seg_cnt {
      cat += "/*";
    }

    exprs.push(Expr::val(cat).into());

    for i in 0..seg_cnt {
      let mut cat = format!("{}:*", self._parts[0]);

      for j in 1..=i {
        cat += &format!("/{}:*", self._parts[j]);
      }

      for _ in (i + 1)..seg_cnt {
        cat += "/*";
      }

      exprs.push(Expr::val(cat).into());
    }

    exprs
  }

  fn _build_specific_exprs(&self) -> Vec<SimpleExpr> {
    let parts = &self._parts;
    let cols = &self._cols;
    let seg_cnt = parts.len();
    let mut results = Vec::<SimpleExpr>::new();

    for i in 0..seg_cnt {
      if i > 0 {
        let mut expr = Expr::val(format!("{}:", parts[0])).concat(cols[0].clone());

        for j in 1..i {
          expr = expr.concat(Expr::val(format!("/{p}:", p = parts[j]))).concat(cols[j].clone());
        }

        expr = expr.concat(Expr::val(format!("/{p}:*", p = parts[i])));

        for _ in (i + 1)..seg_cnt {
          expr = expr.concat(Expr::val("/*"));
        }

        results.push(expr);
      }

      let mut expr = Expr::val(format!("{}:", parts[0])).concat(cols[0].clone());

      for j in 1..=i {
        expr = expr.concat(Expr::val(format!("/{p}:", p = parts[j]))).concat(cols[j].clone());
      }

      for _ in (i + 1)..seg_cnt {
        expr = expr.concat(Expr::val("/*"));
      }

      results.push(expr);
    }

    results
  }

  fn _build_object_conditions(&self, ap_alias: Alias) -> Condition {
    let wildcard_exprs = self._build_wildcard_exprs();
    let mut cond = Condition::any();

    if self._cols.is_empty() {
      for expr in wildcard_exprs.into_iter() {
        cond = cond.add(Expr::col((ap_alias.clone(), ApCol::Object)).eq(expr));
      }
    } else {
      let specific_exprs = self._build_specific_exprs();

      for expr in wildcard_exprs.into_iter().chain(specific_exprs.into_iter()) {
        cond = cond.add(Expr::col((ap_alias.clone(), ApCol::Object)).eq(expr));
      }
    }

    cond
  }

  fn _build_user_conditions(&self, ap_alias: Alias) -> Condition {
    let col_user = Expr::col((ap_alias.clone(), ApCol::User));
    let mut condition = Condition::any().add(col_user.clone().eq("*"));

    // Match by user ID if provided.
    if let Some(user_id) = self._user_id {
      condition = condition.add(col_user.clone().eq(format!("user:{}", user_id)));
    }

    // Match by organization ID if provided.
    if let Some(org_id) = self._org_id {
      condition = condition.add(col_user.clone().eq(format!("org:{}", org_id)));
    }

    // Match by role ID if provided.
    if let Some(role_id) = self._role_id {
      condition = condition.add(col_user.clone().eq(format!("role:{}", role_id)));
    }

    // Always allow admin access if applicable.
    if self._is_admin {
      condition = condition.add(col_user.clone().eq("user:admin"));
    }

    condition
  }

  fn _build_ns_conditions(&self, ap_alias: Alias) -> Condition {
    let col_ns = Expr::col((ap_alias.clone(), ApCol::Namespace));
    let mut condition = Condition::any().add(col_ns.clone().eq("*"));

    if let Some(ns) = &self._namespace {
      condition = condition.add(col_ns.clone().eq(ns));
    }

    condition
  }

  fn _build_action_condition(&self, ap_alias: Alias) -> Condition {
    let col_action = Expr::col((ap_alias.clone(), ApCol::Action));
    let mut condition = Condition::any().add(col_action.clone().eq("*"));

    if let Some(action) = &self._action {
      condition = condition.add(col_action.clone().eq(action));
    }

    condition
  }

  fn _unwrap_alias(&self) -> Alias {
    self._alias.clone().unwrap_or_else(|| Alias::new(Ap.table_name()))
  }

  pub fn namespace<S: AsRef<str>>(mut self, namespace: S) -> Self {
    self._namespace = Some(namespace.as_ref().to_owned());
    self
  }

  pub fn action<S: AsRef<str>>(mut self, action: S) -> Self {
    self._action = Some(action.as_ref().to_owned());
    self
  }

  pub fn user(mut self, id: i32) -> Self {
    self._user_id = Some(id);
    self
  }

  pub fn role(mut self, id: i32) -> Self {
    self._role_id = Some(id);
    self
  }

  pub fn org(mut self, id: i32) -> Self {
    self._org_id = Some(id);
    self
  }

  pub fn owner(self, owner: &UserContext) -> Self {
    let mut access =
      self.user(owner.profile.id).role(owner.profile.role.id).org(owner.profile.organization.id);

    if owner.profile.is_admin {
      access = access.admin();
    }

    access
  }

  pub fn admin(mut self) -> Self {
    self._is_admin = true;
    self
  }

  pub fn alias(mut self, alias: Alias) -> Self {
    self._alias = Some(alias);
    self
  }

  pub fn column(mut self, col: SimpleExpr) -> Self {
    self._cols = vec![col];
    self
  }

  pub fn columns(mut self, cols: Vec<SimpleExpr>) -> Self {
    self._cols = cols;
    self
  }

  pub fn join(mut self, jt: JoinType, rel: RelationDef) -> Self {
    self._joins.push((jt, rel));
    self
  }

  pub fn apply<E: EntityTrait>(self, mut query: Select<E>) -> Select<E> {
    let cond = self.condition();

    for (jt, rel) in self._joins.into_iter() {
      query = query.join(jt, rel);
    }

    query.filter(cond)
  }

  pub fn condition(&self) -> Condition {
    let ap_alias = self._unwrap_alias();
    let mut sub = SelectStatement::new();
    sub
      .expr(Expr::val(1))
      .from_as(Ap.into_table_ref(), ap_alias.clone())
      .and_where(self._build_object_conditions(ap_alias.clone()).into())
      .and_where(self._build_user_conditions(ap_alias.clone()).into())
      .and_where(self._build_ns_conditions(ap_alias.clone()).into())
      .and_where(self._build_action_condition(ap_alias.clone()).into());

    Condition::all().add(Expr::exists(sub))
  }

  pub fn find(self) -> Select<Ap> {
    Ap::find().filter(self._build_object_conditions())
  }

  pub async fn count<'db, C: ConnectionTrait>(self, db: &'db C) -> u64 {
    self.find().count(db).await.unwrap_or(0)
  }

  pub async fn count_with<'db, E, C>(self, query: Select<E>, db: &'db C) -> u64
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    self.apply(query).count(db).await.unwrap_or(0)
  }

  pub async fn check<'db, C: ConnectionTrait>(self, db: &'db C) -> Result<(), SumidaError> {
    if self.find().count(db).await.unwrap_or(0) > 0 {
      Ok(())
    } else {
      Err(SumidaError::NoServiceAvailable)
    }
  }

  pub async fn check_with<'db, E, C>(self, query: Select<E>, db: &'db C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    if self.apply(query).count(db).await.unwrap_or(0) > 0 {
      Ok(())
    } else {
      Err(SumidaError::NoServiceAvailable)
    }
  }
}

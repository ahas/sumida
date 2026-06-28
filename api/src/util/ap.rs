use std::mem;

use entity::access_policies::{ActiveModel as ApAm, Column as ApCol, Entity as ApEntity, Model as ApModel};
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::ActiveValue::NotSet;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::sea_query::{
  Alias, Expr, Func, IntoColumnRef, IntoIden, IntoTableRef, Query, SelectStatement, SimpleExpr,
};
use sea_orm::{
  ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DbErr, DeleteResult, DerivePartialModel,
  EntityName, EntityTrait, FromQueryResult, IntoSimpleExpr, JoinType, PaginatorTrait, QueryFilter,
  QuerySelect, QueryTrait, Select, Set, Statement,
};
use serde::{Deserialize, Serialize};

fn _build_object_condition<C: IntoColumnRef>(
  objects: &[String],
  exprs: &[SimpleExpr],
  in_subqueries: &[SimpleExpr],
  col: C,
) -> Condition {
  let col = Expr::col(col);
  let mut cond = Condition::any().add(col.clone().eq("*"));

  for object in objects {
    cond = cond.add(col.clone().eq(object.clone()));
  }

  for expr in exprs {
    cond = cond.add(col.clone().eq(expr.clone()));
  }

  // object_in_subquery: each entry is a COMPLETE boolean comparison expression
  // (e.g. `"ap"."object" = ANY(SELECT ...)`) built by the caller — added directly as an OR branch.
  // It must NOT be wrapped with `col.eq(..)`, which would render `col = (ANY(...))` (invalid SQL).
  // See `util::org::organization_ancestors_subquery`.
  for sub in in_subqueries {
    cond = cond.add(sub.clone());
  }

  cond
}

fn _build_namespace_condition<C: IntoColumnRef>(namespace: &str, col: C) -> Condition {
  let col = Expr::col(col);
  Condition::any() //
    .add(col.clone().eq(namespace))
    .add(col.eq("*"))
}

fn _build_user_conditions<C: IntoColumnRef>(users: &[ApUser], col: C) -> Condition {
  let col = Expr::col(col);
  let mut condition = Condition::any().add(col.clone().eq("*"));

  for user in users {
    condition = condition.add(col.clone().eq(user.to_string()));
  }

  condition
}

fn _build_action_condition<C: IntoColumnRef>(actions: &[String], col: C) -> Condition {
  let col = Expr::col(col);
  let actions = actions.to_vec();

  Condition::any() //
    .add(col.clone().contains(Expr::val(actions).cast_as("varchar[]")))
    .add(col.contains(Expr::val(vec![String::from("*")]).cast_as("varchar[]")))
}

fn _build_users(users: &[ApUser]) -> Vec<String> {
  users.iter().map(|x| x.to_string()).collect()
}

pub trait ApValue: Clone {
  fn to_vec(self) -> Vec<String>;
}

impl ApValue for &str {
  fn to_vec(self) -> Vec<String> {
    vec![self.to_string()]
  }
}

impl ApValue for String {
  fn to_vec(self) -> Vec<String> {
    vec![self]
  }
}

impl ApValue for Vec<String> {
  fn to_vec(self) -> Vec<String> {
    self
  }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApUser {
  Admin,
  Org(i32),
  Role(i32),
  User(i32),
  ApiKey(String),
  Unknown,
}

impl std::fmt::Display for ApUser {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ApUser::Admin => f.write_str("user:admin"),
      ApUser::Org(id) => write!(f, "org:{id}"),
      ApUser::Role(id) => write!(f, "role:{id}"),
      ApUser::User(id) => write!(f, "user:{id}"),
      ApUser::ApiKey(key) => write!(f, "api:{key}"),
      ApUser::Unknown => f.write_str("user:unknown"),
    }
  }
}
pub trait ApGuard {
  fn object<O: ToString>(self, object: O) -> Self;
  fn column<C: IntoSimpleExpr + Clone>(self, col: C) -> Self;
  fn expr<E: IntoSimpleExpr + Clone>(self, expr: E) -> Self;
  fn join<S: IntoIden + Clone + 'static, T: IntoIden + Clone + 'static>(self, entity: S, column: T)
  -> Self;
  /// OR branch that passes when `ap.object` matches a raw `ANY(SELECT ...)` or equivalent SimpleExpr.
  /// An escape hatch where the caller injects hand-written SQL for a dynamic set like org tree ancestors.
  fn object_in_subquery(self, sub: SimpleExpr) -> Self;
  fn action<T: ApValue>(self, action: T) -> Self;
  fn user(self, user: ApUser) -> Self;
  fn who(self, session: &UserSession) -> Self;
  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E>;
  fn check_allowed<C: ConnectionTrait>(self, db: &C) -> impl Future<Output = bool>;
  fn try_check_allowed<C: ConnectionTrait>(self, db: &C) -> impl Future<Output = Result<(), SumidaError>>;
  fn can_access<E: EntityTrait, C: ConnectionTrait>(
    self,
    query: Select<E>,
    db: &C,
  ) -> impl Future<Output = bool>;
  fn try_access<E: EntityTrait, C: ConnectionTrait>(
    self,
    query: Select<E>,
    db: &C,
  ) -> impl Future<Output = Result<(), SumidaError>>;
}

#[derive(Clone, Debug, Default)]
pub struct ApCondition {
  namespace: String,
  actions: Vec<String>,
  objects: Vec<String>,
  exprs: Vec<SimpleExpr>,
  object_in_subqueries: Vec<SimpleExpr>,
  users: Vec<ApUser>,
  alias: Option<Alias>,
}

impl ApCondition {
  pub fn new<S: ToString>(namespace: S) -> Self {
    Self {
      namespace: namespace.to_string(),
      actions: Vec::new(),
      objects: Vec::new(),
      exprs: Vec::new(),
      object_in_subqueries: Vec::new(),
      users: Vec::new(),
      alias: None,
    }
  }

  pub fn alias(mut self, alias: Alias) -> Self {
    self.alias = Some(alias);
    self
  }

  fn unwrap_alias(&self) -> Alias {
    self.alias.clone().unwrap_or_else(|| Alias::new("ap"))
  }

  /// `coalesce(<alias>.has_allow, false)` — a boolean expr usable in a WHERE/condition.
  fn has_allow_expr(&self, alias: &Alias) -> SimpleExpr {
    Expr::expr(Func::coalesce([
      Expr::col((alias.clone(), Alias::new("has_allow"))).into_simple_expr(),
      Expr::val(false).into_simple_expr(),
    ]))
    .into_simple_expr()
  }

  /// `coalesce(<alias>.has_deny, false)` — a boolean expr usable in a WHERE/condition.
  fn has_deny_expr(&self, alias: &Alias) -> SimpleExpr {
    Expr::expr(Func::coalesce([
      Expr::col((alias.clone(), Alias::new("has_deny"))).into_simple_expr(),
      Expr::val(false).into_simple_expr(),
    ]))
    .into_simple_expr()
  }

  fn condition(&self, alias: Alias) -> Condition {
    Condition::all() //
      .add(self.has_allow_expr(&alias))
      .add(self.has_deny_expr(&alias).not())
  }

  /// `(has_allow, has_deny)` — presence of allow/deny policies matching this condition, evaluated
  /// separately so callers (e.g. `ApOr` global-deny) can combine them across branches.
  pub async fn effects<C: ConnectionTrait>(&self, db: &C) -> (bool, bool) {
    let ap_alias = Alias::new(ApEntity.table_name());
    let col_effect = Expr::col((ap_alias.clone(), ApCol::Effect));
    let condition = Condition::all()
      .add(_build_namespace_condition(&self.namespace, (ap_alias.clone(), ApCol::Namespace)))
      .add(_build_object_condition(
        &self.objects,
        &self.exprs,
        &self.object_in_subqueries,
        (ap_alias.clone(), ApCol::Object),
      ))
      .add(_build_action_condition(&self.actions, (ap_alias.clone(), ApCol::Action)))
      .add(_build_user_conditions(&self.users, (ap_alias.clone(), ApCol::User)));

    let query = ApEntity::find().filter(condition);
    let allowed = query.clone().filter(col_effect.clone().eq("allow")).count(db).await.unwrap_or(0) > 0;
    let denied = query.filter(col_effect.eq("deny")).count(db).await.unwrap_or(0) > 0;

    (allowed, denied)
  }

  pub fn with_lateral_join<E: EntityTrait, A: IntoIden>(
    &self,
    mut query: Select<E>,
    alias: A,
  ) -> Select<E> {
    let ap_alias = self.unwrap_alias();

    let col_ns = (ap_alias.clone(), ApCol::Namespace);
    let col_obj = (ap_alias.clone(), ApCol::Object);
    let col_action = (ap_alias.clone(), ApCol::Action);
    let col_user = (ap_alias.clone(), ApCol::User);
    let col_effect = Expr::col((ap_alias.clone(), ApCol::Effect));

    let stmt = SelectStatement::new()
      .expr_as(Expr::expr(Func::cust("bool_or").arg(col_effect.clone().eq("allow"))), "has_allow")
      .expr_as(Expr::expr(Func::cust("bool_or").arg(col_effect.eq("deny"))), "has_deny")
      .from_as(ApEntity.into_table_ref(), ap_alias.clone())
      .and_where(_build_namespace_condition(&self.namespace, col_ns).into())
      .and_where(
        _build_object_condition(&self.objects, &self.exprs, &self.object_in_subqueries, col_obj).into(),
      )
      .and_where(_build_action_condition(&self.actions, col_action).into())
      .and_where(_build_user_conditions(&self.users, col_user).into())
      .to_owned();

    QueryFilter::query(&mut query) //
      .join_lateral(JoinType::LeftJoin, stmt, alias, Expr::cust("TRUE"));

    query
  }
}

impl ApGuard for ApCondition {
  fn object<O: ToString>(mut self, object: O) -> Self {
    self.objects.push(object.to_string());
    self
  }

  fn column<C: IntoSimpleExpr + Clone>(mut self, col: C) -> Self {
    self.exprs.push(col.into_simple_expr().cast_as("varchar"));
    self
  }

  fn expr<E: IntoSimpleExpr + Clone>(mut self, expr: E) -> Self {
    self.exprs.push(expr.into_simple_expr());
    self
  }

  fn object_in_subquery(mut self, sub: SimpleExpr) -> Self {
    self.object_in_subqueries.push(sub);
    self
  }

  fn join<S, T>(mut self, entity: S, col: T) -> Self
  where
    S: IntoIden + Clone + 'static,
    T: IntoIden + Clone + 'static,
  {
    self.exprs.push(Expr::col((entity, col)).cast_as("varchar"));
    self
  }

  fn action<T: ApValue>(mut self, action: T) -> Self {
    self.actions.append(&mut action.to_vec());
    self
  }

  fn user(mut self, user: ApUser) -> Self {
    self.users.push(user);
    self
  }

  fn who(mut self, session: &UserSession) -> Self {
    self.users.clear();

    if let Some(key) = &session.profile.api_key {
      self.users.push(ApUser::ApiKey(key.to_owned()));
    } else {
      self.users.push(ApUser::User(session.profile.id));
      self.users.push(ApUser::Role(session.profile.role.id));
      self.users.push(ApUser::Org(session.profile.organization.id));

      if session.profile.is_admin {
        self.users.push(ApUser::Admin);
      }
    }

    self
  }

  fn apply<S: EntityTrait>(self, query: Select<S>) -> Select<S> {
    let alias = "ap_check";
    self.with_lateral_join(query, alias).filter(self.condition(Alias::new(alias)))
  }

  async fn check_allowed<C>(self, db: &C) -> bool
  where
    C: ConnectionTrait,
  {
    let (allow, deny) = self.effects(db).await;
    allow && !deny
  }

  async fn try_check_allowed<C: ConnectionTrait>(self, db: &C) -> Result<(), SumidaError> {
    if self.check_allowed(db).await { Ok(()) } else { Err(SumidaError::NoAccessPermission) }
  }

  async fn can_access<E, C>(self, query: Select<E>, db: &C) -> bool
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    #[derive(FromQueryResult)]
    struct CanAccess {
      exists: bool,
    }

    let ap_check_alias = Alias::new("ap_check");
    let subquery = self
      .with_lateral_join(query.select_only().expr(Expr::val(1)), ap_check_alias.clone())
      .filter(self.condition(ap_check_alias));
    let stmt = Query::select().expr(Expr::exists(subquery.into_query())).take();

    let builder = db.get_database_backend();
    let stmt = builder.build(&stmt);

    let Some(result) = CanAccess::find_by_statement(stmt).one(db).await.ok().flatten() else {
      return false;
    };

    result.exists
  }

  async fn try_access<E, C>(self, query: Select<E>, db: &C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    if !self.can_access(query, db).await {
      return Err(SumidaError::NoAccessPermission);
    }

    Ok(())
  }
}

#[derive(Clone, Debug, Default)]
pub struct ApChain {
  children: Vec<ApCondition>,
}

impl ApChain {
  pub fn new() -> Self {
    Self {
      children: Vec::new(),
    }
  }

  pub fn push(mut self, child: ApCondition) -> Self {
    self.children.push(child);
    self
  }

  pub fn push_if<V, F>(mut self, value: Option<V>, mut cb: F) -> Self
  where
    V: ToString,
    F: FnMut(V) -> ApCondition,
  {
    if let Some(v) = value {
      self.children.push(cb(v));
    }

    self
  }
}

impl ApGuard for ApChain {
  fn object<O: ToString>(mut self, object: O) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).object(object.to_string());
    }

    self
  }

  fn column<C: IntoSimpleExpr + Clone>(mut self, col: C) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).column(col.clone());
    }

    self
  }

  fn expr<E: IntoSimpleExpr + Clone>(mut self, expr: E) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).expr(expr.clone());
    }

    self
  }

  fn object_in_subquery(mut self, sub: SimpleExpr) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).object_in_subquery(sub.clone());
    }

    self
  }

  fn join<S, T>(mut self, entity: S, col: T) -> Self
  where
    S: IntoIden + Clone + 'static,
    T: IntoIden + Clone + 'static,
  {
    for child in &mut self.children {
      *child = mem::take(child).join(entity.clone(), col.clone());
    }

    self
  }

  fn action<V: ApValue>(mut self, action: V) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).action(action.clone());
    }

    self
  }

  fn user(mut self, ap_user: ApUser) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).user(ap_user.clone());
    }

    self
  }

  fn who(mut self, session: &UserSession) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).who(session);
    }

    self
  }

  fn apply<E: EntityTrait>(self, mut query: Select<E>) -> Select<E> {
    for (i, child) in self.children.iter().enumerate() {
      let alias = Alias::new(format!("ap_check_{i}"));
      query = child.with_lateral_join(query, alias.clone()).filter(child.condition(alias));
    }

    query
  }

  async fn check_allowed<C>(self, db: &C) -> bool
  where
    C: ConnectionTrait,
  {
    for child in self.children {
      if !child.check_allowed(db).await {
        return false;
      }
    }

    true
  }

  async fn try_check_allowed<C: ConnectionTrait>(self, db: &C) -> Result<(), SumidaError> {
    if self.check_allowed(db).await { Ok(()) } else { Err(SumidaError::NoAccessPermission) }
  }

  async fn can_access<E, C>(self, query: Select<E>, db: &C) -> bool
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    for child in self.children {
      if !child.can_access(query.clone(), db).await {
        return false;
      }
    }

    true
  }

  async fn try_access<E, C>(self, query: Select<E>, db: &C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    for child in self.children {
      if !child.can_access(query.clone(), db).await {
        return Err(SumidaError::NoAccessPermission);
      }
    }

    Ok(())
  }
}

/// OR-combined conditions with **global-deny** semantics: allowed iff some branch has an `allow`
/// AND no branch has a `deny`. A `deny` matched by any branch blocks the whole check — a deny is
/// absolute and cannot be circumvented through another (e.g. ownership) branch.
///
/// Concretely the predicate is `(allow₀ OR allow₁ OR …) AND NOT (deny₀ OR deny₁ OR …)`:
/// - `apply(query)` lays down each child's LATERAL JOIN (`ap_or_<i>`) and filters on that predicate.
/// - `check_allowed`/`can_access` evaluate the same predicate.
#[derive(Clone, Debug, Default)]
pub struct ApOr {
  children: Vec<ApCondition>,
}

impl ApOr {
  pub fn new() -> Self {
    Self {
      children: Vec::new(),
    }
  }

  pub fn push(mut self, child: ApCondition) -> Self {
    self.children.push(child);
    self
  }

  pub fn push_if<V, F>(mut self, value: Option<V>, mut cb: F) -> Self
  where
    V: ToString,
    F: FnMut(V) -> ApCondition,
  {
    if let Some(v) = value {
      self.children.push(cb(v));
    }

    self
  }
}

impl ApGuard for ApOr {
  fn object<O: ToString>(mut self, object: O) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).object(object.to_string());
    }
    self
  }

  fn column<C: IntoSimpleExpr + Clone>(mut self, col: C) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).column(col.clone());
    }
    self
  }

  fn expr<E: IntoSimpleExpr + Clone>(mut self, expr: E) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).expr(expr.clone());
    }
    self
  }

  fn object_in_subquery(mut self, sub: SimpleExpr) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).object_in_subquery(sub.clone());
    }
    self
  }

  fn join<S, T>(mut self, entity: S, col: T) -> Self
  where
    S: IntoIden + Clone + 'static,
    T: IntoIden + Clone + 'static,
  {
    for child in &mut self.children {
      *child = mem::take(child).join(entity.clone(), col.clone());
    }
    self
  }

  fn action<V: ApValue>(mut self, action: V) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).action(action.clone());
    }
    self
  }

  fn user(mut self, ap_user: ApUser) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).user(ap_user.clone());
    }
    self
  }

  fn who(mut self, session: &UserSession) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).who(session);
    }
    self
  }

  fn apply<E: EntityTrait>(self, mut query: Select<E>) -> Select<E> {
    // Global deny: (any branch allows) AND NOT (any branch denies).
    let mut allow_any: Option<SimpleExpr> = None;
    let mut deny_any: Option<SimpleExpr> = None;

    for (i, child) in self.children.iter().enumerate() {
      let alias = Alias::new(format!("ap_or_{i}"));
      query = child.with_lateral_join(query, alias.clone());
      let allow = child.has_allow_expr(&alias);
      let deny = child.has_deny_expr(&alias);
      allow_any = Some(allow_any.map_or(allow.clone(), |acc| acc.or(allow)));
      deny_any = Some(deny_any.map_or(deny.clone(), |acc| acc.or(deny)));
    }

    let allow_any = allow_any.unwrap_or_else(|| Expr::val(false).into());
    let mut cond = Condition::all().add(allow_any);
    if let Some(deny_any) = deny_any {
      cond = cond.add(deny_any.not());
    }

    QueryFilter::filter(query, cond)
  }

  async fn check_allowed<C>(self, db: &C) -> bool
  where
    C: ConnectionTrait,
  {
    let mut any_allow = false;
    let mut any_deny = false;
    for child in &self.children {
      let (allow, deny) = child.effects(db).await;
      any_allow |= allow;
      any_deny |= deny;
    }
    any_allow && !any_deny
  }

  async fn try_check_allowed<C: ConnectionTrait>(self, db: &C) -> Result<(), SumidaError> {
    if self.check_allowed(db).await { Ok(()) } else { Err(SumidaError::NoAccessPermission) }
  }

  async fn can_access<E, C>(self, query: Select<E>, db: &C) -> bool
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    #[derive(FromQueryResult)]
    struct CanAccess {
      exists: bool,
    }

    // EXISTS over the row(s) passing the global-deny OR predicate (built by `apply`).
    let filtered = self.apply(query.select_only().expr(Expr::val(1)));
    let stmt = Query::select().expr(Expr::exists(filtered.into_query())).take();
    let stmt = db.get_database_backend().build(&stmt);

    CanAccess::find_by_statement(stmt)
      .one(db)
      .await
      .ok()
      .flatten()
      .map(|r| r.exists)
      .unwrap_or(false)
  }

  async fn try_access<E, C>(self, query: Select<E>, db: &C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
  {
    if self.can_access(query, db).await {
      Ok(())
    } else {
      Err(SumidaError::NoAccessPermission)
    }
  }
}

/// `ApOr::new()` shortcut.
pub fn or() -> ApOr {
  ApOr::new()
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "ApEntity", alias = "ap_allow", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct ApForUser {
  pub namespace: String,
  pub object: Vec<String>,
  pub action: Vec<String>,
  pub effect: String,
}

/// Prevents a non-admin from granting a dangerous policy that leads directly to self-elevation to super-admin.
///
/// Rejects when `effect="allow"` and any of `namespace`/`object`/`action`/`user` is a wildcard (`*`).
/// Passes for an admin or a `deny` policy (a deny wildcard does not broaden permissions).
/// For paths where `user` is fixed to `role:N`/`api:K`, such as policy injection from role/api_key,
/// pass an empty string for `user` (it is not a wildcard, so it is ignored).
pub fn assert_policy_within_authority(
  is_admin: bool,
  namespace: &str,
  objects: &[String],
  actions: &[String],
  user: &str,
  effect: &str,
) -> Result<(), SumidaError> {
  if is_admin || effect != "allow" {
    return Ok(());
  }

  let has_wildcard =
    namespace == "*" || user == "*" || objects.iter().any(|o| o == "*") || actions.iter().any(|a| a == "*");

  if has_wildcard {
    return Err(SumidaError::NoAccessPermission);
  }

  Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct FindApForUsers {
  users: Vec<ApUser>,
}

impl FindApForUsers {
  pub fn new() -> Self {
    Self { users: Vec::new() }
  }

  pub fn user(mut self, user: ApUser) -> Self {
    self.users.push(user);
    self
  }

  pub fn who(mut self, session: &UserSession) -> Self {
    if let Some(key) = &session.profile.api_key {
      self.users.push(ApUser::ApiKey(key.to_owned()));
    } else {
      self.users.push(ApUser::User(session.profile.id));
      self.users.push(ApUser::Role(session.profile.role.id));
      self.users.push(ApUser::Org(session.profile.organization.id));

      if session.profile.is_admin {
        self.users.push(ApUser::Admin);
      }
    }

    self
  }

  pub async fn find_all<C: ConnectionTrait>(self, db: &C) -> Result<Vec<ApForUser>, DbErr> {
    let result = ApForUser::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      r#"
      SELECT
        ap."namespace" AS "namespace"
        , ARRAY_AGG(DISTINCT ap."object") AS "object"
        , ap."effect" AS "effect"
        , ARRAY_AGG(DISTINCT act) AS "action"
      FROM access_policies ap
        , LATERAL UNNEST(ap."action") AS "act"
      WHERE ap."user" = ANY($1)
      GROUP BY
        ap."namespace"
        , ap."object"
        , ap."effect"
      "#,
      [_build_users(&self.users).into()],
    ))
    .all(db)
    .await?;

    Ok(result)
  }

  #[inline(always)]
  async fn _clear<C: ConnectionTrait>(users: &Vec<String>, db: &C) -> Result<DeleteResult, DbErr> {
    let result = ApEntity::delete_many().filter(ApCol::User.is_in(users)).exec(db).await?;

    Ok(result)
  }

  #[inline(always)]
  async fn _save<C: ConnectionTrait>(
    users: &Vec<String>,
    user_aps: Vec<ApForUser>,
    db: &C,
  ) -> Result<(), DbErr> {
    for user in users {
      for user_ap in &user_aps {
        for object in &user_ap.object {
          let am = ApAm {
            uuid: NotSet,
            namespace: Set(user_ap.namespace.clone()),
            object: Set(object.clone()),
            action: Set(user_ap.action.clone()),
            effect: Set(user_ap.effect.clone()),
            user: Set(user.clone()),
            ..Default::default()
          };

          am.save(db).await?;
        }
      }
    }

    Ok(())
  }

  pub async fn clear<C: ConnectionTrait>(self, db: &C) -> Result<DeleteResult, DbErr> {
    Self::_clear(&_build_users(&self.users), db).await
  }

  pub async fn save<C: ConnectionTrait>(self, user_aps: Vec<ApForUser>, db: &C) -> Result<(), DbErr> {
    Self::_save(&_build_users(&self.users), user_aps, db).await
  }

  pub async fn update<C: ConnectionTrait>(self, user_aps: Vec<ApForUser>, db: &C) -> Result<(), DbErr> {
    let users = _build_users(&self.users);
    Self::_clear(&users, db).await?;
    Self::_save(&users, user_aps, db).await?;

    Ok(())
  }
}

pub struct FindUsersOfAp {
  namespace: String,
  actions: Vec<String>,
  objects: Vec<String>,
  exprs: Vec<SimpleExpr>,
}

impl FindUsersOfAp {
  pub fn new<S: ToString>(namespace: S) -> Self {
    Self {
      namespace: namespace.to_string(),
      actions: Vec::new(),
      objects: Vec::new(),
      exprs: Vec::new(),
    }
  }

  pub fn object<O: ToString>(mut self, object: O) -> Self {
    self.objects.push(object.to_string());
    self
  }

  pub fn expr(mut self, expr: SimpleExpr) -> Self {
    self.exprs.push(expr);
    self
  }

  pub fn join<S, T>(mut self, entity: S, col: T) -> Self
  where
    S: IntoIden + Clone + 'static,
    T: IntoIden + Clone + 'static,
  {
    self.exprs.push(Expr::col((entity, col)).cast_as("varchar"));
    self
  }

  pub fn action<T: ApValue>(mut self, action: T) -> Self {
    self.actions.append(&mut action.to_vec());
    self
  }

  pub async fn all<C>(self, db: &C) -> Vec<ApUser>
  where
    C: ConnectionTrait,
  {
    #[derive(Clone, Debug, FromQueryResult)]
    struct ApUserColumnOnly {
      user: String,
    }

    impl From<ApUserColumnOnly> for ApUser {
      fn from(value: ApUserColumnOnly) -> Self {
        let user = if value.user == "user:admin" {
          Some(ApUser::Admin)
        } else if value.user.starts_with("org:") {
          value.user.strip_prefix("org:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::Org)
        } else if value.user.starts_with("role:") {
          value.user.strip_prefix("role:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::Role)
        } else if value.user.starts_with("user:") {
          value.user.strip_prefix("user:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::User)
        } else if value.user.starts_with("api:") {
          value.user.strip_prefix("api:").map(|x| ApUser::ApiKey(x.to_string()))
        } else {
          Some(ApUser::Unknown)
        };

        user.unwrap_or(ApUser::Unknown)
      }
    }

    let ap_alias = Alias::new(ApEntity.table_name());

    let mut cond = Condition::all()
      .add(Expr::col((ap_alias.clone(), ApCol::Effect)).eq("allow"))
      .add(_build_namespace_condition(&self.namespace, (ap_alias.clone(), ApCol::Namespace)));

    if !self.objects.is_empty() {
      cond = cond.add(_build_object_condition(
        &self.objects,
        &self.exprs,
        &[],
        (ap_alias.clone(), ApCol::Object),
      ));
    }

    if !self.actions.is_empty() {
      cond = cond.add(_build_action_condition(&self.actions, (ap_alias.clone(), ApCol::Action)));
    }

    let rows = ApEntity::find()
      .select_only()
      .expr(Expr::col((ap_alias.clone(), ApCol::User)))
      .filter(cond)
      .into_model::<ApUserColumnOnly>()
      .all(db)
      .await
      .unwrap_or_default();

    rows.into_iter().map(|x| x.into()).collect()
  }
}

#[derive(Clone, Debug, Default)]
pub struct EditAp {
  namespace: Option<String>,
  object: Option<String>,
  action: Option<Vec<String>>,
  users: Vec<ApUser>,
  effects: Vec<String>,
}

impl EditAp {
  pub fn new() -> Self {
    Self {
      namespace: None,
      action: None,
      object: None,
      users: Vec::new(),
      effects: Vec::new(),
    }
  }

  pub fn namespace<T: ToString>(mut self, namespace: T) -> Self {
    self.namespace = Some(namespace.to_string());
    self
  }

  pub fn object<T: ToString>(mut self, object: T) -> Self {
    self.object = Some(object.to_string());
    self
  }

  pub fn action(mut self, action: Vec<String>) -> Self {
    self.action = Some(action);
    self
  }

  pub fn user(mut self, user: ApUser) -> Self {
    self.users.push(user);
    self
  }

  pub fn who(mut self, session: &UserSession) -> Self {
    self.users.clear();

    if let Some(key) = &session.profile.api_key {
      self.users.push(ApUser::ApiKey(key.to_owned()));
    } else {
      self.users.push(ApUser::User(session.profile.id));
      self.users.push(ApUser::Role(session.profile.role.id));
      self.users.push(ApUser::Org(session.profile.organization.id));

      if session.profile.is_admin {
        self.users.push(ApUser::Admin);
      }
    }

    self
  }

  pub fn effect<E: AsRef<str>>(mut self, effect: E) -> Self {
    self.effects.push(effect.as_ref().to_string());
    self
  }

  pub async fn create<C: ConnectionTrait>(self, db: &C) -> anyhow::Result<Vec<ApModel>> {
    let mut result = Vec::new();

    let Some(namespace) = self.namespace else {
      anyhow::bail!("namespace must be set");
    };
    let Some(object) = self.object else {
      anyhow::bail!("object must be set");
    };
    let Some(action) = self.action else {
      anyhow::bail!("action must be set");
    };

    for effect in self.effects {
      for user in &self.users {
        let id = ApEntity::insert(ApAm {
          user: Set(user.to_string()),
          effect: Set(effect.clone()),
          namespace: Set(namespace.clone()),
          object: Set(object.clone()),
          action: Set(action.clone()),
          ..Default::default()
        })
        .exec(db)
        .await?
        .last_insert_id;

        if let Some(model) = ApEntity::find_by_id(id).one(db).await? {
          result.push(model);
        }
      }
    }

    Ok(result)
  }

  pub async fn update<T: ToString, C: ConnectionTrait>(self, effect: T, db: &C) -> anyhow::Result<()> {
    ApEntity::update_many() //
      .apply_if(self.namespace, |q, v| q.filter(ApCol::Namespace.eq(v)))
      .apply_if(self.object, |q, v| q.filter(ApCol::Object.eq(v)))
      .apply_if(self.action, |q, v| {
        q.filter(Expr::col(ApCol::Action).eq(Expr::val(v).cast_as("varchar[]")))
      })
      .apply_if(if self.users.is_empty() { None } else { Some(self.users) }, |q, v| {
        q.filter(ApCol::User.is_in(v.iter().map(|x| x.to_string())))
      })
      .set(ApAm {
        effect: Set(effect.to_string()),
        ..Default::default()
      })
      .exec(db)
      .await?;

    Ok(())
  }

  pub async fn delete<C: ConnectionTrait>(self, db: &C) -> anyhow::Result<()> {
    ApEntity::delete_many() //
      .apply_if(self.namespace, |q, v| q.filter(ApCol::Namespace.eq(v)))
      .apply_if(self.object, |q, v| q.filter(ApCol::Object.eq(v)))
      .apply_if(self.action, |q, v| {
        q.filter(Expr::col(ApCol::Action).eq(Expr::val(v).cast_as("varchar[]")))
      })
      .apply_if(if self.users.is_empty() { None } else { Some(self.users) }, |q, v| {
        q.filter(ApCol::User.is_in(v.iter().map(|x| x.to_string())))
      })
      .exec(db)
      .await?;

    Ok(())
  }
}

pub fn ns<S: ToString>(namespace: S) -> ApCondition {
  ApCondition::new(namespace)
}

pub fn chain() -> ApChain {
  ApChain::new()
}

pub fn for_users() -> FindApForUsers {
  FindApForUsers::new()
}

pub fn find_users<S: ToString>(namespace: S) -> FindUsersOfAp {
  FindUsersOfAp::new(namespace)
}

pub fn edit() -> EditAp {
  EditAp::new()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn vs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
  }

  // assert_policy_within_authority (H-2): block a non-admin's self-elevation to super-admin
  #[test]
  fn admin_bypasses_wildcard_allow() {
    assert!(assert_policy_within_authority(true, "*", &vs(&["*"]), &vs(&["*"]), "*", "allow").is_ok());
  }

  #[test]
  fn deny_with_wildcard_is_allowed() {
    // deny does not broaden permissions, so allow it even with a wildcard.
    assert!(assert_policy_within_authority(false, "*", &vs(&["*"]), &vs(&["*"]), "*", "deny").is_ok());
  }

  #[test]
  fn non_admin_concrete_allow_is_ok() {
    assert!(
      assert_policy_within_authority(false, "my:ns", &vs(&["obj"]), &vs(&["read"]), "user:1", "allow")
        .is_ok()
    );
  }

  #[test]
  fn non_admin_wildcard_namespace_allow_rejected() {
    assert!(
      assert_policy_within_authority(false, "*", &vs(&["obj"]), &vs(&["read"]), "user:1", "allow").is_err()
    );
  }

  #[test]
  fn non_admin_wildcard_object_allow_rejected() {
    assert!(
      assert_policy_within_authority(false, "my:ns", &vs(&["*"]), &vs(&["read"]), "user:1", "allow")
        .is_err()
    );
  }

  #[test]
  fn non_admin_wildcard_action_allow_rejected() {
    assert!(
      assert_policy_within_authority(false, "my:ns", &vs(&["obj"]), &vs(&["*"]), "user:1", "allow")
        .is_err()
    );
  }

  #[test]
  fn non_admin_wildcard_user_allow_rejected() {
    assert!(
      assert_policy_within_authority(false, "my:ns", &vs(&["obj"]), &vs(&["read"]), "*", "allow").is_err()
    );
  }
}

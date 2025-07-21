use core::session::UserContext;
use entity::access_policies::{Column as ApCol, Entity as ApEntity};
use error::SumidaError;
use sea_orm::EntityName;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::IntoTableRef;
use sea_orm::sea_query::SelectStatement;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::{
  Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Select,
  sea_query::{Expr, SimpleExpr},
};

pub struct Ap {
  _namespace: String,
  _object: Option<SimpleExpr>,
  _actions: Vec<String>,
  _user_id: Option<i32>,
  _role_id: Option<i32>,
  _org_id: Option<i32>,
  _alias: Option<Alias>,
  _is_admin: bool,
}

impl Ap {
  pub fn new<P: AsRef<str>>(namespace: P) -> Self {
    Self {
      _namespace: namespace.as_ref().to_owned(),
      _object: None,
      _actions: Vec::new(),
      _user_id: None,
      _role_id: None,
      _org_id: None,
      _alias: None,
      _is_admin: false,
    }
  }

  pub(crate) fn _build_object_conditions(&self, ap_alias: Alias) -> Condition {
    let col_obj = Expr::col((ap_alias, ApCol::Object));
    let mut cond = Condition::any().add(col_obj.clone().eq("*"));

    if let Some(object) = &self._object {
      cond = cond.add(col_obj.eq(object.clone()));
    }

    cond
  }

  pub(crate) fn _build_user_conditions(&self, ap_alias: Alias) -> Condition {
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

  pub(crate) fn _build_namespace_condition(&self, ap_alias: Alias) -> Condition {
    let col_ns = Expr::col((ap_alias.clone(), ApCol::Namespace));
    Condition::any() //
      .add(col_ns.clone().eq(&self._namespace))
      .add(col_ns.eq("*"))
  }

  pub(crate) fn _build_action_condition(&self, ap_alias: Alias) -> Condition {
    let col_action = Expr::col((ap_alias.clone(), ApCol::Action));
    Condition::any() //
      // .add(col_action.clone().binary(PgBinOper::Overlap, Expr::val(self._actions.clone())))
      .add(col_action.clone().contains(Expr::val(self._actions.clone()).cast_as("varchar[]")))
      .add(col_action.contains(Expr::val(vec![String::from("*")]).cast_as("varchar[]")))
  }

  pub(crate) fn _unwrap_alias(&self) -> Alias {
    // self._alias.clone().unwrap_or_else(|| Alias::new(ApEntity.table_name()))
    self._alias.clone().unwrap_or_else(|| Alias::new("ap"))
  }

  pub fn object(mut self, expr: SimpleExpr) -> Self {
    self._object = Some(expr);
    self
  }

  pub fn action<S: AsRef<str>>(mut self, action: S) -> Self {
    self._actions.push(action.as_ref().to_owned());
    self
  }

  pub fn actions(mut self, actions: &mut Vec<String>) -> Self {
    self._actions.append(actions);
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

  pub fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E> {
    query.filter(self.condition())
  }

  pub fn build_statement(&self, ap_alias: Alias) -> SelectStatement {
    SelectStatement::new()
      .expr(Expr::val(1))
      .from_as(ApEntity.into_table_ref(), ap_alias.clone())
      .and_where(self._build_namespace_condition(ap_alias.clone()).into())
      .and_where(self._build_object_conditions(ap_alias.clone()).into())
      .and_where(self._build_action_condition(ap_alias.clone()).into())
      .and_where(self._build_user_conditions(ap_alias.clone()).into())
      .to_owned()
  }

  pub fn build_condition(&self, ap_alias: Alias) -> Condition {
    Condition::all()
      .add(self._build_namespace_condition(ap_alias.clone()))
      .add(self._build_object_conditions(ap_alias.clone()))
      .add(self._build_action_condition(ap_alias.clone()))
      .add(self._build_user_conditions(ap_alias.clone()))
  }

  pub fn condition(&self) -> Condition {
    let ap_alias = self._unwrap_alias();
    let col_effect = Expr::col((ap_alias.clone(), ApCol::Effect));
    let stmt = self.build_statement(ap_alias.clone());

    let mut stmt_allow = stmt.clone();
    stmt_allow.and_where(col_effect.clone().eq("allow"));

    let mut stmt_deny = stmt.clone();
    stmt_deny.and_where(col_effect.eq("deny"));

    Condition::all() //
      .add(Expr::exists(stmt_allow))
      .add(Expr::exists(stmt_deny).not())
  }

  pub async fn count<'db, E, C>(self, query: Select<E>, db: &'db C) -> u64
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    PaginatorTrait::count(self.apply(query), db).await.unwrap_or(0)
  }

  pub async fn check<'db, E, C>(self, query: Select<E>, db: &'db C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    if PaginatorTrait::count(self.apply(query), db).await.unwrap_or(0) > 0 {
      Ok(())
    } else {
      Err(SumidaError::NoServiceAvailable)
    }
  }

  pub async fn check_allowed<'db, C>(self, db: &'db C) -> bool
  where
    C: ConnectionTrait,
  {
    let ap_alias = Alias::new(ApEntity.table_name());
    let col_effect = Expr::col((ap_alias.clone(), ApCol::Effect));
    let condition = self.build_condition(ap_alias.clone());
    let query = ApEntity::find().filter(condition);

    let allowed_count = query.clone().filter(col_effect.clone().eq("allow")).count(db).await.unwrap_or(0);
    let denied_count = query.clone().filter(col_effect.eq("deny")).count(db).await.unwrap_or(0);

    allowed_count > 0 && denied_count == 0
  }
}

pub struct ApChain {
  items: Vec<Ap>,
}

impl ApChain {
  pub fn new() -> Self {
    Self { items: Vec::new() }
  }

  pub fn add(mut self, ap: Ap) -> Self {
    self.items.push(ap);
    self
  }

  pub fn condition(self) -> Condition {
    let mut cond_allow = Condition::any();
    let mut cond_deny = Condition::all();

    for item in self.items {
      let ap_alias = item._unwrap_alias();
      let col_effect = Expr::col((ap_alias.clone(), ApCol::Effect));
      let stmt = item.build_statement(ap_alias.clone());

      let mut stmt_allow = stmt.clone();
      stmt_allow.and_where(col_effect.clone().eq("allow"));

      let mut stmt_deny = stmt.clone();
      stmt_deny.and_where(col_effect.clone().eq("deny"));

      cond_allow = cond_allow.add(Expr::exists(stmt_allow));
      cond_deny = cond_deny.add(Expr::exists(stmt_deny).not());
    }

    Condition::all().add(cond_allow).add(cond_deny)
  }

  pub fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E> {
    query.filter(self.condition())
  }
}

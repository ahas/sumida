use std::mem;

use entity::access_policies::{Column as ApCol, Entity as ApEntity};
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ActiveModelTrait,
  ActiveValue::NotSet,
  ColumnTrait, Condition, ConnectionTrait, DbErr, DeleteResult, DerivePartialModel, EntityName,
  EntityTrait, FromQueryResult, PaginatorTrait, QueryFilter, QuerySelect, Select, Set, Statement,
  sea_query::{
    Alias, Expr, IntoColumnRef, IntoTableRef, SelectStatement, SimpleExpr, extension::postgres::PgExpr,
  },
};
use serde::{Deserialize, Serialize};

fn _build_object_condition<C: IntoColumnRef>(
  objects: &[String],
  object_refs: &[SimpleExpr],
  col: C,
) -> Condition {
  let col = Expr::col(col);
  let mut cond = Condition::any().add(col.clone().eq("*"));

  for object in objects {
    cond = cond.add(col.clone().eq(object.clone()));
  }

  for object_ref in object_refs {
    cond = cond.add(col.clone().eq(object_ref.clone()));
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
  let actions = actions.iter().cloned().collect::<Vec<_>>();

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

impl<'a> ApValue for &'a str {
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

#[derive(Clone, Debug)]
pub enum ApUser {
  Admin,
  Org(i32),
  Role(i32),
  User(i32),
  ApiKey(String),
  Unknown,
}

impl ToString for ApUser {
  fn to_string(&self) -> String {
    match self.clone() {
      ApUser::Admin => "user:admin".to_string(),
      ApUser::Org(id) => format!("org:{id}"),
      ApUser::Role(id) => format!("role:{id}"),
      ApUser::User(id) => format!("user:{id}"),
      ApUser::ApiKey(key) => format!("api:{key}"),
      ApUser::Unknown => "user:unknown".to_string(),
    }
  }
}

pub trait ApGuard {
  fn object<O: ToString>(self, object: O) -> Self;
  fn object_ref(self, expr: SimpleExpr) -> Self;
  fn action<T: ApValue>(self, action: T) -> Self;
  fn user(self, user: ApUser) -> Self;
  fn who(self, session: &UserSession) -> Self;
  fn condition(&self) -> Condition;
  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E>;
  async fn can_access<'db, C: ConnectionTrait>(self, db: &'db C) -> bool;
  async fn try_access<'db, C: ConnectionTrait>(self, db: &'db C) -> Result<(), SumidaError>;
}

#[derive(Clone, Debug, Default)]
pub struct ApCondition {
  namespace: String,
  actions: Vec<String>,
  objects: Vec<String>,
  object_refs: Vec<SimpleExpr>,
  users: Vec<ApUser>,
  alias: Option<Alias>,
}

impl ApCondition {
  pub fn new<S: AsRef<str>>(namespace: S) -> Self {
    Self {
      namespace: namespace.as_ref().to_owned(),
      actions: Vec::new(),
      objects: Vec::new(),
      object_refs: Vec::new(),
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

  pub fn build_statement(&self, ap_alias: Alias) -> SelectStatement {
    SelectStatement::new()
      .expr(Expr::val(1))
      .from_as(ApEntity.into_table_ref(), ap_alias.clone())
      .and_where(_build_namespace_condition(&self.namespace, (ap_alias.clone(), ApCol::Namespace)).into())
      .and_where(
        _build_object_condition(&self.objects, &self.object_refs, (ap_alias.clone(), ApCol::Object)).into(),
      )
      .and_where(_build_action_condition(&self.actions, (ap_alias.clone(), ApCol::Action)).into())
      .and_where(_build_user_conditions(&self.users, (ap_alias.clone(), ApCol::User)).into())
      .to_owned()
  }

  pub fn build_condition(&self, ap_alias: Alias) -> Condition {
    Condition::all()
      .add(_build_namespace_condition(&self.namespace, (ap_alias.clone(), ApCol::Namespace)))
      .add(_build_object_condition(&self.objects, &self.object_refs, (ap_alias.clone(), ApCol::Object)))
      .add(_build_action_condition(&self.actions, (ap_alias.clone(), ApCol::Action)))
      .add(_build_user_conditions(&self.users, (ap_alias.clone(), ApCol::User)))
  }
}

impl ApGuard for ApCondition {
  fn object<O: ToString>(mut self, object: O) -> Self {
    self.objects.push(object.to_string());
    self
  }

  fn object_ref(mut self, expr: SimpleExpr) -> Self {
    self.object_refs.push(expr);
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

  fn condition(&self) -> Condition {
    let ap_alias = self.unwrap_alias();

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

  fn apply<S: EntityTrait>(self, query: Select<S>) -> Select<S> {
    query.filter(self.condition())
  }

  async fn can_access<'db, C>(self, db: &'db C) -> bool
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

  async fn try_access<'db, C>(self, db: &'db C) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    if !self.can_access(db).await {
      return Err(SumidaError::NoServiceAvailable);
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

  pub fn add(mut self, child: ApCondition) -> Self {
    self.children.push(child);
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

  fn object_ref(mut self, expr: SimpleExpr) -> Self {
    for child in &mut self.children {
      *child = mem::take(child).object_ref(expr.clone());
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

  fn condition(&self) -> Condition {
    let mut cond = Condition::all();

    for child in &self.children {
      cond = cond.add(child.condition());
    }

    cond
  }

  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E> {
    query.filter(self.condition())
  }

  async fn can_access<'db, C>(self, db: &'db C) -> bool
  where
    C: ConnectionTrait,
  {
    for child in self.children {
      if !child.can_access(db).await {
        return false;
      }
    }

    true
  }

  async fn try_access<'db, C>(self, db: &'db C) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    for child in self.children {
      if !child.can_access(db).await {
        return Err(SumidaError::NoServiceAvailable);
      }
    }

    Ok(())
  }
}

pub struct ApQuery<E>
where
  E: EntityTrait,
{
  query: Select<E>,
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::access_policies::Entity", alias = "ap_allow", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct ApForUser {
  pub namespace: String,
  pub object: Vec<String>,
  pub action: Vec<String>,
  pub effect: String,
}

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

  pub async fn find_all<'db, C: ConnectionTrait>(self, db: &'db C) -> Result<Vec<ApForUser>, DbErr> {
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
  async fn _clear<'db, C: ConnectionTrait>(users: &Vec<String>, db: &'db C) -> Result<DeleteResult, DbErr> {
    let result = ApEntity::delete_many().filter(ApCol::User.is_in(users)).exec(db).await?;

    Ok(result)
  }

  #[inline(always)]
  async fn _save<'db, C: ConnectionTrait>(
    users: &Vec<String>,
    user_aps: Vec<ApForUser>,
    db: &'db C,
  ) -> Result<(), DbErr> {
    for user in users {
      for user_ap in &user_aps {
        for object in &user_ap.object {
          let am = entity::access_policies::ActiveModel {
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

  pub async fn clear<'db, C: ConnectionTrait>(self, db: &'db C) -> Result<DeleteResult, DbErr> {
    Self::_clear(&_build_users(&self.users), db).await
  }

  pub async fn save<'db, C: ConnectionTrait>(
    self,
    user_aps: Vec<ApForUser>,
    db: &'db C,
  ) -> Result<(), DbErr> {
    Self::_save(&_build_users(&self.users), user_aps, db).await
  }

  pub async fn update<'db, C: ConnectionTrait>(
    self,
    user_aps: Vec<ApForUser>,
    db: &'db C,
  ) -> Result<(), DbErr> {
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
  object_refs: Vec<SimpleExpr>,
  alias: Option<Alias>,
}

impl FindUsersOfAp {
  pub fn new<S: AsRef<str>>(namespace: S) -> Self {
    Self {
      namespace: namespace.as_ref().to_owned(),
      actions: Vec::new(),
      objects: Vec::new(),
      object_refs: Vec::new(),
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

  pub fn object<O: ToString>(mut self, object: O) -> Self {
    self.objects.push(object.to_string());
    self
  }

  pub fn object_ref(mut self, expr: SimpleExpr) -> Self {
    self.object_refs.push(expr);
    self
  }

  pub fn action<T: ApValue>(mut self, action: T) -> Self {
    self.actions.append(&mut action.to_vec());
    self
  }

  pub async fn find_all<'db, C>(self, db: &'db C) -> Vec<ApUser>
  where
    C: ConnectionTrait,
  {
    #[derive(Clone, Debug, FromQueryResult)]
    struct ApUserColumnOnly {
      user: String,
    }

    impl Into<ApUser> for ApUserColumnOnly {
      fn into(self) -> ApUser {
        let user = if self.user == "user:admin" {
          Some(ApUser::Admin)
        } else if self.user.starts_with("org:") {
          self.user.strip_prefix("org:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::Org)
        } else if self.user.starts_with("role:") {
          self.user.strip_prefix("role:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::Role)
        } else if self.user.starts_with("user:") {
          self.user.strip_prefix("user:").and_then(|x| x.parse::<i32>().ok()).map(ApUser::User)
        } else if self.user.starts_with("api:") {
          self.user.strip_prefix("api:").map(|x| ApUser::ApiKey(x.to_string()))
        } else {
          Some(ApUser::Unknown)
        };

        user.unwrap_or(ApUser::Unknown)
      }
    }

    let ap_alias = self.unwrap_alias();
    let rows = ApEntity::find()
      .select_only()
      .expr(Expr::col((ap_alias.clone(), ApCol::User)))
      .filter(
        Condition::all()
          .add(_build_namespace_condition(&self.namespace, (ap_alias.clone(), ApCol::Namespace)))
          .add(_build_object_condition(&self.objects, &self.object_refs, (ap_alias.clone(), ApCol::Object)))
          .add(_build_action_condition(&self.actions, (ap_alias.clone(), ApCol::Action))),
      )
      .into_model::<ApUserColumnOnly>()
      .all(db)
      .await
      .unwrap_or_default();

    rows.into_iter().map(|x| x.into()).collect()
  }
}

pub fn ns<S: AsRef<str>>(namespace: S) -> ApCondition {
  ApCondition::new(namespace)
}

pub fn chain() -> ApChain {
  ApChain::new()
}

pub fn for_users() -> FindApForUsers {
  FindApForUsers::new()
}

pub fn find_users<S: AsRef<str>>(namespace: S) -> FindUsersOfAp {
  FindUsersOfAp::new(namespace)
}

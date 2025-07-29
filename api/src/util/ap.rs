use infra::session::UserContext;
use entity::access_policies::{Column as ApCol, Entity as ApEntity};
use error::SumidaError;
use sea_orm::DbErr;
use sea_orm::DeleteResult;
use sea_orm::EntityName;
use sea_orm::FromQueryResult;
use sea_orm::Statement;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::IntoTableRef;
use sea_orm::sea_query::SelectStatement;
use sea_orm::sea_query::extension::postgres::PgExpr;
use sea_orm::{
  ActiveModelTrait,
  ActiveValue::{NotSet, Set},
  ColumnTrait, Condition, ConnectionTrait, DerivePartialModel, EntityTrait, PaginatorTrait, QueryFilter,
  Select,
  sea_query::{Expr, SimpleExpr},
};
use serde::{Deserialize, Serialize};
use std::mem;

#[derive(Clone, Debug)]
pub enum ApUser {
  Admin,
  Org(i32),
  Role(i32),
  User(i32),
  ApiKey(String),
}

impl ToString for ApUser {
  fn to_string(&self) -> String {
    match self {
      ApUser::Admin => "user:admin".to_string(),
      ApUser::Org(id) => format!("org:{id}"),
      ApUser::Role(id) => format!("role:{id}"),
      ApUser::User(id) => format!("user:{id}"),
      ApUser::ApiKey(key) => format!("api:{key}"),
    }
  }
}

pub trait ApGuard {
  fn object(self, expr: SimpleExpr) -> Self;

  fn action<S: AsRef<str>>(self, action: S) -> Self;

  fn actions(self, actions: &mut Vec<String>) -> Self;

  fn user(self, ap_user: ApUser) -> Self;

  fn owner(self, owner: &UserContext) -> Self;

  fn condition(&self) -> Condition;

  fn count<'db, E, C>(self, query: Select<E>, db: &'db C) -> impl Future<Output = u64>
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync;

  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E>;

  fn check<'db, E, C>(self, query: Select<E>, db: &'db C) -> impl Future<Output = Result<(), SumidaError>>
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync;

  fn check_allowed<'db, C>(self, db: &'db C) -> impl Future<Output = bool>
  where
    C: ConnectionTrait;
}

#[derive(Clone, Debug, Default)]
pub struct Ap {
  _namespace: String,
  _object: Option<SimpleExpr>,
  _actions: Vec<String>,
  _users: Vec<ApUser>,
  _alias: Option<Alias>,
}

impl Ap {
  pub fn new<P: AsRef<str>>(namespace: P) -> Self {
    Self {
      _namespace: namespace.as_ref().to_owned(),
      _object: None,
      _actions: Vec::new(),
      _users: Vec::new(),
      _alias: None,
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

    for ap_user in &self._users {
      condition = condition.add(col_user.clone().eq(ap_user.to_string()));
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

  pub fn alias(mut self, alias: Alias) -> Self {
    self._alias = Some(alias);
    self
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
}

impl ApGuard for Ap {
  fn object(mut self, expr: SimpleExpr) -> Self {
    self._object = Some(expr);
    self
  }

  fn action<S: AsRef<str>>(mut self, action: S) -> Self {
    self._actions.push(action.as_ref().to_owned());
    self
  }

  fn actions(mut self, actions: &mut Vec<String>) -> Self {
    self._actions.append(actions);
    self
  }

  fn user(mut self, ap_user: ApUser) -> Self {
    self._users.push(ap_user);
    self
  }

  fn owner(mut self, owner: &UserContext) -> Self {
    if let Some(key) = &owner.profile.api_key {
      self._users.push(ApUser::ApiKey(key.to_owned()));
    } else {
      self._users.push(ApUser::User(owner.profile.id));
      self._users.push(ApUser::Role(owner.profile.role.id));
      self._users.push(ApUser::Org(owner.profile.organization.id));

      if owner.profile.is_admin {
        self._users.push(ApUser::Admin);
      }
    }

    self
  }

  fn condition(&self) -> Condition {
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

  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E> {
    query.filter(self.condition())
  }

  async fn count<'db, E, C>(self, query: Select<E>, db: &'db C) -> u64
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    PaginatorTrait::count(self.apply(query), db).await.unwrap_or(0)
  }

  async fn check<'db, E, C>(self, query: Select<E>, db: &'db C) -> Result<(), SumidaError>
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    let query = self.apply(query);

    if PaginatorTrait::count(query, db).await.unwrap_or(0) > 0 {
      Ok(())
    } else {
      Err(SumidaError::NoServiceAvailable)
    }
  }

  async fn check_allowed<'db, C>(self, db: &'db C) -> bool
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

#[derive(Clone, Debug, Default)]
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
}

impl ApGuard for ApChain {
  fn object(mut self, expr: SimpleExpr) -> Self {
    for item in &mut self.items {
      *item = mem::take(item).object(expr.clone());
    }

    self
  }

  fn action<S: AsRef<str>>(mut self, action: S) -> Self {
    for item in &mut self.items {
      *item = mem::take(item).action(action.as_ref().to_string());
    }

    self
  }

  fn actions(mut self, actions: &mut Vec<String>) -> Self {
    for item in &mut self.items {
      *item = mem::take(item).actions(actions);
    }

    self
  }

  fn user(mut self, ap_user: ApUser) -> Self {
    for item in &mut self.items {
      *item = mem::take(item).user(ap_user.clone());
    }

    self
  }

  fn owner(mut self, owner: &UserContext) -> Self {
    for item in &mut self.items {
      *item = mem::take(item).owner(owner);
    }

    self
  }

  fn condition(&self) -> Condition {
    let mut cond_allow = Condition::any();
    let mut cond_deny = Condition::all();

    for item in &self.items {
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

  fn apply<E: EntityTrait>(self, query: Select<E>) -> Select<E> {
    query.filter(self.condition())
  }

  async fn count<'db, E, C>(self, query: Select<E>, db: &'db C) -> u64
  where
    E: EntityTrait,
    C: ConnectionTrait,
    E::Model: Sync,
  {
    PaginatorTrait::count(self.apply(query), db).await.unwrap_or(0)
  }

  async fn check<'db, E, C>(self, query: Select<E>, db: &'db C) -> Result<(), SumidaError>
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

  async fn check_allowed<'db, C>(self, db: &'db C) -> bool
  where
    C: ConnectionTrait,
  {
    for item in self.items {
      if !item.check_allowed(db).await {
        return false;
      }
    }

    true
  }
}

#[derive(Clone, Debug, Serialize, Deserialize, DerivePartialModel)]
#[sea_orm(entity = "entity::access_policies::Entity", alias = "ap_allow", from_query_result)]
#[serde(rename_all = "camelCase")]
pub struct UserAp {
  namespace: String,
  object: Vec<String>,
  action: Vec<String>,
  effect: String,
}

impl UserAp {
  pub fn query() -> UserApQuery {
    UserApQuery::new()
  }
}

#[derive(Clone, Debug)]
pub struct UserApQuery {
  _users: Vec<ApUser>,
}

impl UserApQuery {
  pub fn new() -> Self {
    Self { _users: Vec::new() }
  }

  pub fn user(mut self, ap_user: ApUser) -> Self {
    self._users.push(ap_user);
    self
  }

  pub fn owner(mut self, owner: &UserContext) -> Self {
    if let Some(key) = &owner.profile.api_key {
      self._users.push(ApUser::ApiKey(key.to_owned()));
    } else {
      self._users.push(ApUser::User(owner.profile.id));
      self._users.push(ApUser::Role(owner.profile.role.id));
      self._users.push(ApUser::Org(owner.profile.organization.id));

      if owner.profile.is_admin {
        self._users.push(ApUser::Admin);
      }
    }

    self
  }

  fn _build_users(&self) -> Vec<String> {
    self._users.iter().map(|x| x.to_string()).collect()
  }

  pub async fn find_all<'db, C: ConnectionTrait>(self, db: &'db C) -> Result<Vec<UserAp>, DbErr> {
    let result = UserAp::find_by_statement(Statement::from_sql_and_values(
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
      [self._build_users().into()],
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
    user_aps: Vec<UserAp>,
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
    Self::_clear(&self._build_users(), db).await
  }

  pub async fn save<'db, C: ConnectionTrait>(self, user_aps: Vec<UserAp>, db: &'db C) -> Result<(), DbErr> {
    Self::_save(&self._build_users(), user_aps, db).await
  }

  pub async fn update<'db, C: ConnectionTrait>(
    self,
    user_aps: Vec<UserAp>,
    db: &'db C,
  ) -> Result<(), DbErr> {
    let users = self._build_users();
    Self::_clear(&users, db).await?;
    Self::_save(&users, user_aps, db).await?;

    Ok(())
  }
}

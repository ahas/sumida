use core::session::UserContext;
use entity::access_policies::Column as ApColumn;
use entity::access_policies::Entity as Ap;
use sea_orm::sea_query::Alias;
use sea_orm::sea_query::IntoColumnRef;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{Condition, JoinType, QuerySelect, prelude::*};

pub struct Access {
  _namespace: Option<String>,
  _object: Option<String>,
  _action: Option<String>,
  _user_id: Option<i32>,
  _role_id: Option<i32>,
  _org_id: Option<i32>,
  _alias: Option<Alias>,
  _is_admin: bool,
  _is_default_allow: bool,
}

impl Access {
  pub fn new() -> Self {
    Self {
      _namespace: None,
      _object: None,
      _action: None,
      _user_id: None,
      _role_id: None,
      _org_id: None,
      _alias: None,
      _is_admin: false,
      _is_default_allow: false,
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

  pub fn default_allow(mut self) -> Self {
    self._is_default_allow = true;
    self
  }

  pub fn alias(mut self, alias: Alias) -> Self {
    self._alias = Some(alias);
    self
  }

  pub fn _unwrap_alias(&self) -> Alias {
    self._alias.clone().unwrap_or_else(|| Alias::new(Ap.table_name()))
  }

  pub fn join<Q, C>(query: &mut Q, alias: Alias, column: C)
  where
    Q: QuerySelect,
    C: IntoColumnRef,
  {
    sea_orm::QuerySelect::query(query).join_as(
      JoinType::LeftJoin,
      entity::access_policies::Entity,
      alias.clone(),
      Condition::all().add(
        Expr::col((alias.clone(), entity::access_policies::Column::Object))
          .eq(Expr::col(column).cast_as("varchar")),
      ),
    );
  }

  /// Builds a permission checking condition based on provided parameters.
  pub fn build(self) -> Condition {
    let alias = self._unwrap_alias();
    let ns = self._namespace.expect("Access: namespace must be set");
    let action = self._action.expect("Access: action must be set");
    let obj = self._object;

    // Define columns using the alias.
    let col_uuid = Expr::col((alias.clone(), ApColumn::Uuid));
    let col_ns = Expr::col((alias.clone(), ApColumn::Namespace));
    let col_obj = Expr::col((alias.clone(), ApColumn::Object));
    let col_user = Expr::col((alias.clone(), ApColumn::User));
    let col_action = Expr::col((alias.clone(), ApColumn::Action));

    // Match any user identity (user, org, or role) or admin.
    let mut permission_match = Condition::any();

    // Match by user ID if provided.
    if let Some(user_id) = self._user_id {
      permission_match = permission_match.add(col_user.clone().eq(format!("user:{}", user_id)));
    }

    // Match by organization ID if provided.
    if let Some(org_id) = self._org_id {
      permission_match = permission_match.add(col_user.clone().eq(format!("org:{}", org_id)));
    }

    // Match by role ID if provided.
    if let Some(role_id) = self._role_id {
      permission_match = permission_match.add(col_user.clone().eq(format!("role:{}", role_id)));
    }

    // Always allow admin access if applicable.
    if self._is_admin {
      permission_match = permission_match.add(col_user.clone().eq("user:admin"));
    }

    // If default-allow is set, allow access when no matching access policy exists (id IS NULL).
    let mut lhs: Option<SimpleExpr> = None;
    if self._is_default_allow {
      lhs = Some(col_uuid.is_null());
    }

    // Define the right-hand side condition:
    // - Namespace must match.
    // - Action must match the requested action or be "all".
    let mut rhs = Condition::all()
      .add(Condition::any().add(col_ns.clone().eq(ns)).add(col_ns.clone().eq("all")))
      .add(Condition::any().add(col_action.clone().eq(action)).add(col_action.clone().eq("all")));

    // If object is specified, include it in the condition.
    if let Some(obj) = obj {
      rhs = rhs.add(col_obj.eq(obj));
    }

    // Add the permission matching condition.
    rhs = rhs.add(permission_match);

    // Final condition:
    // - Allow if lhs (default allow) applies,
    // - OR if rhs (namespace, action, user match) applies.
    let mut condition = Condition::any();
    if let Some(lhs) = lhs {
      condition = condition.add(lhs);
    }
    condition.add(rhs)
  }

  pub fn filter<E, C>(self, query: Select<E>, column: C) -> Select<E>
  where
    E: EntityTrait,
    C: IntoColumnRef,
  {
    let alias = self._unwrap_alias();
    let mut query = query.filter(self.build());

    Self::join(&mut query, alias, column);

    query
  }

  pub fn find(self) -> Select<Ap> {
    Ap::find().filter(self.build())
  }

  pub async fn count<'db, C: ConnectionTrait>(self, db: &'db C) -> u64 {
    self.find().count(db).await.unwrap_or(0)
  }
}

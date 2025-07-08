use argon2::{
  Argon2,
  password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
};
use sea_orm::{prelude::*, sea_query::{Alias, Asterisk}, JoinType, QuerySelect};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id: i32,
  #[sea_orm(unique)]
  pub username: String,
  pub password: String,
  pub email: Option<String>,
  pub full_name: Option<String>,
  pub phone: Option<String>,
  pub is_admin: bool,
  pub is_enabled: bool,
  pub role_id: i32,
  pub organization_id: i32,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl Model {
  pub async fn get_organization(
    &self,
    db_conn: &DatabaseConnection,
  ) -> Result<Option<super::organizations::Model>, DbErr> {
    super::organizations::Entity::find_by_id(self.organization_id).one(db_conn).await
  }

  pub async fn get_role(&self, db_conn: &DatabaseConnection) -> Result<Option<super::roles::Model>, DbErr> {
    super::roles::Entity::find_by_id(self.role_id).one(db_conn).await
  }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_one = "super::preferences::Entity")]
  Preference,
  #[sea_orm(
    belongs_to = "super::organizations::Entity",
    from = "Column::OrganizationId",
    to = "super::organizations::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  Organization,
  #[sea_orm(
    belongs_to = "super::roles::Entity",
    from = "Column::RoleId",
    to = "super::roles::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  Role,
}

impl Related<super::preferences::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Preference.def()
  }
}

impl Related<super::organizations::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Organization.def()
  }
}

impl Related<super::roles::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Role.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

impl Entity {
  pub fn select() -> Select<Self> {
    let org_alias = Alias::new("org");
    let role_alias = Alias::new("role");

    Self::find()
      .select_only()
      .expr(Expr::col((Self, Asterisk)))
      .expr_as(super::user_blocks::Entity::exists_expr(), "is_blocked")
      .expr_as(Expr::col((org_alias.clone(), "id")), "org_id")
      .expr_as(Expr::col((org_alias.clone(), "name")), "org_name")
      .expr_as(Expr::col((role_alias.clone(), "id")), "role_id")
      .expr_as(Expr::col((role_alias.clone(), "name")), "role_name")
      .join_as(JoinType::LeftJoin, super::users::Relation::Organization.def(), org_alias.clone())
      .join_as(JoinType::LeftJoin, super::users::Relation::Role.def(), role_alias.clone())
  }

  pub fn hash_password<S>(password: S) -> argon2::password_hash::Result<String>
  where
    S: AsRef<str>,
  {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    Ok(argon2.hash_password(password.as_ref().as_bytes(), &salt)?.to_string())
  }
}

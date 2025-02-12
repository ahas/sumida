use argon2::{
  password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
  Argon2,
};
use sea_orm::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub username: String,
  pub password: String,
  pub organization: i32,
  pub email: Option<String>,
  pub full_name: Option<String>,
  pub phone: Option<String>,
  pub role: i32,
  pub is_admin: bool,
  pub is_enabled: bool,
  pub last_logged_in_at: Option<DateTimeWithTimeZone>,
  pub created_at: DateTimeWithTimeZone,
  pub updated_at: DateTimeWithTimeZone,
}

impl Model {
  pub async fn get_organization(
    &self,
    db_conn: &DatabaseConnection,
  ) -> Result<Option<super::organizations::Model>, DbErr> {
    super::organizations::Entity::find_by_id(self.organization).one(db_conn).await
  }

  pub async fn get_role(&self, db_conn: &DatabaseConnection) -> Result<Option<super::roles::Model>, DbErr> {
    super::roles::Entity::find_by_id(self.role).one(db_conn).await
  }
}

impl Entity {
  pub fn hash_password<S>(password: S) -> argon2::password_hash::Result<String>
  where
    S: AsRef<str>,
  {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);

    Ok(argon2.hash_password(password.as_ref().as_bytes(), &salt)?.to_string())
  }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_one = "super::preferences::Entity")]
  Preferences,
}

impl Related<super::preferences::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Preferences.def()
  }
}

impl ActiveModelBehavior for ActiveModel {}

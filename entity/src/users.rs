use sea_orm::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "users")]
pub struct Model {
  #[sea_orm(primary_key)]
  pub id:                   i32,
  #[sea_orm(indexed, unique)]
  pub username:             String,
  pub password:             String,
  /// When this account last reset its credentials. Everything issued to it up to that moment — a
  /// token, a session — is refused from then on; `None` is an account that has never reset them.
  #[sea_orm(nullable)]
  pub credentials_reset_at: Option<DateTimeUtc>,
  #[sea_orm(nullable)]
  pub email:                Option<String>,
  #[sea_orm(nullable)]
  pub full_name:            Option<String>,
  #[sea_orm(nullable)]
  pub phone:                Option<String>,
  pub is_admin:             bool,
  pub is_enabled:           bool,
  #[sea_orm(indexed)]
  pub role_id:              i32,
  #[sea_orm(indexed)]
  pub organization_id:      i32,
  pub created_at:           DateTimeUtc,
  pub updated_at:           DateTimeUtc,
}

impl Model {
  pub async fn get_organization(
    &self,
    db: &DatabaseConnection,
  ) -> Result<Option<super::organizations::Model>, DbErr> {
    super::organizations::Entity::find_by_id(self.organization_id).one(db).await
  }

  pub async fn get_role(&self, db: &DatabaseConnection) -> Result<Option<super::roles::Model>, DbErr> {
    super::roles::Entity::find_by_id(self.role_id).one(db).await
  }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(has_one = "super::user_prefs::Entity")]
  Pref,
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

impl Related<super::user_prefs::Entity> for Entity {
  fn to() -> RelationDef {
    Relation::Pref.def()
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

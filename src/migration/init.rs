use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;
use serde_json::json;
use ForeignKeyAction::*;

#[derive(DeriveIden)]
enum Organizations {
  Table,
  Id,
  Name,
  Parent,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Roles {
  Table,
  Id,
  Name,
  Description,
  Ability,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
  Table,
  Username,
  Password,
  Email,
  FullName,
  Phone,
  IsAdmin,
  IsEnabled,
  Role,
  Organization,
  LastLoggedInAt,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum UserInvitations {
  Table,
  Username,
  Uuid,
  ExpiredAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum UserVerifications {
  Table,
  Username,
  Kind,
  Method,
  Code,
  ExpiredAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum Preferences {
  Table,
  Username,
  Data,
}

#[derive(DeriveIden)]
enum ApiKeys {
  Table,
  Key,
  User,
  Ability,
  CreatedAt,
}

#[derive(DeriveIden)]
enum MailForms {
  Table,
  Kind,
  Sender,
  Subject,
  Content,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db_conn = manager.get_connection();

    // Extensions
    db_conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\";").await?;

    // Functions
    db_conn
      .execute_unprepared(
        r#"
        CREATE OR REPLACE FUNCTION manage_updated_at(_tbl regclass) RETURNS VOID AS $$
        BEGIN
          EXECUTE format('CREATE TRIGGER set_updated_at BEFORE UPDATE ON %s
                          FOR EACH ROW EXECUTE PROCEDURE set_updated_at()', _tbl);
        END;
        $$ LANGUAGE plpgsql;

        CREATE OR REPLACE FUNCTION set_updated_at() RETURNS trigger AS $$
        BEGIN
          IF (
            NEW IS DISTINCT FROM OLD AND
            NEW.updated_at IS NOT DISTINCT FROM OLD.updated_at
          ) THEN
            NEW.updated_at := CURRENT_TIMESTAMP;
          END IF;

          RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        "#,
      )
      .await?;

    // BEGIN organizations table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Organizations::Table)
          .col(pk_auto(Organizations::Id))
          .col(string(Organizations::Name))
          .col(integer_null(Organizations::Parent))
          .col(timestamp_with_time_zone(Organizations::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Organizations::UpdatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("organizations_organizations_fk")
          .from(Organizations::Table, Organizations::Parent)
          .to(Organizations::Table, Organizations::Id)
          .on_delete(SetNull)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("organizations_parent_idx")
          .table(Organizations::Table)
          .col(Organizations::Parent)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    db_conn.execute_unprepared("SELECT manage_updated_at('organizations');").await?;
    // END organizations table

    // BEGIN roles table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Roles::Table)
          .col(pk_auto(Roles::Id))
          .col(string(Roles::Name))
          .col(string(Roles::Description).default(""))
          .col(json_binary(Roles::Ability).default(json!({})))
          .col(timestamp_with_time_zone(Roles::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Roles::UpdatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    db_conn.execute_unprepared("SELECT manage_updated_at('roles');").await?;
    // END roles table

    // BEGIN users table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Users::Table)
          .col(string(Users::Username).primary_key())
          .col(string(Users::Password))
          .col(string_null(Users::Email))
          .col(string_null(Users::FullName))
          .col(string_null(Users::Phone))
          .col(integer(Users::Role))
          .col(integer(Users::Organization))
          .col(boolean(Users::IsAdmin).default(false))
          .col(boolean(Users::IsEnabled).default(false))
          .col(timestamp_with_time_zone_null(Users::LastLoggedInAt))
          .col(timestamp_with_time_zone(Users::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Users::UpdatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("users_organizations_fk")
          .from(Users::Table, Users::Organization)
          .to(Organizations::Table, Organizations::Id)
          .on_delete(SetNull)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("users_roles_fk")
          .from(Users::Table, Users::Role)
          .to(Roles::Table, Roles::Id)
          .on_delete(SetNull)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("users_organization_idx")
          .table(Users::Table)
          .col(Users::Organization)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("users_role_idx")
          .table(Users::Table)
          .col(Users::Role)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    db_conn.execute_unprepared("SELECT manage_updated_at('users');").await?;
    // END users table

    // BEGIN preferences table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Preferences::Table)
          .col(string(Preferences::Username).primary_key())
          .col(json_binary(Preferences::Data).default(json!({})))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("preferences_users_fk")
          .from(Preferences::Table, Preferences::Username)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("preferences_username_idx")
          .table(Preferences::Table)
          .col(Preferences::Username)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // END preferences table

    // BEGIN api_keys table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(ApiKeys::Table)
          .col(string(ApiKeys::Key).primary_key())
          .col(string_null(ApiKeys::User))
          .col(json_binary_null(ApiKeys::Ability))
          .col(timestamp_with_time_zone(ApiKeys::CreatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    // END api_keys table

    // BEGIN user_invitations table
    manager
      .create_table(
        Table::create()
          .table(UserInvitations::Table)
          .col(uuid(UserInvitations::Uuid).primary_key().default(Expr::cust("uuid_generate_v4()")))
          .col(string(UserInvitations::Username))
          .col(timestamp_with_time_zone(UserInvitations::ExpiredAt))
          .col(timestamp_with_time_zone(UserInvitations::CreatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("user_invitations_username_fk")
          .from(UserInvitations::Table, UserInvitations::Username)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END user_invitations table

    // BEGIN user_verifications table
    manager
      .create_table(
        Table::create()
          .table(UserVerifications::Table)
          .col(string(UserVerifications::Username))
          .col(string(UserVerifications::Kind))
          .col(string(UserVerifications::Method))
          .col(string(UserVerifications::Code))
          .col(timestamp_with_time_zone(UserVerifications::ExpiredAt))
          .col(timestamp_with_time_zone(UserVerifications::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().col(UserVerifications::Username).col(UserVerifications::Kind))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("user_verifications_username_fk")
          .from(UserVerifications::Table, UserVerifications::Username)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END user_verifications table

    // BEGIN mail_forms table
    manager
      .create_table(
        Table::create()
          .table(MailForms::Table)
          .col(string(MailForms::Kind).primary_key())
          .col(string(MailForms::Sender))
          .col(string(MailForms::Subject))
          .col(string(MailForms::Content).default(""))
          .col(timestamp_with_time_zone(MailForms::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(MailForms::UpdatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db_conn = manager.get_connection();

    // Indexes
    manager
      .drop_index(
        Index::drop().if_exists().name("organizations_parent_idx").table(Organizations::Table).to_owned(),
      )
      .await?;
    manager
      .drop_index(Index::drop().if_exists().name("users_organization_idx").table(Users::Table).to_owned())
      .await?;
    manager
      .drop_index(Index::drop().if_exists().name("users_role_idx").table(Users::Table).to_owned())
      .await?;
    manager
      .drop_index(
        Index::drop().if_exists().name("preferences_username_idx").table(Preferences::Table).to_owned(),
      )
      .await?;

    // Foreign Keys
    manager
      .drop_foreign_key(
        ForeignKey::drop().name("organizations_organizations_fk").table(Organizations::Table).to_owned(),
      )
      .await?;
    manager
      .drop_foreign_key(ForeignKey::drop().name("users_organizations_fk").table(Users::Table).to_owned())
      .await?;
    manager
      .drop_foreign_key(ForeignKey::drop().name("users_roles_fk").table(Users::Table).to_owned())
      .await?;
    manager
      .drop_foreign_key(
        ForeignKey::drop().name("preferences_users_fk").table(Preferences::Table).to_owned(),
      )
      .await?;
    manager
      .drop_foreign_key(
        ForeignKey::drop().name("user_invitations_username_fk").table(UserInvitations::Table).to_owned(),
      )
      .await?;
    manager
      .drop_foreign_key(
        ForeignKey::drop()
          .name("user_verifications_username_fk")
          .table(UserVerifications::Table)
          .to_owned(),
      )
      .await?;

    // Triggers
    db_conn.execute_unprepared("DROP TRIGGER IF EXISTS set_updated_at ON organizations;").await?;
    db_conn.execute_unprepared("DROP TRIGGER IF EXISTS set_updated_at ON roles;").await?;
    db_conn.execute_unprepared("DROP TRIGGER IF EXISTS set_updated_at ON users;").await?;

    // Tables
    manager.drop_table(Table::drop().if_exists().table(Organizations::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Roles::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Users::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Preferences::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(ApiKeys::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserInvitations::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserVerifications::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(MailForms::Table).to_owned()).await?;

    // Functions
    db_conn.execute_unprepared("DROP FUNCTION IF EXISTS manage_updated_at(in _tbl regclass);").await?;
    db_conn.execute_unprepared("DROP FUNCTION IF EXISTS set_updated_at();").await?;

    // Extensions
    db_conn.execute_unprepared("DROP EXTENSION IF EXISTS \"uuid-ossp\";").await?;

    Ok(())
  }
}

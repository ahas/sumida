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
enum UserBlocks {
  Table,
  Id,
  User,
  BlockedAt,
  BlockedBy,
  Reason,
  ExpiresAt,
  LiftedAt,
  Note,
}

#[derive(DeriveIden)]
enum UserInvitations {
  Table,
  User,
  Uuid,
  ExpiredAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum UserSignInLogs {
  Table,
  Id,
  User,
  Method,
  Code,
  IpAddress,
  UserAgent,
  OsFamily,
  OsMajor,
  OsMinor,
  OsPatch,
  OsPatchMinor,
  DeviceFamily,
  DeviceBrand,
  DeviceModel,
  CreatedAt,
}

#[derive(DeriveIden)]
enum UserVerifications {
  Table,
  User,
  Kind,
  Method,
  Code,
  ExpiredAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum Preferences {
  Table,
  User,
  Data,
}

#[derive(DeriveIden)]
enum ApiKeys {
  Table,
  Key,
  User,
  Ability,
  ExpiresAt,
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
          .name("fk_organizations_parent")
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
          .name("idx_organizations_parent")
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
          .name("fk_users_organization")
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
          .name("fk_users_role")
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
          .name("idx_users_organization")
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
          .name("idx_users_role")
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
          .col(string(Preferences::User).primary_key())
          .col(json_binary(Preferences::Data).default(json!({})))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("fk_preferences__users")
          .from(Preferences::Table, Preferences::User)
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
          .name("idx_preferences__user")
          .table(Preferences::Table)
          .col(Preferences::User)
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
          .col(string(ApiKeys::User))
          .col(json_binary_null(ApiKeys::Ability))
          .col(timestamp_with_time_zone_null(ApiKeys::ExpiresAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(ApiKeys::CreatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("fk_api_keys_user")
          .from(ApiKeys::Table, ApiKeys::User)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END api_keys table

    // BEGIN user_blocks table
    manager
      .create_table(
        Table::create()
          .table(UserBlocks::Table)
          .col(pk_auto(UserBlocks::Id))
          .col(string(UserBlocks::User))
          .col(timestamp_with_time_zone(UserBlocks::BlockedAt))
          .col(string_null(UserBlocks::BlockedBy))
          .col(string(UserBlocks::Reason))
          .col(timestamp_with_time_zone_null(UserBlocks::ExpiresAt))
          .col(timestamp_with_time_zone_null(UserBlocks::LiftedAt))
          .col(string_null(UserBlocks::Note))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("fk_user_blocks_user")
          .from(UserBlocks::Table, UserBlocks::User)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("fk_user_blocks_blocked_by")
          .from(UserBlocks::Table, UserBlocks::BlockedBy)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END user_blocks table

    // BEGIN user_invitations table
    manager
      .create_table(
        Table::create()
          .table(UserInvitations::Table)
          .col(uuid(UserInvitations::Uuid).primary_key().default(Expr::cust("uuid_generate_v4()")))
          .col(string(UserInvitations::User))
          .col(timestamp_with_time_zone(UserInvitations::ExpiredAt))
          .col(timestamp_with_time_zone(UserInvitations::CreatedAt).default(Expr::current_timestamp()))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("user_invitations_user_fk")
          .from(UserInvitations::Table, UserInvitations::User)
          .to(Users::Table, Users::Username)
          .on_delete(Cascade)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END user_invitations table

    // BEGIN user_sign_in_logs
    manager
      .create_table(
        Table::create()
          .table(UserSignInLogs::Table)
          .col(pk_auto(UserSignInLogs::Id))
          .col(string(UserSignInLogs::User))
          .col(string(UserSignInLogs::Method))
          .col(integer(UserSignInLogs::Code))
          .col(string(UserSignInLogs::IpAddress))
          .col(string(UserSignInLogs::UserAgent))
          .col(string(UserSignInLogs::OsFamily))
          .col(string_null(UserSignInLogs::OsMajor))
          .col(string_null(UserSignInLogs::OsMinor))
          .col(string_null(UserSignInLogs::OsPatch))
          .col(string_null(UserSignInLogs::OsPatchMinor))
          .col(string(UserSignInLogs::DeviceFamily))
          .col(string_null(UserSignInLogs::DeviceBrand))
          .col(string_null(UserSignInLogs::DeviceModel))
          .col(timestamp_with_time_zone(UserSignInLogs::CreatedAt))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("fk_user_sign_in_logs_user")
          .from(UserSignInLogs::Table, UserSignInLogs::User)
          .to(Users::Table, Users::Username)
          .on_delete(SetNull)
          .on_update(Cascade)
          .to_owned(),
      )
      .await?;
    // END user_sign_in_logs

    // BEGIN user_verifications table
    manager
      .create_table(
        Table::create()
          .table(UserVerifications::Table)
          .col(string(UserVerifications::User))
          .col(string(UserVerifications::Kind))
          .col(string(UserVerifications::Method))
          .col(string(UserVerifications::Code))
          .col(timestamp_with_time_zone(UserVerifications::ExpiredAt))
          .col(timestamp_with_time_zone(UserVerifications::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().col(UserVerifications::User).col(UserVerifications::Kind))
          .to_owned(),
      )
      .await?;
    manager
      .create_foreign_key(
        ForeignKey::create()
          .name("user_verifications_user_fk")
          .from(UserVerifications::Table, UserVerifications::User)
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
        Index::drop().if_exists().name("preferences_user_idx").table(Preferences::Table).to_owned(),
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
        ForeignKey::drop().name("user_invitations_user_fk").table(UserInvitations::Table).to_owned(),
      )
      .await?;
    manager
      .drop_foreign_key(
        ForeignKey::drop().name("user_verifications_user_fk").table(UserVerifications::Table).to_owned(),
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

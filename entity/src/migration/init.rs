use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;
use serde_json::json;

#[derive(DeriveIden)]
enum AccessPolicies {
  Table,
  Uuid,
  Namespace,
  Object,
  Action,
  User,
  Effect,
  CreatedAt,
}

#[derive(DeriveIden)]
enum Organizations {
  Table,
  Id,
  Name,
  ParentId,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Roles {
  Table,
  Id,
  Name,
  Description,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
  Table,
  Id,
  Username,
  Password,
  Email,
  FullName,
  Phone,
  IsAdmin,
  IsEnabled,
  RoleId,
  OrganizationId,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum UserBlocks {
  Table,
  Id,
  UserId,
  IpAddress,
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
  UserId,
  Uuid,
  ExpiresAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum UserLogs {
  Table,
  Id,
  UserId,
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
  UserId,
  Kind,
  Method,
  Code,
  ExpiresAt,
  CreatedAt,
}

#[derive(DeriveIden)]
enum UserPrefs {
  Table,
  UserId,
  Data,
}

#[derive(DeriveIden)]
enum ApiKeys {
  Table,
  Key,
  UserId,
  ExpiresAt,
  IsSecured,
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
    let db = manager.get_connection();

    // Extensions
    db.execute_unprepared("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\";").await?;

    // Functions
    db.execute_unprepared(
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

    // BEGIN access_policies table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(AccessPolicies::Table)
          .col(uuid(AccessPolicies::Uuid).default(Expr::cust("uuid_generate_v4()")))
          .col(string(AccessPolicies::Namespace))
          .col(string(AccessPolicies::Effect))
          .col(string(AccessPolicies::Object))
          .col(
            array(AccessPolicies::Action, ColumnType::String(StringLen::None)).default(Expr::cust("'{}'")),
          )
          .col(string(AccessPolicies::User))
          .col(timestamp_with_time_zone(AccessPolicies::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_access_policies__uuid").col(AccessPolicies::Uuid))
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_access_policies__tuple")
          .table(AccessPolicies::Table)
          .col(AccessPolicies::Namespace)
          .col(AccessPolicies::Object)
          .col(AccessPolicies::Action)
          .col(AccessPolicies::User)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_access_policies__group")
          .table(AccessPolicies::Table)
          .col(AccessPolicies::Namespace)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_access_policies__user")
          .table(AccessPolicies::Table)
          .col(AccessPolicies::User)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;

    // BEGIN organizations table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Organizations::Table)
          .col(integer(Organizations::Id).auto_increment())
          .col(string(Organizations::Name))
          .col(integer_null(Organizations::ParentId))
          .col(timestamp_with_time_zone(Organizations::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Organizations::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_organizations__id").col(Organizations::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_organizations__parent_id")
              .from(Organizations::Table, Organizations::ParentId)
              .to(Organizations::Table, Organizations::Id)
              .on_delete(SetNull)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_organizations__parent_id")
          .table(Organizations::Table)
          .col(Organizations::ParentId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    db.execute_unprepared("SELECT manage_updated_at('organizations');").await?;
    // END organizations table

    // BEGIN roles table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Roles::Table)
          .col(integer(Roles::Id).auto_increment())
          .col(string(Roles::Name))
          .col(string(Roles::Description).default(""))
          .col(timestamp_with_time_zone(Roles::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Roles::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_roles__id").col(Roles::Id))
          .to_owned(),
      )
      .await?;
    db.execute_unprepared("SELECT manage_updated_at('roles');").await?;
    // END roles table

    // BEGIN users table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Users::Table)
          .col(integer(Users::Id).auto_increment())
          .col(string(Users::Username))
          .col(string(Users::Password))
          .col(string_null(Users::Email))
          .col(string_null(Users::FullName))
          .col(string_null(Users::Phone))
          .col(integer(Users::RoleId))
          .col(integer(Users::OrganizationId))
          .col(boolean(Users::IsAdmin).default(false))
          .col(boolean(Users::IsEnabled).default(false))
          .col(timestamp_with_time_zone(Users::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Users::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_users__id").col(Users::Id))
          .index(
            Index::create()
              .if_not_exists()
              .name("uq_users__username")
              .table(Users::Table)
              .col(Users::Username)
              .unique(),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_users__organization_id")
              .from(Users::Table, Users::OrganizationId)
              .to(Organizations::Table, Organizations::Id)
              .on_delete(SetNull)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_users__role_id")
              .from(Users::Table, Users::RoleId)
              .to(Roles::Table, Roles::Id)
              .on_delete(SetNull)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;

    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_users__organization_id")
          .table(Users::Table)
          .col(Users::OrganizationId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_users__role_id")
          .table(Users::Table)
          .col(Users::RoleId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    db.execute_unprepared("SELECT manage_updated_at('users');").await?;
    // END users table

    // BEGIN user_prefs table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(UserPrefs::Table)
          .col(integer(UserPrefs::UserId))
          .col(json_binary(UserPrefs::Data).default(json!({})))
          .primary_key(Index::create().name("pk_user_prefs__user_id").col(UserPrefs::UserId))
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_prefs__user_id")
              .from(UserPrefs::Table, UserPrefs::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_user_prefs__user_id")
          .table(UserPrefs::Table)
          .col(UserPrefs::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // END user_prefs table

    // BEGIN api_keys table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(ApiKeys::Table)
          .col(string(ApiKeys::Key))
          .col(integer(ApiKeys::UserId))
          .col(timestamp_with_time_zone_null(ApiKeys::ExpiresAt).default(Expr::current_timestamp()))
          .col(boolean(ApiKeys::IsSecured).default(false))
          .col(timestamp_with_time_zone(ApiKeys::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_api_keys__key").col(ApiKeys::Key))
          .foreign_key(
            ForeignKey::create()
              .name("fk_api_keys__user_id")
              .from(ApiKeys::Table, ApiKeys::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END api_keys table

    // BEGIN user_blocks table
    manager
      .create_table(
        Table::create()
          .table(UserBlocks::Table)
          .col(integer(UserBlocks::Id).auto_increment())
          .col(integer_null(UserBlocks::UserId))
          .col(string_null(UserBlocks::IpAddress))
          .col(timestamp_with_time_zone(UserBlocks::BlockedAt))
          .col(integer_null(UserBlocks::BlockedBy))
          .col(string_null(UserBlocks::Reason))
          .col(timestamp_with_time_zone_null(UserBlocks::ExpiresAt))
          .col(timestamp_with_time_zone_null(UserBlocks::LiftedAt))
          .col(string_null(UserBlocks::Note))
          .primary_key(Index::create().name("pk_user_blocks__id").col(UserBlocks::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_blocks__user_id")
              .from(UserBlocks::Table, UserBlocks::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_blocks__blocked_by")
              .from(UserBlocks::Table, UserBlocks::BlockedBy)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END user_blocks table

    // BEGIN user_invitations table
    manager
      .create_table(
        Table::create()
          .table(UserInvitations::Table)
          .col(uuid(UserInvitations::Uuid).default(Expr::cust("uuid_generate_v4()")))
          .col(integer(UserInvitations::UserId))
          .col(timestamp_with_time_zone(UserInvitations::ExpiresAt))
          .col(timestamp_with_time_zone(UserInvitations::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_user_invitations__uuid").col(UserInvitations::Uuid))
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_invitations__user_id")
              .from(UserInvitations::Table, UserInvitations::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END user_invitations table

    // BEGIN user_logs
    manager
      .create_table(
        Table::create()
          .table(UserLogs::Table)
          .col(integer(UserLogs::Id).auto_increment())
          .col(integer(UserLogs::UserId))
          .col(string(UserLogs::Method))
          .col(integer(UserLogs::Code))
          .col(string(UserLogs::IpAddress))
          .col(string(UserLogs::UserAgent))
          .col(string(UserLogs::OsFamily))
          .col(string_null(UserLogs::OsMajor))
          .col(string_null(UserLogs::OsMinor))
          .col(string_null(UserLogs::OsPatch))
          .col(string_null(UserLogs::OsPatchMinor))
          .col(string(UserLogs::DeviceFamily))
          .col(string_null(UserLogs::DeviceBrand))
          .col(string_null(UserLogs::DeviceModel))
          .col(timestamp_with_time_zone(UserLogs::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_user_logs__id").col(UserLogs::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_logs__user_id")
              .from(UserLogs::Table, UserLogs::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(SetNull)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END user_logs

    // BEGIN user_verifications table
    manager
      .create_table(
        Table::create()
          .table(UserVerifications::Table)
          .col(integer(UserVerifications::UserId))
          .col(string(UserVerifications::Kind))
          .col(string(UserVerifications::Method))
          .col(string(UserVerifications::Code))
          .col(timestamp_with_time_zone(UserVerifications::ExpiresAt))
          .col(timestamp_with_time_zone(UserVerifications::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(
            Index::create()
              .name("pk_user_verifications__user_id__kind")
              .col(UserVerifications::UserId)
              .col(UserVerifications::Kind),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_user_verifications__user_id")
              .from(UserVerifications::Table, UserVerifications::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END user_verifications table

    // BEGIN mail_forms table
    manager
      .create_table(
        Table::create()
          .table(MailForms::Table)
          .col(string(MailForms::Kind))
          .col(string(MailForms::Sender))
          .col(string(MailForms::Subject))
          .col(string(MailForms::Content).default(""))
          .col(timestamp_with_time_zone(MailForms::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(MailForms::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_mail_forms__kind").col(MailForms::Kind))
          .to_owned(),
      )
      .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // Tables
    manager.drop_table(Table::drop().if_exists().table(MailForms::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserVerifications::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserLogs::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserInvitations::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserBlocks::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(ApiKeys::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(UserPrefs::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Users::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Roles::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(Organizations::Table).to_owned()).await?;
    manager.drop_table(Table::drop().if_exists().table(AccessPolicies::Table).to_owned()).await?;

    // Functions
    db.execute_unprepared("DROP FUNCTION IF EXISTS manage_updated_at(in _tbl regclass);").await?;
    db.execute_unprepared("DROP FUNCTION IF EXISTS set_updated_at();").await?;

    // Extensions
    db.execute_unprepared("DROP EXTENSION IF EXISTS \"uuid-ossp\";").await?;

    Ok(())
  }
}

use ForeignKeyAction::*;
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;
use serde_json::json;

use super::helper::*;

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
  /// 이 계정이 마지막으로 자격을 재설정한 순간.
  CredentialsResetAt,
  CreatedAt,
  UpdatedAt,
}

#[derive(DeriveIden)]
enum Bans {
  Table,
  Id,

  UserId,

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
  CreatedBy,
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
enum AccessLogs {
  Table,
  Id,
  UserId,
  Method,
  Code,
  Count,
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
  KeyPrefix,
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
    attach_timestamp_auto_update(db, "organizations").await?;
    // END organizations table

    // BEGIN roles table
    manager
      .create_table(
        Table::create()
          .if_not_exists()
          .table(Roles::Table)
          .col(integer(Roles::Id).auto_increment())
          .col(string(Roles::Name))
          .col(string_null(Roles::Description))
          .col(timestamp_with_time_zone(Roles::CreatedAt).default(Expr::current_timestamp()))
          .col(timestamp_with_time_zone(Roles::UpdatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_roles__id").col(Roles::Id))
          .to_owned(),
      )
      .await?;
    attach_timestamp_auto_update(db, "roles").await?;
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
          .col(timestamp_with_time_zone_null(Users::CredentialsResetAt))
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
    attach_timestamp_auto_update(db, "users").await?;
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
          .col(string_null(ApiKeys::KeyPrefix))
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
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_api_keys__user_id")
          .table(ApiKeys::Table)
          .col(ApiKeys::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // END api_keys table

    // BEGIN bans table
    manager
      .create_table(
        Table::create()
          .table(Bans::Table)
          .col(integer(Bans::Id).auto_increment())
          .col(integer_null(Bans::UserId))
          .col(string_null(Bans::IpAddress).default("*"))
          .col(string_null(Bans::UserAgent).default("*"))
          .col(string_null(Bans::OsFamily).default("*"))
          .col(string_null(Bans::OsMajor).default("*"))
          .col(string_null(Bans::OsMinor).default("*"))
          .col(string_null(Bans::OsPatch).default("*"))
          .col(string_null(Bans::OsPatchMinor).default("*"))
          .col(string_null(Bans::DeviceFamily).default("*"))
          .col(string_null(Bans::DeviceBrand).default("*"))
          .col(string_null(Bans::DeviceModel).default("*"))
          .col(timestamp_with_time_zone(Bans::CreatedAt).default(Expr::current_timestamp()))
          .col(integer_null(Bans::CreatedBy))
          .col(string_null(Bans::Reason))
          .col(timestamp_with_time_zone_null(Bans::ExpiresAt))
          .col(timestamp_with_time_zone_null(Bans::LiftedAt))
          .col(string_null(Bans::Note))
          .primary_key(Index::create().name("pk_bans__id").col(Bans::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_bans__user_id")
              .from(Bans::Table, Bans::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .foreign_key(
            ForeignKey::create()
              .name("fk_bans__created_by")
              .from(Bans::Table, Bans::CreatedBy)
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
          .name("idx_bans__user_id")
          .table(Bans::Table)
          .col(Bans::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__ip_address")
          .table(Bans::Table)
          .col(Bans::IpAddress)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__user_agent")
          .table(Bans::Table)
          .col(Bans::UserAgent)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__os_family")
          .table(Bans::Table)
          .col(Bans::OsFamily)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__os_major")
          .table(Bans::Table)
          .col(Bans::OsMajor)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__os_minor")
          .table(Bans::Table)
          .col(Bans::OsMinor)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__os_patch")
          .table(Bans::Table)
          .col(Bans::OsPatch)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__os_patch_minor")
          .table(Bans::Table)
          .col(Bans::OsPatchMinor)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__device_brand")
          .table(Bans::Table)
          .col(Bans::DeviceBrand)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__device_family")
          .table(Bans::Table)
          .col(Bans::DeviceFamily)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .name("idx_bans__device_model")
          .table(Bans::Table)
          .col(Bans::DeviceModel)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // END bans table

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
    manager
      .create_index(
        Index::create()
          .if_not_exists()
          .table(UserInvitations::Table)
          .name("idx_user_invitations__user_id")
          .col(UserInvitations::UserId)
          .index_type(IndexType::BTree)
          .to_owned(),
      )
      .await?;
    // END user_invitations table

    // BEGIN access_logs
    manager
      .create_table(
        Table::create()
          .table(AccessLogs::Table)
          .col(integer(AccessLogs::Id).auto_increment())
          .col(integer(AccessLogs::UserId))
          .col(string(AccessLogs::Method))
          .col(integer(AccessLogs::Code))
          .col(integer_null(AccessLogs::Count))
          .col(string(AccessLogs::IpAddress))
          .col(string(AccessLogs::UserAgent))
          .col(string(AccessLogs::OsFamily))
          .col(string_null(AccessLogs::OsMajor))
          .col(string_null(AccessLogs::OsMinor))
          .col(string_null(AccessLogs::OsPatch))
          .col(string_null(AccessLogs::OsPatchMinor))
          .col(string(AccessLogs::DeviceFamily))
          .col(string_null(AccessLogs::DeviceBrand))
          .col(string_null(AccessLogs::DeviceModel))
          .col(timestamp_with_time_zone(AccessLogs::CreatedAt).default(Expr::current_timestamp()))
          .primary_key(Index::create().name("pk_access_logs__id").col(AccessLogs::Id))
          .foreign_key(
            ForeignKey::create()
              .name("fk_access_logs__user_id")
              .from(AccessLogs::Table, AccessLogs::UserId)
              .to(Users::Table, Users::Id)
              .on_delete(Cascade)
              .on_update(Cascade),
          )
          .to_owned(),
      )
      .await?;
    // END access_logs

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
    attach_timestamp_auto_update(db, "mail_forms").await?;

    // 비밀번호를 바꾸는 이유는 이미 내준 자격이 남의 손에 있을 수 있어서다. 토큰도 세션 쿠키도
    // 가져간 쪽이 든 것과 구별할 수 없으므로, 계정이 재설정 시각을 들고 로그인 경로가 그 이전에
    // 세워진 자격을 전부 거절한다.
    db.execute_unprepared(
      r#"
      COMMENT ON COLUMN users.credentials_reset_at IS
        'When this account last reset its credentials. Every token and session established up to it is refused; NULL means it never has.';
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    detach_timestamp_auto_update(db, "organizations").await?;
    detach_timestamp_auto_update(db, "roles").await?;
    detach_timestamp_auto_update(db, "users").await?;
    detach_timestamp_auto_update(db, "mail_forms").await?;

    drop_table(manager, MailForms::Table).await?;
    drop_table(manager, UserVerifications::Table).await?;
    drop_table(manager, AccessLogs::Table).await?;
    drop_table(manager, UserInvitations::Table).await?;
    drop_table(manager, Bans::Table).await?;
    drop_table(manager, ApiKeys::Table).await?;
    drop_table(manager, UserPrefs::Table).await?;
    drop_table(manager, Users::Table).await?;
    drop_table(manager, Roles::Table).await?;
    drop_table(manager, Organizations::Table).await?;

    // Functions
    db.execute_unprepared("DROP FUNCTION IF EXISTS manage_updated_at(in _tbl regclass);")
      .await?;
    db.execute_unprepared("DROP FUNCTION IF EXISTS set_updated_at();").await?;

    // Extensions
    db.execute_unprepared("DROP EXTENSION IF EXISTS \"uuid-ossp\";").await?;

    Ok(())
  }
}

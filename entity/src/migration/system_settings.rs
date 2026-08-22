use sea_orm_migration::prelude::*;

use super::helper::attach_timestamp_auto_update;

/// system_settings — 공용 운영 설정 key-value 테이블(feature "system-settings").
///
/// 값 하나가 곧 행 하나다. `category` 는 화면이 설정을 묶어 보여 주는 축이고, 키마다 하나로
/// 정해져 있으므로 그 키를 심는 마이그레이션이 함께 정한다. sumida 자신의 키는 여기서 심고,
/// 그 위에 선 서비스의 키는 각자의 마이그레이션이 심는다.
///
/// `updated_by` 로 users 를 참조하므로 소비자 Migrator 에서 sumida init 다음에 배치한다.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      r#"
      CREATE TYPE system_setting_data_type_enum AS ENUM
        ('string','integer','number','boolean','duration_days','json');

      CREATE TABLE system_settings (
        key         TEXT PRIMARY KEY,
        value       JSONB NOT NULL DEFAULT 'null'::jsonb,
        data_type   system_setting_data_type_enum NOT NULL,
        label       TEXT NOT NULL DEFAULT '',
        description TEXT,
        min         JSONB,
        max         JSONB,
        is_editable BOOLEAN NOT NULL DEFAULT TRUE,
        category    TEXT NOT NULL DEFAULT 'general',
        updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_by  INTEGER REFERENCES users(id) ON UPDATE CASCADE ON DELETE SET NULL
      );
      "#,
    )
    .await?;

    attach_timestamp_auto_update(db, "system_settings").await?;

    // sumida 자신의 설정 시드(인증 정책).
    db.execute_unprepared(
      r#"
      INSERT INTO system_settings
        (key, value, data_type, label, description, min, max, is_editable, category) VALUES
        ('security.ban.enabled', 'true'::jsonb, 'boolean',
         'system_settings.security.ban.enabled.label',
         'system_settings.security.ban.enabled.description',
         NULL, NULL, true, 'auth'),
        ('security.ban.max_failed_attempts', '10'::jsonb, 'integer',
         'system_settings.security.ban.max_failed_attempts.label',
         'system_settings.security.ban.max_failed_attempts.description',
         '1'::jsonb, '1000'::jsonb, true, 'auth'),
        ('security.ban.failure_reset_after', '"none"'::jsonb, 'string',
         'system_settings.security.ban.failure_reset_after.label',
         'system_settings.security.ban.failure_reset_after.description',
         NULL, NULL, true, 'auth'),
        ('security.ban.duration', '"forever"'::jsonb, 'string',
         'system_settings.security.ban.duration.label',
         'system_settings.security.ban.duration.description',
         NULL, NULL, true, 'auth'),
        ('security.ban.target', '"ip"'::jsonb, 'string',
         'system_settings.security.ban.target.label',
         'system_settings.security.ban.target.description',
         NULL, NULL, true, 'auth')
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    db.execute_unprepared("DROP TABLE IF EXISTS system_settings CASCADE;").await?;
    db.execute_unprepared("DROP TYPE IF EXISTS system_setting_data_type_enum;").await?;
    Ok(())
  }
}

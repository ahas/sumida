use sea_orm_migration::prelude::*;

/// system_settings 갈래 컬럼과 sumida 자신의 설정 시드(feature "system-settings").
///
/// 테이블을 만드는 마이그레이션은 이미 배포된 데이터베이스에 적용을 마쳤고 sea-orm 은 이름으로만
/// 추적하므로, 그 파일을 고쳐서는 기존 데이터베이스에 아무것도 더할 수 없다. 그래서 별도
/// 마이그레이션이며 전부 재실행 가능하다. 소비자 Migrator 에서 system_settings 테이블 생성 다음에
/// 배치한다.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      "ALTER TABLE system_settings ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT 'general';",
    )
    .await?;

    // sumida 자신의 설정 시드(인증 정책). 그 위에 선 서비스의 키는 각자의 마이그레이션이 심는다.
    // 운영자가 고쳐 둔 값을 덮지 않도록 ON CONFLICT DO NOTHING.
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
      ON CONFLICT (key) DO NOTHING
      "#,
    )
    .await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    db.execute_unprepared("DELETE FROM system_settings WHERE key LIKE 'security.ban.%';")
      .await?;
    db.execute_unprepared("ALTER TABLE system_settings DROP COLUMN IF EXISTS category;")
      .await?;
    Ok(())
  }
}

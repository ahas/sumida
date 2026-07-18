use sea_orm_migration::prelude::*;

use super::helper::attach_timestamp_auto_update;

/// system_settings — 공용 운영 설정 key-value 테이블(feature "system-settings").
///
/// 이 마이그레이션 도입 전에 각 앱(site-app/trainer)이 자체 마이그레이션으로 이미 테이블을 만들어
/// 배포한 상태이므로, **멱등**하게(IF NOT EXISTS·enum/FK 존재 검사) 작성한다. 기존 배포 DB 에선
/// no-op, 신규 DB 에선 생성. users 테이블(FK updated_by)에 의존하므로 소비자 Migrator 에서
/// sumida init 다음·시드 마이그레이션 이전에 배치한다.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    // enum 타입(멱등 — Postgres CREATE TYPE 은 IF NOT EXISTS 미지원이라 DO 블록으로).
    db.execute_unprepared(
      r#"
      DO $$ BEGIN
        IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'system_setting_data_type_enum') THEN
          CREATE TYPE system_setting_data_type_enum AS ENUM
            ('string','integer','number','boolean','duration_days','json');
        END IF;
      END $$;
      "#,
    )
    .await?;

    db.execute_unprepared(
      r#"
      CREATE TABLE IF NOT EXISTS system_settings (
        key         TEXT PRIMARY KEY,
        value       JSONB NOT NULL DEFAULT 'null'::jsonb,
        data_type   system_setting_data_type_enum NOT NULL,
        label       TEXT NOT NULL DEFAULT '',
        description  TEXT,
        min         JSONB,
        max         JSONB,
        is_editable BOOLEAN NOT NULL DEFAULT TRUE,
        updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_by  INTEGER
      );
      "#,
    )
    .await?;

    // updated_by → users(id) FK (멱등 — 제약명 존재 검사).
    db.execute_unprepared(
      r#"
      DO $$ BEGIN
        IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_system_settings__updated_by') THEN
          ALTER TABLE system_settings ADD CONSTRAINT fk_system_settings__updated_by
            FOREIGN KEY (updated_by) REFERENCES users(id) ON UPDATE CASCADE ON DELETE SET NULL;
        END IF;
      END $$;
      "#,
    )
    .await?;

    // updated_at 자동갱신 트리거 — manage_updated_at 은 CREATE TRIGGER 라 비멱등이므로
    // 기존 DB 대비 먼저 드롭 후 재부착.
    db.execute_unprepared("DROP TRIGGER IF EXISTS set_updated_at ON system_settings;")
      .await?;
    attach_timestamp_auto_update(db, "system_settings").await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    db.execute_unprepared("DROP TABLE IF EXISTS system_settings CASCADE;")
      .await?;
    db.execute_unprepared("DROP TYPE IF EXISTS system_setting_data_type_enum;")
      .await?;
    Ok(())
  }
}

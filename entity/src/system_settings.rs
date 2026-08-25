use error::SumidaError;
use sea_orm::ActiveValue::*;
use sea_orm::IntoActiveModel;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// sumida 자신이 소유하는 설정 키. 인증 정책(로그인 실패 잠금)은 기반 층의 관심사이므로 여기 산다.
pub mod keys {
  pub const SECURITY_BAN_ENABLED: &str = "security.ban.enabled";
  pub const SECURITY_BAN_MAX_FAILED_ATTEMPTS: &str = "security.ban.max_failed_attempts";
  pub const SECURITY_BAN_FAILURE_RESET_AFTER: &str = "security.ban.failure_reset_after";
  pub const SECURITY_BAN_DURATION: &str = "security.ban.duration";
  pub const SECURITY_BAN_TARGET: &str = "security.ban.target";
}

/// 시스템 설정(운영 설정 key-value) — 공용 feature `system-settings`.
///
/// key-value 운영 설정 저장소. 이 엔티티는 스키마·접근 헬퍼와 함께 sumida 자신이 쓰는 키 상수
/// (`keys` 모듈)만 소유한다. 그 위에 선 서비스의 키 상수는 각 소비자 크레이트가 자체 `keys`
/// 모듈로 소유하고, 어느 쪽도 상대의 키 목록을 알지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, Serialize, Deserialize, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "system_setting_data_type_enum")]
pub enum SystemSettingDataType {
  #[sea_orm(string_value = "string")]
  #[serde(rename = "string")]
  String,
  #[sea_orm(string_value = "integer")]
  #[serde(rename = "integer")]
  Integer,
  #[sea_orm(string_value = "number")]
  #[serde(rename = "number")]
  Number,
  #[sea_orm(string_value = "boolean")]
  #[serde(rename = "boolean")]
  Boolean,
  #[sea_orm(string_value = "duration_days")]
  #[serde(rename = "duration_days")]
  DurationDays,
  #[sea_orm(string_value = "json")]
  #[serde(rename = "json")]
  Json,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "system_settings")]
pub struct Model {
  #[sea_orm(primary_key, auto_increment = false)]
  pub key:         String,
  #[sea_orm(column_type = "JsonBinary")]
  pub value:       Json,
  pub data_type:   SystemSettingDataType,
  pub label:       String,
  pub description: Option<String>,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub min:         Option<Json>,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub max:         Option<Json>,
  pub is_editable: bool,
  /// 설정이 속한 갈래 — auth·execution·retention·storage·general. 선언하지 않은 행은 general.
  pub category:    String,
  pub updated_at:  DateTimeUtc,
  pub updated_by:  Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
  #[sea_orm(
    belongs_to = "super::users::Entity",
    from = "Column::UpdatedBy",
    to = "super::users::Column::Id",
    on_update = "Cascade",
    on_delete = "SetNull"
  )]
  UpdatedByUser,
}

impl ActiveModelBehavior for ActiveModel {}

impl Entity {
  pub async fn get_i64<C>(key: &str, db: &'_ C) -> Result<Option<i64>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(Self::find_by_id(key).one(db).await?.and_then(|m| m.value.as_i64()))
  }

  pub async fn get_bool<C>(key: &str, db: &'_ C) -> Result<Option<bool>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(Self::find_by_id(key).one(db).await?.and_then(|m| m.value.as_bool()))
  }

  pub async fn get_string<C>(key: &str, db: &'_ C) -> Result<Option<String>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(Self::find_by_id(key).one(db).await?.and_then(|m| m.value.as_str().map(String::from)))
  }

  /// JSON 값을 그대로 반환. caller 가 자료형을 알고 있는 경우 사용.
  pub async fn get_raw<C>(key: &str, db: &'_ C) -> Result<Option<Json>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(Self::find_by_id(key).one(db).await?.map(|m| m.value))
  }

  /// 값을 갱신. `data_type` 정합성 검증은 service 계층에서 수행한다.
  pub async fn set_value<C>(
    key: &str,
    value: Json,
    updated_by: Option<i32>,
    db: &'_ C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    let Some(existing) = Self::find_by_id(key).one(db).await? else {
      return Err(SumidaError::NoData("SystemSetting".to_owned(), key.to_owned()));
    };
    if !existing.is_editable {
      return Err(SumidaError::Unsupported(format!("system setting `{key}` is not editable")));
    }
    let mut active = existing.into_active_model();
    active.value = Set(value);
    active.updated_by = Set(updated_by);
    active.updated_at = Set(chrono::Utc::now());
    Self::update(active).exec(db).await?;
    Ok(())
  }

  /// 없으면 생성(자동등록 기본 메타), 있으면 갱신 — 멱등. `set_value` 는 update-only(신규 key 는
  /// NoData)라, 스테이지별 외부 작업 id 처럼 서버가 런타임에 새로 등록하는 설정은 이걸 쓴다
  /// (새 스테이지 추가 시 setting 행이 없어 set_value 가 실패하던 문제).
  pub async fn upsert_value<C>(
    key: &str,
    value: Json,
    data_type: SystemSettingDataType,
    label: &str,
    updated_by: Option<i32>,
    db: &'_ C,
  ) -> Result<(), SumidaError>
  where
    C: ConnectionTrait,
  {
    // 자동관리 setting(스테이지별 자동 등록 키 등)은 is_editable 체크를 우회해 서버가 직접 갱신한다
    // — set_value 는 is_editable=false 를 거부하는데(사용자 편집 금지), 서버 자동 재등록은 허용해야 한다.
    if let Some(existing) = Self::find_by_id(key).one(db).await? {
      let mut active = existing.into_active_model();
      active.value = Set(value);
      active.updated_by = Set(updated_by);
      active.updated_at = Set(chrono::Utc::now());
      Self::update(active).exec(db).await?;
      return Ok(());
    }
    let active = ActiveModel {
      key:         Set(key.to_owned()),
      value:       Set(value),
      data_type:   Set(data_type),
      label:       Set(label.to_owned()),
      description: Set(None),
      min:         Set(None),
      max:         Set(None),
      is_editable: Set(false),
      // 자동등록 setting 은 갈래를 선언하지 않는다 — 컬럼 기본값(general)이 붙는다.
      category:    NotSet,
      updated_at:  Set(chrono::Utc::now()),
      updated_by:  Set(updated_by),
    };
    Self::insert(active).exec(db).await?;
    Ok(())
  }
}

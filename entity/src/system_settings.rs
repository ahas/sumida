use error::SumidaError;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::*, IntoActiveModel};
use serde::{Deserialize, Serialize};

/// 시스템 설정(운영 설정 key-value) — 공용 feature `system-settings`.
///
/// key-value 운영 설정 저장소. 앱별 키 상수(예: `site-app.*`, `task.purge.*`)는 각 소비자
/// 크레이트가 자체 `keys` 모듈로 소유하고, 이 엔티티는 스키마와 접근 헬퍼만 제공한다.
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, Serialize, Deserialize, DeriveActiveEnum)]
#[sea_orm(
  rs_type = "String",
  db_type = "Enum",
  enum_name = "system_setting_data_type_enum"
)]
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
  pub key: String,
  #[sea_orm(column_type = "JsonBinary")]
  pub value: Json,
  pub data_type: SystemSettingDataType,
  pub label: String,
  pub description: Option<String>,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub min: Option<Json>,
  #[sea_orm(column_type = "JsonBinary", nullable)]
  pub max: Option<Json>,
  pub is_editable: bool,
  pub updated_at: DateTimeWithTimeZone,
  pub updated_by: Option<i32>,
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
    Ok(
      Self::find_by_id(key)
        .one(db)
        .await?
        .and_then(|m| m.value.as_i64()),
    )
  }

  pub async fn get_bool<C>(key: &str, db: &'_ C) -> Result<Option<bool>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(
      Self::find_by_id(key)
        .one(db)
        .await?
        .and_then(|m| m.value.as_bool()),
    )
  }

  pub async fn get_string<C>(key: &str, db: &'_ C) -> Result<Option<String>, SumidaError>
  where
    C: ConnectionTrait,
  {
    Ok(
      Self::find_by_id(key)
        .one(db)
        .await?
        .and_then(|m| m.value.as_str().map(String::from)),
    )
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
      return Err(SumidaError::Unsupported(format!(
        "system setting `{key}` is not editable"
      )));
    }
    let mut active = existing.into_active_model();
    active.value = Set(value);
    active.updated_by = Set(updated_by);
    active.updated_at = Set(chrono::Utc::now().fixed_offset());
    Self::update(active).exec(db).await?;
    Ok(())
  }
}

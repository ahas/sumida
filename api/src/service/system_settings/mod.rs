pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use sea_orm::{ConnectionTrait, EntityTrait};
use serde_json::Value as JsonValue;

pub use self::dto::*;

// 시스템 설정 서비스 — 소비 서비스 공용 key-value 운영 설정 CRUD.
//
// 네임스페이스 비의존(자유 함수): 라우트별 접근 게이트(service_ap!)와 갱신자 세션은 각 소비자
// 컨트롤러가 소유한다. 세션은 갱신자 기록(updated_by)용으로만 컨트롤러에서 넘어온다.

pub async fn list<C: ConnectionTrait>(db: &C) -> Result<Vec<SystemSettingItem>, SumidaError> {
  Ok(SystemSettings::find().into_model::<SystemSettingItem>().all(db).await?)
}

pub async fn get<C: ConnectionTrait>(db: &C, key: &str) -> Result<Option<SystemSettingItem>, SumidaError> {
  Ok(SystemSettings::find_by_id(key).into_model::<SystemSettingItem>().one(db).await?)
}

/// 값 갱신 — data_type 정합성·min/max 범위 검증 후 entity `set_value` 위임
/// (is_editable 검사는 `set_value` 안에서 수행).
pub async fn update<C: ConnectionTrait>(
  db: &C,
  key: &str,
  value: JsonValue,
  updated_by: Option<i32>,
) -> Result<(), SumidaError> {
  let Some(existing) = SystemSettings::find_by_id(key).one(db).await? else {
    return Err(SumidaError::NoData("SystemSetting".into(), key.to_owned()));
  };

  validate_value_type(&existing.data_type, &value)?;
  validate_range(&existing.data_type, &value, existing.min.as_ref(), existing.max.as_ref())?;

  SystemSettings::set_value(key, value, updated_by, db).await
}

fn validate_value_type(data_type: &SystemSettingDataType, value: &JsonValue) -> Result<(), SumidaError> {
  let ok = match data_type {
    SystemSettingDataType::String => value.is_string(),
    SystemSettingDataType::Integer | SystemSettingDataType::DurationDays => value.as_i64().is_some(),
    SystemSettingDataType::Number => value.is_number(),
    SystemSettingDataType::Boolean => value.is_boolean(),
    SystemSettingDataType::Json => true,
  };
  if ok {
    Ok(())
  } else {
    Err(SumidaError::Unsupported(format!(
      "value type mismatch: expected {data_type:?}, got {value}"
    )))
  }
}

fn validate_range(
  data_type: &SystemSettingDataType,
  value: &JsonValue,
  min: Option<&JsonValue>,
  max: Option<&JsonValue>,
) -> Result<(), SumidaError> {
  use SystemSettingDataType::*;
  match data_type {
    Integer | DurationDays => {
      let v = value.as_i64().unwrap_or_default();
      if let Some(m) = min.and_then(|v| v.as_i64())
        && v < m
      {
        return Err(SumidaError::Unsupported(format!("value {v} < min {m}")));
      }
      if let Some(m) = max.and_then(|v| v.as_i64())
        && v > m
      {
        return Err(SumidaError::Unsupported(format!("value {v} > max {m}")));
      }
    }
    Number => {
      let v = value.as_f64().unwrap_or_default();
      if let Some(m) = min.and_then(|v| v.as_f64())
        && v < m
      {
        return Err(SumidaError::Unsupported(format!("value {v} < min {m}")));
      }
      if let Some(m) = max.and_then(|v| v.as_f64())
        && v > m
      {
        return Err(SumidaError::Unsupported(format!("value {v} > max {m}")));
      }
    }
    _ => {}
  }
  Ok(())
}

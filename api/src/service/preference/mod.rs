pub mod dto;

use core::db;
use entity::prelude::*;
use error::SumidaError;
use sea_orm::{EntityTrait, Set, Unchanged};
use serde_json::json;

pub struct PreferenceService;

impl PreferenceService {
  pub async fn get(user_id: i32) -> Result<serde_json::Value, SumidaError> {
    let db_conn = db::get_connection();
    let pref = Preferences::find_by_id(user_id).one(db_conn).await?;

    Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
  }

  pub async fn update(user_id: i32, data: serde_json::Value) -> Result<(), SumidaError> {
    use entity::preferences::ActiveModel;

    let db_conn = db::get_connection();
    let pref = Preferences::find_by_id(user_id).one(db_conn).await?;

    match pref {
      Some(_) => {
        Preferences::update(ActiveModel {
          user_id: Unchanged(user_id),
          data: Set(data),
        })
        .exec(db_conn)
        .await?;
      }
      None => {
        Preferences::insert(ActiveModel {
          user_id: Set(user_id),
          data: Set(data),
        })
        .exec(db_conn)
        .await?;
      }
    }

    Ok(())
  }
}

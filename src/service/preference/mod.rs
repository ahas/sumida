pub mod dto;

use crate::core::db;
use crate::core::error::AppError;
use crate::entity::prelude::*;

use sea_orm::{EntityTrait, Set, Unchanged};
use serde_json::json;

pub async fn get(username: String) -> Result<serde_json::Value, AppError> {
  let db_conn = db::get_connection();
  let pref = Preferences::find_by_id(username).one(db_conn).await?;

  Ok(pref.map(|x| x.data).unwrap_or_else(|| json!({})))
}

pub async fn update(username: String, data: serde_json::Value) -> Result<(), AppError> {
  use crate::entity::preferences::ActiveModel;

  let db_conn = db::get_connection();
  let pref = Preferences::find_by_id(username.clone()).one(db_conn).await?;

  match pref {
    Some(_) => {
      Preferences::update(ActiveModel {
        username: Unchanged(username.clone()),
        data: Set(data),
      })
      .exec(db_conn)
      .await?;
    }
    None => {
      Preferences::insert(ActiveModel {
        username: Set(username.clone()),
        data: Set(data),
      })
      .exec(db_conn)
      .await?;
    }
  }

  Ok(())
}

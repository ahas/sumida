use error::SumidaError;
use sea_orm::{ConnectionTrait, EntityTrait, Select};

use crate::util::ap::ApGuard;

pub mod access_log;
pub mod access_policy;
pub mod admin;
pub mod api_key;
pub mod auth;
pub mod ban;
#[cfg(feature = "board")]
pub mod bps;
#[cfg(feature = "file")]
pub mod fs;
#[cfg(feature = "mail-form")]
pub mod mail_form;
pub mod me;
#[cfg(feature = "membership")]
pub mod membership;
pub mod monitoring;
pub mod organization;
pub mod role;
#[cfg(feature = "system-settings")]
pub mod system_settings;
pub mod user;
pub mod user_invitation;
pub mod user_verification;

pub mod prelude;

pub trait ApiService<A: ApGuard, E: EntityTrait> {
  fn default_query_with(query: Select<E>) -> Select<E> {
    query
  }

  fn ap(&self) -> A;

  fn try_access<Action, C>(
    &self,
    action: Action,
    query: Select<E>,
    db: &C,
  ) -> impl Future<Output = Result<(), SumidaError>>
  where
    Action: AsRef<str>,
    C: ConnectionTrait,
  {
    self.ap().action(action.as_ref()).try_access(Self::default_query_with(query), db)
  }

  fn select(&self, query: Select<E>) -> Select<E> {
    self.ap().action("read").apply(Self::default_query_with(query))
  }
}

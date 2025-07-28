use core::session::UserContext;

use sea_orm::{EntityTrait, Select};

use crate::util::ap::Ap;

pub mod admin;
pub mod api_key;
pub mod auth;
pub mod bs;
pub mod mail_form;
pub mod me;
pub mod organization;
pub mod role;
pub mod user;
pub mod user_block;
pub mod user_invitation;
pub mod user_log;
pub mod user_verification;

pub mod prelude;

pub trait ApiService<E: EntityTrait> {
  fn select(user_ctx: &UserContext, query: Select<E>) -> Select<E>;
  fn ap(user_ctx: &UserContext) -> Ap;
}

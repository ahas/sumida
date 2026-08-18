pub mod access_log;
pub mod api_key;
pub mod auth;
pub mod authz;
pub mod ban;
#[cfg(feature = "board")]
pub mod bps;
pub mod dashboard;
#[cfg(feature = "file")]
pub mod fs;
#[cfg(feature = "mail-form")]
pub mod mail_form;
pub mod me;
#[cfg(feature = "membership")]
pub mod membership;
pub mod organization;
pub mod relation_tuple;
pub mod role;
pub mod user;
pub mod user_invitation;
pub mod user_verification;

pub mod prelude;

pub trait ApiController<StateT>
where
  StateT: Clone + Send + Sync + 'static,
{
  fn router(self, state: &StateT) -> axum::Router<StateT>;

  fn mount(self, prefix: &str, state: &StateT, root: axum::Router<StateT>) -> axum::Router<StateT>
  where
    Self: Sized,
  {
    root.nest(prefix, self.router(state))
  }
}

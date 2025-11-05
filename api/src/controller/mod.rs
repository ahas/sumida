pub mod access_log;
pub mod access_policy;
pub mod api_key;
pub mod auth;
pub mod ban;
pub mod bs;
pub mod fs;
pub mod mail_form;
pub mod me;
pub mod organization;
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

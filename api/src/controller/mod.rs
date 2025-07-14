pub mod api_key;
pub mod bs;
pub mod mail_form;
pub mod organization;
pub mod preference;
pub mod role;
pub mod user;
pub mod user_block;
pub mod user_invitation;
pub mod user_log;
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

use serde::Serialize;

pub trait RoleAbilityProvider<T>
where
  T: Default + Serialize,
{
  fn get() -> T {
    T::default()
  }
}

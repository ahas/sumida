use serde::Serialize;

pub trait RoleProvider<T>
where
  T: Default + Serialize,
{
  fn get() -> T {
    T::default()
  }
}

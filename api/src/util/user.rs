use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};

pub fn hash_password<P>(password: P) -> argon2::password_hash::Result<String>
where
  P: AsRef<str>,
{
  let argon2 = Argon2::default();
  let salt = SaltString::generate(&mut OsRng);

  Ok(argon2.hash_password(password.as_ref().as_bytes(), &salt)?.to_string())
}

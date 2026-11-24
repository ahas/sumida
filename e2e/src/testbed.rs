//! The configuration a run under BMO's testbed is handed.
//!
//! The testbed names its own PostgreSQL server in `BMO_TEST_POSTGRES` and hands the tests nothing else. Where
//! that variable is set and nothing names the admin database, the harness takes both of its databases on that
//! server and the rest of its settings from the workspace's `settings.e2e.example.toml`, so the suite runs
//! with no `.env.e2e` of the machine's own. A variable the environment already holds is kept.

use url::Url;

const SERVER: &str = "BMO_TEST_POSTGRES";
const ADMIN_DATABASE: &str = "postgres";
const TEST_DATABASE: &str = "sumida_test";
const SESSION_STORE: &str = "redis://127.0.0.1:6379/9";
const SETTINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../settings.e2e.example.toml");

/// Fills in the harness's variables from the testbed's server, where the testbed names one and the caller
/// names no admin database.
pub fn configure() {
  let Some(server) = std::env::var_os(SERVER).and_then(|value| value.into_string().ok()) else {
    return;
  };

  if std::env::var_os("DATABASE_ADMIN_URL").is_some() {
    return;
  }

  let on = |database: &str| {
    let mut address = Url::parse(&server).unwrap_or_else(|error| panic!("{SERVER} is no address: {error}"));

    address.set_path(&format!("/{database}"));
    address.to_string()
  };
  let variables = [
    ("DATABASE_ADMIN_URL", on(ADMIN_DATABASE)),
    ("SUMIDA_SETTINGS", SETTINGS.to_owned()),
    ("SUMIDA_DATABASE__URL", on(TEST_DATABASE)),
    // The address a test that connects on its own terms reads, as `.env.e2e` names it beside the settings.
    ("DATABASE_URL", on(TEST_DATABASE)),
    ("SUMIDA_SESSION__REDIS__URL", SESSION_STORE.to_owned()),
  ];

  for (name, value) in variables {
    if std::env::var_os(name).is_none() {
      // SAFETY: called once, from `init_once`, before the settings are first read; every other test of the
      // binary waits on the same cell, as it does while `dotenvy` sets the variables of `.env.e2e`.
      unsafe { std::env::set_var(name, value) };
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_settings_file_is_the_workspace_example() {
    assert!(std::path::Path::new(SETTINGS).is_file(), "{SETTINGS} does not exist");
  }
}

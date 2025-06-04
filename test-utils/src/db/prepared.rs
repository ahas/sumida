//! A ready-to-use `MockDatabase` seeded with one
//!  • Organization  (id = 10, “ACME”)
//!  • Role          (id = 1,  “admin”)
//!  • User          (username = “alice”, org = 10, role = 1)
//!
//! The queue order matches the queries that `service/user`
/// functions (`sign_in`, `list`, etc.) typically emit:
///   1. SELECT user      → alice
///   2. SELECT role      → admin
///   3. SELECT org       → ACME
///   4. COUNT(*)         → 1
///   5. UPDATE / INSERT  → rows_affected = 1
///
/// If your test needs a different order or extra rows,
/// copy this file and tweak the `append_*` sequence.
use sea_orm::{
  DatabaseBackend,
  MockDatabase,
  MockExecResult,
  prelude::*,
  sea_query::Value, // for count(*)
};

use crate::fixtures::{mk_org, mk_role, mk_user};

/// Return a `DatabaseConnection` backed by `MockDatabase`.
/// Suitable for **fast, in-process “service-level” tests**.
pub fn prepared_db() -> DatabaseConnection {
  // 1) Seed rows
  let org = mk_org(10, "ACME", None); // organization_id = 10
  let role = mk_role(1, "admin"); // role_id = 1
  let user = mk_user("alice", "pw123", org.id, role.id); // username = alice

  // 2) Build MockDatabase queue
  MockDatabase::new(DatabaseBackend::Postgres)
        // Users::find_by_id("alice")
        .append_query_results([vec![user.clone().into()]])
        // Roles::find_by_id(1)
        .append_query_results([vec![role.clone().into()]])
        // Organizations::find_by_id(10)
        .append_query_results([vec![org.clone().into()]])
        // SELECT COUNT(*)
        .append_query_results([vec![Value::Int(Some(1)).into()]])
        // UPDATE / INSERT exec result
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection()
}

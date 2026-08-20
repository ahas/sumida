use e2e::prelude::*;

/// A deadlock is a retryable conflict, and the decision must not depend on the language the server
/// speaks.
///
/// Two transactions take the same two rows in opposite orders. Each ends up holding what the other
/// waits for, the database kills one of them with SQLSTATE `40P01`, and the survivor commits. The
/// killed one is the transaction the caller is meant to run again — so the helper that decides
/// whether to retry has to recognise it.
#[tokio::test]
async fn a_deadlock_is_a_retryable_conflict() {
  use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
  use tokio::sync::Barrier;

  let _mock = Mock::new().await;
  let db = infra::db::connect().await;

  let first = TestOrg::create(&db).await.expect("seed the first organization").model.id;
  let second = TestOrg::create(&db).await.expect("seed the second organization").model.id;

  let lock = |id: i32| {
    Statement::from_string(
      DbBackend::Postgres,
      format!("SELECT id FROM organizations WHERE id = {id} FOR UPDATE"),
    )
  };

  // Both transactions must be holding their first row before either reaches for the second one.
  let both_hold_one = std::sync::Arc::new(Barrier::new(2));

  let mut crossing = Vec::with_capacity(2);
  for (mine, theirs) in [(first, second), (second, first)] {
    let db = db.clone();
    let both_hold_one = both_hold_one.clone();

    crossing.push(tokio::spawn(async move {
      let txn = db.begin().await?;
      txn.execute(lock(mine)).await?;

      both_hold_one.wait().await;

      txn.execute(lock(theirs)).await?;
      txn.commit().await?;

      Ok::<(), sea_orm::DbErr>(())
    }));
  }

  let mut killed = Vec::new();
  for crosser in crossing {
    if let Err(err) = crosser.await.expect("the crossing task ran to completion") {
      killed.push(err);
    }
  }

  assert_eq!(killed.len(), 1, "the database must kill exactly one of two crossed transactions");
  assert!(
    api::util::db::is_retryable_conflict(&killed[0]),
    "a deadlock must be recognised as retryable, got: {}",
    killed[0],
  );
}

/// An error that did not come from the database is not something running the transaction again can
/// fix, and neither is an ordinary constraint violation.
#[tokio::test]
async fn an_error_that_is_not_a_conflict_is_not_retryable() {
  use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};

  let _mock = Mock::new().await;
  let db = infra::db::connect().await;

  let syntax_error = db
    .execute(Statement::from_string(DbBackend::Postgres, "SELECT this_column_does_not_exist"))
    .await
    .expect_err("the statement cannot succeed");
  assert!(!api::util::db::is_retryable_conflict(&syntax_error));

  assert!(!api::util::db::is_retryable_conflict(&DbErr::Custom(
    "not from the database".to_owned()
  )));
  assert!(!api::util::db::is_retryable_conflict(&DbErr::RecordNotFound("nothing here".to_owned())));
}

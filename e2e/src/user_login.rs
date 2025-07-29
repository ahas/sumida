use crate::db::setup_test_db;

#[tokio::test]
async fn test_db_setup() {
  setup_test_db().await.expect("DB setup failed");
  // 이후 테스트 작성...
}

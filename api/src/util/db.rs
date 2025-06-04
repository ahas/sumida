use sea_orm::DbErr;

pub fn is_serialization_failure(error: &DbErr) -> bool {
  // PostgreSQL의 직렬화 실패 오류 코드 40001을 확인합니다.
  match error {
    DbErr::Query(err) => err.to_string().contains("could not serialize access"),
    DbErr::Exec(err) => err.to_string().contains("could not serialize access"),
    _ => false,
  }
}

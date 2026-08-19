use axum::Json;
use axum::response::IntoResponse;
use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::{ConnectionTrait, DbErr, FromQueryResult, Statement};
use serde::Serialize;

/// How far back the authentication and storage views look, counting today.
const AUTH_WINDOW_DAYS: i64 = 14;
/// How many offending addresses a caller is shown. Beyond a handful the list stops being read.
const TOP_ADDRESS_LIMIT: i64 = 5;
/// How many media types are named before the rest are summed into one row.
const TOP_MIME_LIMIT: i64 = 6;

/// One day of sign-in attempts, split by whether they were let through.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AuthDailyItem {
  pub date:      chrono::NaiveDate,
  pub succeeded: i64,
  pub refused:   i64,
}

/// An address that has been refused repeatedly, and what it was refused for.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct AuthOffender {
  pub ip_address:   String,
  pub refused:      i64,
  pub last_attempt: DateTime<FixedOffset>,
}

/// What the sign-in log says about the recent past.
///
/// A refusal is any result other than success, and the reasons differ in kind: a wrong password is
/// a person mistyping, a banned account is a decision already taken, an unknown account from one
/// address repeated is somebody guessing. The daily split answers "is this normal", the offender
/// list answers "who".
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthActivity {
  /// The first and last day the series covers, so the client draws the window that was asked for
  /// rather than the days that happened to have an attempt.
  pub from:       chrono::NaiveDate,
  pub to:         chrono::NaiveDate,
  pub daily:      Vec<AuthDailyItem>,
  pub offenders:  Vec<AuthOffender>,
  pub created_at: DateTime<FixedOffset>,
}

impl IntoResponse for AuthActivity {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl AuthActivity {
  pub async fn new<C: ConnectionTrait>(db: &C) -> Result<Self, DbErr> {
    let backend = db.get_database_backend();

    let daily = AuthDailyItem::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT
        date_trunc('day', created_at)::date AS "date"
        , count(*) FILTER (WHERE code = 0) AS "succeeded"
        , count(*) FILTER (WHERE code <> 0) AS "refused"
      FROM access_logs
      WHERE created_at >= CURRENT_DATE - ($1::int - 1)
      GROUP BY "date"
      ORDER BY "date"
      "#,
      [AUTH_WINDOW_DAYS.into()],
    ))
    .all(db)
    .await?;

    let offenders = AuthOffender::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT
        ip_address
        , count(*) AS "refused"
        , max(created_at) AS "last_attempt"
      FROM access_logs
      WHERE code <> 0 AND created_at >= CURRENT_DATE - ($1::int - 1)
      GROUP BY ip_address
      ORDER BY "refused" DESC, "last_attempt" DESC
      LIMIT $2
      "#,
      [AUTH_WINDOW_DAYS.into(), TOP_ADDRESS_LIMIT.into()],
    ))
    .all(db)
    .await?;

    let today = Utc::now().date_naive();

    Ok(Self {
      from: today - chrono::Duration::days(AUTH_WINDOW_DAYS - 1),
      to: today,
      daily,
      offenders,
      created_at: Utc::now().fixed_offset(),
    })
  }
}

/// One media type and what it occupies.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct StorageKind {
  pub mime_type: String,
  pub files:     i64,
  pub bytes:     i64,
}

#[derive(Clone, Debug, Serialize, FromQueryResult)]
struct StorageTotal {
  files:        i64,
  bytes:        i64,
  unreferenced: i64,
}

/// What the file store holds.
///
/// The total is every stored file, and the breakdown names the heaviest media types with the rest
/// summed into one row — a list with a row per type would be mostly noise. `unreferenced` counts
/// files nothing points at any more, which is the number worth acting on.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
  pub files:        i64,
  pub bytes:        i64,
  pub unreferenced: i64,
  pub kinds:        Vec<StorageKind>,
  pub created_at:   DateTime<FixedOffset>,
}

impl IntoResponse for StorageUsage {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl StorageUsage {
  pub async fn new<C: ConnectionTrait>(db: &C) -> Result<Self, DbErr> {
    let backend = db.get_database_backend();

    let total = StorageTotal::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT
        count(*) AS "files"
        , COALESCE(sum(size), 0)::bigint AS "bytes"
        , count(*) FILTER (WHERE refs <= 0) AS "unreferenced"
      FROM files
      "#,
      [],
    ))
    .one(db)
    .await?
    .unwrap_or(StorageTotal {
      files:        0,
      bytes:        0,
      unreferenced: 0,
    });

    let kinds = StorageKind::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT mime_type, count(*) AS "files", COALESCE(sum(size), 0)::bigint AS "bytes"
      FROM files
      GROUP BY mime_type
      ORDER BY "bytes" DESC
      LIMIT $1
      "#,
      [TOP_MIME_LIMIT.into()],
    ))
    .all(db)
    .await?;

    Ok(Self {
      files: total.files,
      bytes: total.bytes,
      unreferenced: total.unreferenced,
      kinds,
      created_at: Utc::now().fixed_offset(),
    })
  }
}

/// One day of new accounts.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct SignupDailyItem {
  pub date:    chrono::NaiveDate,
  pub signups: i64,
}

#[derive(Clone, Debug, Serialize, FromQueryResult)]
struct AccountTotals {
  users:         i64,
  organizations: i64,
  roles:         i64,
  api_keys:      i64,
  bans:          i64,
}

/// How many accounts exist and how fast they arrive.
///
/// The totals answer "how big is this", the series answers "is that changing". Neither loses its
/// meaning as the installation grows, which the newest few accounts do: past a few thousand users
/// the last five to sign up say nothing about anything.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountActivity {
  pub from:          chrono::NaiveDate,
  pub to:            chrono::NaiveDate,
  pub daily:         Vec<SignupDailyItem>,
  pub users:         i64,
  pub organizations: i64,
  pub roles:         i64,
  pub api_keys:      i64,
  pub bans:          i64,
  pub created_at:    DateTime<FixedOffset>,
}

impl IntoResponse for AccountActivity {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

impl AccountActivity {
  pub async fn new<C: ConnectionTrait>(db: &C) -> Result<Self, DbErr> {
    let backend = db.get_database_backend();

    let daily = SignupDailyItem::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT date_trunc('day', created_at)::date AS "date", count(*) AS "signups"
      FROM users
      WHERE created_at >= CURRENT_DATE - ($1::int - 1)
      GROUP BY "date"
      ORDER BY "date"
      "#,
      [AUTH_WINDOW_DAYS.into()],
    ))
    .all(db)
    .await?;

    // One statement rather than five list probes: the counts are read together and shown together,
    // and five round trips to learn five numbers is four too many.
    let totals = AccountTotals::find_by_statement(Statement::from_sql_and_values(
      backend,
      r#"
      SELECT
        (SELECT count(*) FROM users) AS "users"
        , (SELECT count(*) FROM organizations) AS "organizations"
        , (SELECT count(*) FROM roles) AS "roles"
        , (SELECT count(*) FROM api_keys) AS "api_keys"
        , (SELECT count(*) FROM bans) AS "bans"
      "#,
      [],
    ))
    .one(db)
    .await?
    .unwrap_or(AccountTotals {
      users:         0,
      organizations: 0,
      roles:         0,
      api_keys:      0,
      bans:          0,
    });

    let today = Utc::now().date_naive();

    Ok(Self {
      from: today - chrono::Duration::days(AUTH_WINDOW_DAYS - 1),
      to: today,
      daily,
      users: totals.users,
      organizations: totals.organizations,
      roles: totals.roles,
      api_keys: totals.api_keys,
      bans: totals.bans,
      created_at: Utc::now().fixed_offset(),
    })
  }
}

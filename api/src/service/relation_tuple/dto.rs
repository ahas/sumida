use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::FromQueryResult;
use sea_orm::prelude::DateTimeWithTimeZone;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The list filter, one field per column of the `/relation-tuples` screen (design §9.6).
///
/// Every field is an **exact** match, not the `ILIKE '%…%'` `AccessPolicyFilter` uses. A tuple's
/// columns are identifiers — a resource type, a relation, a canonical subject key — and a substring
/// search over identifiers is how `user:1` finds `user:12` and a support engineer revokes the wrong
/// grant. The one thing this screen exists to make precise is *which* row.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleFilter {
  pub resource_type: Option<String>,
  pub resource_id:   Option<String>,
  pub relation:      Option<String>,
  /// A canonical subject key — `user:7`, `role:3#member`, `organization:100#member`, `user:*`.
  pub subject:       Option<String>,
}

/// One tuple as the screen renders it.
///
/// The shape is §9.6's new column set, and what is *missing* is the point: there is no `effect`.
/// Deny is a rule construct now (`board_post#view` subtracts `blocked`), not a row, so a column for
/// it would be a field nobody can set and every reader would ask about.
#[derive(Clone, Debug, Serialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleListItem {
  pub id:            i64,
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  /// `relation_tuples.subject_key`, the generated column — one field rather than the stored triple,
  /// because that is the form every other surface in this subsystem speaks (`SubjectSet::keys`,
  /// `created_by`, the design's own `user:7` / `role:3#member` notation).
  pub subject:       String,
  pub expires_at:    Option<DateTimeWithTimeZone>,
  pub created_at:    DateTimeWithTimeZone,
  /// Who granted it. `access_policies` recorded no grantor at all, which is half the reason
  /// `assert_policy_within_authority` could not check that a granter holds what they grant.
  pub created_by:    String,
}

impl IntoResponse for RelationTupleListItem {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRelationTuple {
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  pub subject:       String,
  /// The one caveat this model has (§13.4). Absent is a grant that does not expire.
  #[serde(default)]
  pub expires_at:    Option<DateTimeWithTimeZone>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationTupleCreated {
  pub id: i64,
}

impl IntoResponse for RelationTupleCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct RelationTupleDeleted;

impl IntoResponse for RelationTupleDeleted {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

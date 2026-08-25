use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use entity::membership::subscriptions::Column;
use entity::membership::{OwnerKind, SubscriptionStatus};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoSimpleExpr};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

fn default_owner_kind() -> OwnerKind {
  OwnerKind::Personal
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionFilter {
  #[cfg_attr(feature = "openapi", param(inline))]
  pub owner_kind: Option<OneOrMany<OwnerKind>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub owner_id:   Option<OneOrMany<i32>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub status:     Option<OneOrMany<SubscriptionStatus>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub plan_code:  Option<OneOrMany<String>>,
}

/// What `/subscriptions` accepts, declared once.
///
/// The owner scope is a condition emitted into the list statement — everything for a platform
/// administrator, otherwise the caller's own rows and the organizations they may read — so
/// `ownerKind` and `ownerId` narrow inside it rather than choosing it.
///
/// `ownerId` carries no relation hint: what it points at depends on `ownerKind`, and a hint names
/// exactly one resource.
const SUBSCRIPTION_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("ownerKind", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["personal", "organization"])
    .with_facet(),
  FieldSpec::new("ownerId", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_facet(),
  FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["pending", "active", "past-due", "canceled", "expired"])
    .with_facet(),
  FieldSpec::new("planCode", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_facet(),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("currentPeriodEnd", FieldKind::DateTime),
  FieldSpec::sort_only("id", FieldKind::Integer),
];

impl FilterSchema for SubscriptionFilter {
  fn fields() -> &'static [FieldSpec] {
    SUBSCRIPTION_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "ownerKind" => Column::OwnerKind.into_simple_expr(),
      "ownerId" => Column::OwnerId.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "planCode" => Column::PlanCode.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
      "currentPeriodEnd" => Column::CurrentPeriodEnd.into_simple_expr(),
      "id" => Column::Id.into_simple_expr(),
      _ => return None,
    })
  }

  fn default_sort() -> &'static str {
    "id"
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListItem {
  pub id:                   i32,
  pub code:                 String,
  pub plan_id:              i32,
  pub plan_code:            String,
  pub owner_kind:           OwnerKind,
  pub owner_id:             i32,
  pub status:               SubscriptionStatus,
  pub current_period_start: DateTimeWithTimeZone,
  pub current_period_end:   DateTimeWithTimeZone,
  pub cancel_at_period_end: bool,
  pub auto_renew:           bool,
  pub created_at:           DateTimeWithTimeZone,
  pub updated_at:           DateTimeWithTimeZone,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, FromQueryResult)]
#[serde(rename_all = "camelCase")]
pub struct GetSubscription {
  pub id:                   i32,
  pub code:                 String,
  pub plan_id:              i32,
  pub plan_code:            String,
  pub owner_kind:           OwnerKind,
  pub owner_id:             i32,
  pub status:               SubscriptionStatus,
  pub current_period_start: DateTimeWithTimeZone,
  pub current_period_end:   DateTimeWithTimeZone,
  pub cancel_at_period_end: bool,
  pub canceled_at:          Option<DateTimeWithTimeZone>,
  pub auto_renew:           bool,
  #[cfg_attr(feature = "openapi", schema(value_type = Object))]
  pub allowed_actions:      JsonValue,
  pub created_at:           DateTimeWithTimeZone,
  pub updated_at:           DateTimeWithTimeZone,
}

impl IntoResponse for GetSubscription {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscription {
  pub plan_code:       String,
  #[serde(default = "default_owner_kind")]
  pub owner_kind:      OwnerKind,
  #[serde(default)]
  pub owner_id:        Option<i32>,
  #[serde(default)]
  pub idempotency_key: Option<String>,
  /// Opt the new subscription into auto-renewal (requires `billing_key`).
  #[serde(default)]
  pub auto_renew:      bool,
  /// PG billing key (정기결제) stored for auto-renewal.
  #[serde(default)]
  pub billing_key:     Option<String>,
  /// Optional discount coupon(s) applied to the first period's recorded amount (parity with the paid
  /// checkout path). Only meaningful when the first period is charged (i.e. not a free trial).
  #[serde(default)]
  pub coupon_code:     Option<String>,
  #[serde(default)]
  pub coupon_codes:    Option<Vec<String>>,
}

/// Toggle auto-renew on an existing subscription. Enabling requires a billing key
/// (either supplied here or already stored).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutoRenew {
  pub enabled:     bool,
  #[serde(default)]
  pub billing_key: Option<String>,
}

/// Switch an active subscription to a different plan mid-period (upgrade/downgrade).
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
  pub plan_code: String,
}

/// Proration breakdown for a mid-period plan change. Amounts are in minor currency
/// units, prorated by the fraction of the current period still remaining. Actual
/// money movement (charging `proration_amount` or crediting a negative one) is the
/// PG layer's responsibility; this records the canonical breakdown.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProrationResult {
  pub subscription_id:  i32,
  pub old_plan_code:    String,
  pub new_plan_code:    String,
  /// Unused remainder of the old plan's price for this period (a credit).
  pub old_unused:       i64,
  /// New plan's price prorated for the remaining period (a charge).
  pub new_prorated:     i64,
  /// Net to settle now = new_prorated - old_unused (negative = credit owed).
  pub proration_amount: i64,
  /// Tracking code of the recorded proration purchase.
  pub purchase_code:    String,
}

impl IntoResponse for ProrationResult {
  fn into_response(self) -> axum::response::Response {
    Json(self).into_response()
  }
}

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelQuery {
  #[serde(default)]
  pub at_period_end: bool,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCreated {
  pub id:   i32,
  pub code: String,
}

impl IntoResponse for SubscriptionCreated {
  fn into_response(self) -> axum::response::Response {
    (StatusCode::CREATED, Json(self)).into_response()
  }
}

#[derive(Clone, Debug, Serialize)]
pub struct SubscriptionUpdated;

impl IntoResponse for SubscriptionUpdated {
  fn into_response(self) -> axum::response::Response {
    Json(json!({})).into_response()
  }
}

#[cfg(test)]
mod filter_tests {
  use entity::membership::subscriptions::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::authz::Authz;
  use crate::authz::subject::SubjectSet;
  use crate::util::filter::{ConditionTree, listed};

  #[test]
  fn the_declaration_is_sound() {
    SubscriptionFilter::validate().expect("the subscription filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "ownerKind=personal&ownerId=1&status=active&planCode=pro";
    let filter: SubscriptionFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.owner_kind.is_some());
    assert!(filter.owner_id.is_some());
    assert!(filter.status.is_some());
    assert!(filter.plan_code.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: SubscriptionFilter = serde_urlencoded::from_str(
      "ownerKind=personal,organization&ownerId=1,2&status=active,expired&planCode=pro,lite",
    )
    .expect("a list parses");

    assert_eq!(filter.owner_kind.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.owner_id.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.status.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.plan_code.as_ref().map(OneOrMany::len), Some(2));

    let filter: SubscriptionFilter =
      serde_urlencoded::from_str("ownerKind=&ownerId=&status=&planCode=").expect("an empty value parses");

    assert!(listed(filter.owner_kind.as_ref()).is_none());
    assert!(listed(filter.owner_id.as_ref()).is_none());
    assert!(listed(filter.status.as_ref()).is_none());
    assert!(listed(filter.plan_code.as_ref()).is_none());
  }

  #[test]
  fn an_unknown_enum_variant_is_refused() {
    assert!(serde_urlencoded::from_str::<SubscriptionFilter>("status=lapsed").is_err());
  }

  /// Every variant the flat parameters parse is a variant the declaration names — `past-due`
  /// included, which is where a hand-copied spelling would part company with serde.
  #[test]
  fn the_declared_variants_are_the_ones_the_parameters_parse() {
    for key in ["ownerKind", "status"] {
      let field = SubscriptionFilter::field(key).unwrap_or_else(|| panic!("`{key}` is declared"));

      for variant in field.variants {
        serde_urlencoded::from_str::<SubscriptionFilter>(&format!("{key}={variant}"))
          .unwrap_or_else(|_| panic!("`{key}={variant}` must parse"));
      }
    }
  }

  /// The owner scope is a condition emitted into the list statement, not a refusal answered before
  /// it — the path that does refuse is reached from the single-row read and the mutations, never
  /// from the list — so no parameter here picks a scope.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["owner_kind", "owner_id", "plan_code", "auto_renew", "canceled_at"] {
      assert!(SubscriptionFilter::field(spelling).is_none(), "{spelling}");
      assert!(SubscriptionFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(SubscriptionFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    for key in ["createdAt", "currentPeriodEnd", "id"] {
      assert!(SubscriptionFilter::sort_column(key).is_some(), "{key}");
    }
    for key in ["ownerKind", "ownerId", "status", "planCode"] {
      assert!(SubscriptionFilter::sort_column(key).is_none(), "{key}");
    }
  }

  /// One caller for the rule compiler. Which tuples it would match is never read — what these
  /// assert is the shape of the statement, not the rows it returns.
  fn caller() -> SubjectSet {
    SubjectSet::from_parts(vec!["user:7".to_owned(), "role:3#member".to_owned()], vec![7])
  }

  /// The owner scope this list is built from: the caller's own rows, their organization's, and the
  /// organizations `organization#view_subscription` reaches — the last left in the statement as a
  /// subquery, the way the service leaves it. The scope is itself a disjunction, which is what makes
  /// the parenthesising asserted below load-bearing: a tree's `OR` flattened into it would hand the
  /// caller every other owner's rows.
  fn scope() -> Select<Entity> {
    use sea_orm::{QueryFilter, QuerySelect, QueryTrait};

    let orgs = Authz::filter(
      "organization",
      "view_subscription",
      &caller(),
      entity::organizations::Entity::find()
        .select_only()
        .column(entity::organizations::Column::Id),
    )
    .expect("the organization scope compiles");

    Entity::find().filter(
      Condition::any()
        .add(
          Condition::all()
            .add(Column::OwnerKind.eq(OwnerKind::Personal))
            .add(Column::OwnerId.eq(7)),
        )
        .add(
          Condition::all()
            .add(Column::OwnerKind.eq(OwnerKind::Organization))
            .add(Column::OwnerId.eq(100)),
        )
        .add(
          Condition::all()
            .add(Column::OwnerKind.eq(OwnerKind::Organization))
            .add(Column::OwnerId.in_subquery(orgs.into_query())),
        ),
    )
  }

  fn where_of(query: Select<Entity>) -> String {
    use sea_orm::QueryTrait;
    use sea_orm::sea_query::PostgresQueryBuilder;

    let sql = query.as_query().to_string(PostgresQueryBuilder);

    sql.split_once(" WHERE ").expect("a WHERE clause").1.to_owned()
  }

  /// The tree as the one condition a list adds to a query it has already scoped.
  fn compiled(raw: &str) -> Condition {
    ConditionTree::parse(raw)
      .expect("the tree parses")
      .compile::<SubscriptionFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"status","op":"eq","value":"active"},
      {"type":"field","key":"planCode","op":"eq","value":"pro"}
    ]}"#;

    let bare = where_of(Entity::find().filter(compiled(TREE)));
    let scoped = where_of(scope().filter(compiled(TREE)));

    assert!(bare.contains(" OR "), "{bare}");
    assert!(scoped.contains(&where_of(scope())), "the scope must survive whole: {scoped}");
    assert!(
      scoped.ends_with(&format!(" AND ({bare})")),
      "the tree must be one more conjunct: {scoped}"
    );
  }

  /// The column stores text, so the bound value needs no cast. A key backed by a database enum
  /// names its type in the declaration instead, and the cast lands in the predicate.
  #[test]
  fn a_text_backed_enum_binds_without_a_cast() {
    use sea_orm::QueryFilter;

    let predicate = where_of(
      Entity::find().filter(compiled(r#"{"type":"field","key":"status","op":"eq","value":"active"}"#)),
    );

    assert!(SubscriptionFilter::enum_type("status").is_none());
    assert!(predicate.contains("'active'"), "{predicate}");
    assert!(!predicate.contains("CAST("), "{predicate}");
  }
}

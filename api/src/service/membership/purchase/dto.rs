use entity::membership::membership_purchases::Column;
use entity::membership::{OwnerKind, PlanKind, PurchaseStatus};
use sea_orm::prelude::*;
use sea_orm::sea_query::SimpleExpr;
use sea_orm::{FromQueryResult, IntoSimpleExpr};
use sea_orm_crud::OneOrMany;
use serde::{Deserialize, Serialize};

use crate::util::filter::{FieldKind, FieldSpec, FilterSchema, MatchStrategy, Op};

#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi", into_params(parameter_in = Query))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseFilter {
  #[cfg_attr(feature = "openapi", param(inline))]
  pub owner_kind: Option<OneOrMany<OwnerKind>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub owner_id:   Option<OneOrMany<i32>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub kind:       Option<OneOrMany<PlanKind>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub status:     Option<OneOrMany<PurchaseStatus>>,
  #[cfg_attr(feature = "openapi", param(inline))]
  pub plan_code:  Option<OneOrMany<String>>,
}

/// What `/membership-purchases` accepts, declared once.
///
/// The owner scope is a condition emitted into the list statement — everything for an
/// administrator, otherwise the caller's own rows and their organization's — so `ownerKind` and
/// `ownerId` narrow inside it rather than choosing it.
///
/// `ownerId` carries no relation hint: what it points at depends on `ownerKind`, and a hint names
/// exactly one resource. A picker would have to branch on another key, which the hint cannot say.
const PURCHASE_FIELDS: &[FieldSpec] = &[
  FieldSpec::new("ownerKind", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["personal", "organization"])
    .with_facet(),
  FieldSpec::new("ownerId", FieldKind::Integer, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_facet(),
  FieldSpec::new("kind", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["subscription", "credit"])
    .with_facet(),
  FieldSpec::new("status", FieldKind::Enum, MatchStrategy::Comparison, &[Op::Eq, Op::In])
    .with_variants(&["pending", "completed", "failed", "refunded"])
    .with_facet(),
  FieldSpec::new("planCode", FieldKind::Text, MatchStrategy::Comparison, &[Op::Eq, Op::In]).with_facet(),
  FieldSpec::sort_only("createdAt", FieldKind::DateTime),
  FieldSpec::sort_only("id", FieldKind::Integer),
];

impl FilterSchema for PurchaseFilter {
  fn fields() -> &'static [FieldSpec] {
    PURCHASE_FIELDS
  }

  fn column(key: &str) -> Option<SimpleExpr> {
    Some(match key {
      "ownerKind" => Column::OwnerKind.into_simple_expr(),
      "ownerId" => Column::OwnerId.into_simple_expr(),
      "kind" => Column::Kind.into_simple_expr(),
      "status" => Column::Status.into_simple_expr(),
      "planCode" => Column::PlanCode.into_simple_expr(),
      "createdAt" => Column::CreatedAt.into_simple_expr(),
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
pub struct PurchaseListItem {
  pub id:              i32,
  pub code:            String,
  pub kind:            PlanKind,
  pub plan_id:         i32,
  pub plan_code:       String,
  pub owner_kind:      OwnerKind,
  pub owner_id:        i32,
  pub status:          PurchaseStatus,
  pub amount:          i64,
  pub subtotal:        i64,
  pub discount_amount: i64,
  pub tax_amount:      i64,
  pub coupon_code:     Option<String>,
  pub currency:        String,
  pub paid_at:         Option<DateTimeWithTimeZone>,
  pub created_at:      DateTimeWithTimeZone,
}

#[cfg(test)]
mod filter_tests {
  use entity::membership::membership_purchases::Entity;
  use sea_orm::{Condition, EntityTrait, Select};

  use super::*;
  use crate::util::filter::{ConditionTree, listed};

  #[test]
  fn the_declaration_is_sound() {
    PurchaseFilter::validate().expect("the purchase filter declaration must be servable");
  }

  #[test]
  fn every_declared_filterable_key_is_a_query_parameter_the_list_reads() {
    let query = "ownerKind=personal&ownerId=1&kind=credit&status=completed&planCode=pro";
    let filter: PurchaseFilter = serde_urlencoded::from_str(query).expect("the flat filter parses");

    assert!(filter.owner_kind.is_some());
    assert!(filter.owner_id.is_some());
    assert!(filter.kind.is_some());
    assert!(filter.status.is_some());
    assert!(filter.plan_code.is_some());
  }

  #[test]
  fn a_multi_value_key_takes_a_list_and_an_empty_value_means_unfiltered() {
    let filter: PurchaseFilter = serde_urlencoded::from_str(
      "ownerKind=personal,organization&ownerId=1,2&kind=subscription,credit       &status=pending,completed&planCode=pro,lite",
    )
    .expect("a list parses");

    assert_eq!(filter.owner_kind.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.owner_id.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.kind.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.status.as_ref().map(OneOrMany::len), Some(2));
    assert_eq!(filter.plan_code.as_ref().map(OneOrMany::len), Some(2));

    let filter: PurchaseFilter = serde_urlencoded::from_str("ownerKind=&ownerId=&kind=&status=&planCode=")
      .expect("an empty value parses");

    assert!(listed(filter.owner_kind.as_ref()).is_none());
    assert!(listed(filter.owner_id.as_ref()).is_none());
    assert!(listed(filter.kind.as_ref()).is_none());
    assert!(listed(filter.status.as_ref()).is_none());
    assert!(listed(filter.plan_code.as_ref()).is_none());
  }

  #[test]
  fn an_unknown_enum_variant_is_refused() {
    assert!(serde_urlencoded::from_str::<PurchaseFilter>("status=disputed").is_err());
    assert!(serde_urlencoded::from_str::<PurchaseFilter>("ownerKind=personal,corporate").is_err());
  }

  /// Every variant the flat parameters parse is a variant the declaration names.
  #[test]
  fn the_declared_variants_are_the_ones_the_parameters_parse() {
    for key in ["ownerKind", "kind", "status"] {
      let field = PurchaseFilter::field(key).unwrap_or_else(|| panic!("`{key}` is declared"));

      for variant in field.variants {
        serde_urlencoded::from_str::<PurchaseFilter>(&format!("{key}={variant}"))
          .unwrap_or_else(|_| panic!("`{key}={variant}` must parse"));
      }
    }
  }

  /// The owner scope is a condition emitted into the list statement, not a refusal answered before
  /// it, so no parameter here picks a scope. Every key the resource never declared stays
  /// unreachable.
  #[test]
  fn an_undeclared_key_resolves_to_nothing() {
    for spelling in ["owner_kind", "owner_id", "plan_code", "amount", "coupon_code", "paid_at"] {
      assert!(PurchaseFilter::field(spelling).is_none(), "{spelling}");
      assert!(PurchaseFilter::resolve(spelling).is_none(), "{spelling}");
      assert!(PurchaseFilter::sort_column(spelling).is_none(), "{spelling}");
    }
  }

  #[test]
  fn only_declared_sortable_keys_resolve() {
    assert!(PurchaseFilter::sort_column("createdAt").is_some());
    assert!(PurchaseFilter::sort_column("id").is_some());
    for key in ["ownerKind", "ownerId", "kind", "status", "planCode"] {
      assert!(PurchaseFilter::sort_column(key).is_none(), "{key}");
    }
  }

  /// The owner scope this list is built from: the caller's own rows, or their organization's. It is
  /// itself a disjunction, which is what makes the parenthesising asserted below load-bearing — a
  /// tree's `OR` flattened into it would hand the caller every other owner's rows.
  fn scope() -> Select<Entity> {
    use sea_orm::QueryFilter;

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
      .compile::<PurchaseFilter>()
      .expect("the tree compiles")
  }

  /// A root `any` under the read scope. The builder renders the disjunction inside its own
  /// parentheses and the query adds it with `AND`, so the widest thing a tree can do is add one
  /// conjunct: the scope stays whole beside it instead of becoming one of the alternatives.
  #[test]
  fn an_or_inside_a_tree_stays_a_conjunct_of_the_scope() {
    use sea_orm::QueryFilter;

    const TREE: &str = r#"{"type":"any","of":[
      {"type":"field","key":"status","op":"eq","value":"pending"},
      {"type":"field","key":"planCode","op":"in","value":["pro"]}
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
      Entity::find().filter(compiled(r#"{"type":"field","key":"status","op":"eq","value":"pending"}"#)),
    );

    assert!(PurchaseFilter::enum_type("status").is_none());
    assert!(predicate.contains("'pending'"), "{predicate}");
    assert!(!predicate.contains("CAST("), "{predicate}");
  }
}

use sea_orm::sea_query::{Expr, SimpleExpr};

/// `ap.object`가 `start_org_id`의 조직 + 모든 조상 조직 id text에 매칭되는지 검사할 때
/// `ApGuard::object_in_subquery`로 넘기는 raw SimpleExpr.
///
/// 평가 결과: `ANY(SELECT id::text FROM RECURSIVE chain)` — PostgreSQL의 `value = ANY(subquery)`
/// 구문과 동일.
///
/// 사용 예 (`UserService::ap()`):
///
/// ```ignore
/// use crate::util::org::organization_ancestors_subquery;
/// use sea_orm::sea_query::Expr;
///
/// let start = Expr::col((entity::users::Entity, entity::users::Column::OrganizationId))
///   .into_simple_expr();
/// ap::ns("org:user").who(&self.session)
///   .join(Users, Column::Id)
///   .object_in_subquery(organization_ancestors_subquery(start))
/// ```
///
/// 주의: `LATERAL` 안에서 매 행마다 평가되므로 list 쿼리에서는 N×depth만큼 비용이 누적될
/// 수 있다. `organizations.parent_id` 인덱스가 있는 한 depth가 깊지 않다면 비용은 작다.
pub fn organization_ancestors_subquery(start_org_id_expr: SimpleExpr) -> SimpleExpr {
  Expr::cust_with_expr(
    "ANY(WITH RECURSIVE _ap_chain AS ( \
        SELECT id, parent_id FROM organizations WHERE id = $1 \
        UNION ALL \
        SELECT o.id, o.parent_id FROM organizations o JOIN _ap_chain c ON o.id = c.parent_id \
      ) SELECT id::text FROM _ap_chain)",
    start_org_id_expr,
  )
}

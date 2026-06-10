use sea_orm::sea_query::{Expr, SimpleExpr};

/// Raw SimpleExpr passed to `ApGuard::object_in_subquery` to check whether `ap.object`
/// matches the id text of `start_org_id`'s organization plus all of its ancestor organizations.
///
/// Evaluates to: `ANY(SELECT id::text FROM RECURSIVE chain)` — identical to PostgreSQL's
/// `value = ANY(subquery)` syntax.
///
/// Example (`UserService::ap()`):
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
/// Note: it is evaluated per row inside the `LATERAL`, so on list queries the cost can accumulate
/// by N×depth. As long as the `organizations.parent_id` index exists and depth is shallow, the cost is small.
pub fn organization_ancestors_subquery(start_org_id_expr: SimpleExpr) -> SimpleExpr {
  // Produce the FULL comparison `"ap"."object" = ANY(...)` as one raw expression.
  //
  // We cannot return just `ANY(...)` and let the caller wrap it with `col.eq(..)`, because
  // sea-query renders `col = (ANY(...))` (parenthesized right-hand side), which is invalid
  // PostgreSQL syntax and makes the whole authorization query error out (silently denying access).
  // `ap.object` is the fixed object column built in `ApGuard::with_lateral_join`.
  Expr::cust_with_expr(
    r#""ap"."object" = ANY(WITH RECURSIVE _ap_chain AS (
        SELECT id, parent_id FROM organizations WHERE id = $1
        UNION ALL
        SELECT o.id, o.parent_id FROM organizations o JOIN _ap_chain c ON o.id = c.parent_id
      ) SELECT id::text FROM _ap_chain)"#,
    start_org_id_expr,
  )
}

use sea_orm::{
  Condition, EntityTrait, IntoSimpleExpr, JoinType, QuerySelect, RelationDef, Select,
  sea_query::{Expr, SimpleExpr},
};

use entity::access_policies;

/// `Srn` builder
///
/// * `pattern` – string with `?` placeholders (e.g. `"res_a:?/res_b:?/res_c:?"`).
/// * `columns()` – expressions that replace each `?` in order (cast to `varchar`).
/// * `join()` – additional joins required to reference those columns.
///
/// ```rust
/// let srn = Srn::new("res_a:?/res_b:?/res_c:?")
///     .columns(vec![
///         Expr::col((ResA, res_a::Column::Id)).cast_as("varchar"),
///         Expr::col((ResB, res_b::Column::Id)).cast_as("varchar"),
///         Expr::col((ResC, res_c::Column::Id)).cast_as("varchar"),
///     ])
///     .join(JoinType::LeftJoin, res_c::Relation::ResB.def())
///     .join(JoinType::LeftJoin, res_b::Relation::ResA.def());
///
/// let query = srn
///     .apply(res_c::Entity::find())
///     .filter(srn.condition());
/// ```
pub struct Srn {
  ns: Option<String>,
  parts: Vec<String>,        // split pattern parts
  id_exprs: Vec<SimpleExpr>, // expressions replacing '?'
  joins: Vec<(JoinType, RelationDef)>,
}

impl Srn {
  /// Create from a pattern with `?` placeholders.
  pub fn new(pattern: &str) -> Self {
    Self {
      ns: None,
      parts: pattern.split('/').map(str::to_owned).collect(),
      id_exprs: Vec::new(),
      joins: Vec::new(),
    }
  }

  pub fn namespace<S: AsRef<str>>(mut self, ns: S) -> Self {
    self.ns = Some(ns.as_ref().to_owned());
    self
  }

  /// Register expressions that substitute `?` in order.
  pub fn columns(mut self, exprs: Vec<SimpleExpr>) -> Self {
    self.id_exprs = exprs;
    self
  }

  /// Register an additional join.
  pub fn join(mut self, jt: JoinType, rel: RelationDef) -> Self {
    self.joins.push((jt, rel));
    self
  }

  /// Apply all joins to a query.
  /// Apply all registered joins to the incoming `Select`.
  /// Apply all registered joins to the incoming `Select`.
  /// Apply all registered joins **consuming** the builder.
  ///
  /// Call `self.condition()` **before** `apply` if you still need the condition, e.g.:
  /// ```rust
  /// let cond = srn.condition();
  /// let query = srn.apply(res_c::Entity::find()).filter(cond);
  /// ```
  pub fn apply<E: EntityTrait>(self, mut query: Select<E>) -> Select<E> {
    for (join_type, relation) in self.joins {
      // move out, no Clone needed
      query = query.join(join_type, relation);
    }
    query
  }

  /// Build a `Condition` that matches: exact → prefix wildcards → global wildcard.
  pub fn condition(&self) -> Condition {
    let mut cond = Condition::any();
    let depth = self.parts.len();

    // 1. exact & prefix patterns
    for lvl in (1..=depth).rev() {
      let pat = self.build_expr(lvl);
      cond = cond.add(self.object_col().eq(pat));
    }

    // 2. global wildcard (e.g. `res_a:*/*/*`)
    cond = cond.add(self.object_col().eq(self.build_global_expr()));

    if let Some(ns) = self.ns.clone() {
      cond = cond.add(
        Expr::col((access_policies::Entity, entity::access_policies::Column::Namespace))
          .eq(ns)
          .into_simple_expr(),
      );
    }

    cond
  }

  // ---------- helpers ----------

  fn object_col(&self) -> SimpleExpr {
    Expr::col((access_policies::Entity, access_policies::Column::Object)).into()
  }

  /// Build expression up to `lvl` specific parts; rest replaced by `*`.
  fn build_expr(&self, lvl: usize) -> SimpleExpr {
    let mut expr_opt: Option<SimpleExpr> = None;

    for (idx, part) in self.parts.iter().enumerate() {
      let literal = part.split(':').next().unwrap();
      let id_expr = if idx < lvl {
        self.id_exprs.get(idx).cloned().unwrap_or_else(|| Expr::val("*").into())
      } else {
        Expr::val("*").into()
      };

      let segment = Expr::cust_with_exprs("CONCAT(?, ':', ?)", vec![Expr::val(literal).into(), id_expr]);

      expr_opt = Some(if let Some(prev) = expr_opt {
        Expr::cust_with_exprs("CONCAT(?, '/', ?)", vec![prev, segment])
      } else {
        segment
      });
    }

    expr_opt.expect("Srn::build_expr failed")
  }

  /// Build the most generic wildcard pattern (`res_a:*/*/*`).
  fn build_global_expr(&self) -> SimpleExpr {
    let mut expr_opt: Option<SimpleExpr> = None;

    for part in &self.parts {
      let literal = part.split(':').next().unwrap();
      let segment =
        Expr::cust_with_exprs("CONCAT(?, ':', ?)", vec![Expr::val(literal).into(), Expr::val("*").into()]);
      expr_opt = Some(if let Some(prev) = expr_opt {
        Expr::cust_with_exprs("CONCAT(?, '/', ?)", vec![prev, segment])
      } else {
        segment
      });
    }

    expr_opt.expect("Srn::build_global_expr failed")
  }
}

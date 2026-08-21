pub mod dto;

use entity::prelude::*;
use error::SumidaError;
use infra::session::UserSession;
use sea_orm::{
  ConnectionTrait,
  EntityTrait,
  FromQueryResult,
  PaginatorTrait,
  QueryOrder,
  QuerySelect,
  Select,
  Statement,
};

use self::dto::*;
use crate::SumidaState;
use crate::authz::subject::SubjectSet;
use crate::authz::write::{SubjectKey, TupleFilter, TupleWrite, validate_write};
use crate::authz::{Authz, AuthzError};
use crate::util::filter::ConditionQuery;
use crate::util::pagination::Pagination;

/// The CRUD surface over `relation_tuples` — granting and revoking, as a resource.
///
/// The whole of the authorization is two calls into `Authz`, and that is deliberate: every write
/// path here goes through `Authz::write` / `Authz::revoke`, which check `<resource_type>#grant`
/// **on the object being granted**. Nothing in this file decides who may grant what, because a rule
/// that lives at a call site is a rule the next call site can forget.
///
/// **The authority rule starts from "no", and the rule set names the path to yes.** A granter must
/// hold `grant` on the object they are granting: `organization#grant = manage`, so an organization
/// manager may grant inside their own subtree and nowhere else; `service#grant = operator |
/// platform:root->superuser`, so delegating a route gate needs operator on that namespace. The
/// exemption is `platform:root#superuser`, which most `grant` rules union in through their own
/// chain.
///
/// A check shaped the other way round — one that enumerates what a caller may *not* write, such as
/// refusing wildcards — passes every concrete grant. That lets any account hand any subject a
/// specific power over somebody else's row, and there is nothing in the row to compare the granter
/// against.
pub struct RelationTupleService<S: SumidaState> {
  state:   S,
  session: UserSession,
}

impl<S: SumidaState> RelationTupleService<S> {
  pub fn new(state: S, session: UserSession) -> Self {
    Self { state, session }
  }

  fn subject(&self) -> SubjectSet {
    SubjectSet::of(&self.session)
  }

  /// The columns every read of this table projects. `subject_key` is the generated column, renamed
  /// to the wire's `subject`.
  fn default_query_with(query: Select<RelationTuples>) -> Select<RelationTuples> {
    query
      .select_only()
      .column(RelationTuplesColumn::Id)
      .column(RelationTuplesColumn::ResourceType)
      .column(RelationTuplesColumn::ResourceId)
      .column(RelationTuplesColumn::Relation)
      .column_as(RelationTuplesColumn::SubjectKey, "subject")
      .column(RelationTuplesColumn::ExpiresAt)
      .column(RelationTuplesColumn::CreatedAt)
      .column(RelationTuplesColumn::CreatedBy)
  }

  /// The row scope, as the compiled rule renders it.
  ///
  /// `relation_tuple#view` is `service:access_policy->operate_read`, which reads no column of the
  /// row under test — so the predicate is session-constant and the list is all-or-nothing. It is
  /// still emitted **into the statement** rather than evaluated once and branched on, because that
  /// is what makes the rule the only thing deciding: the day `relation_tuple#view` grows a
  /// row-dependent branch, this list narrows with it and no code here changes.
  fn select(&self, query: Select<RelationTuples>) -> Result<Select<RelationTuples>, SumidaError> {
    Ok(Authz::filter(
      "relation_tuple",
      "view",
      &self.subject(),
      Self::default_query_with(query),
    )?)
  }

  /// The read scope first, the flat parameters inside it, and the condition tree last as one more
  /// conjunct. The sort key resolves through `RelationTupleFilter` 's own declaration, so what the
  /// descriptor offers and what this statement orders by cannot part company.
  pub async fn list(
    &self,
    filter: RelationTupleFilter,
    conditions: ConditionQuery,
    pagination: Pagination,
  ) -> Result<(Vec<RelationTupleListItem>, u64), SumidaError> {
    let db = self.state.db();
    let mut query =
      conditions.apply::<RelationTupleFilter, _>(filter.apply(self.select(RelationTuples::find())?))?;

    let count = query.clone().count(db).await?;

    query = pagination.order_by_schema::<RelationTupleFilter, _>(query);
    // A stable tiebreaker under LIMIT: several tuples can share any declared sort key.
    query = query.order_by(RelationTuplesColumn::Id, pagination.order.clone());
    query = pagination.paginate(query);

    let items = query.into_model::<RelationTupleListItem>().all(db).await?;

    Ok((items, count))
  }

  pub async fn get(&self, id: i64) -> Result<Option<RelationTupleListItem>, SumidaError> {
    Ok(
      self
        .select(RelationTuples::find_by_id(id))?
        .into_model::<RelationTupleListItem>()
        .one(self.state.db())
        .await?,
    )
  }

  /// Writes one tuple, authority-checked against `<resource_type>#grant`.
  ///
  /// Idempotent by `uq_relation_tuples__tuple`: re-posting the same five identity columns refreshes
  /// the grant's expiry and its `created_by` instead of adding a second row. That is why there is no
  /// `PATCH` — a tuple's identity *is* its five columns, so changing any of them is a revoke and a
  /// grant, and the only field that can be edited in place is the expiry, which this path already
  /// edits. A `PATCH` would have to merge the request onto the stored row and re-derive the final
  /// state before it could re-check authority; a partial update over an authorization record is a
  /// stepwise-circumvention problem nobody needs to have.
  pub async fn create(&self, data: CreateRelationTuple) -> Result<i64, SumidaError> {
    let db = self.state.db();
    let subject = self.subject();

    let key = SubjectKey::parse(&data.subject).map_err(malformed)?;
    let mut write = TupleWrite::new(&data.resource_type, &data.resource_id, &data.relation, key.clone());
    if let Some(at) = data.expires_at {
      write = write.expiring(at);
    }

    // Run the vocabulary check first and answer its three refusals as `400`. `Authz::write` runs it
    // again on the way through — it is pure — but by then the error has become a `500`, and a
    // malformed grant is a client's mistake, not the server's.
    validate_write(&write).map_err(malformed)?;
    // Same reason, one layer down: `organization` is keyed by `organizations.id integer`, so
    // `resourceId: "abc"` is a malformed request and not a `500` from a failed comparison.
    Authz::object_key(&data.resource_type, &data.resource_id).map_err(malformed)?;
    assert_grantable(&data.resource_type)?;

    Authz::write(write, &subject, db).await?;

    // Read the id back rather than returning it from the insert: the write is an upsert, so on a
    // re-grant the row already existed and there is no new id to report.
    Id::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      "SELECT id FROM relation_tuples
        WHERE resource_type = $1 AND resource_id = $2 AND relation = $3 AND subject_key = $4",
      [
        data.resource_type.into(),
        data.resource_id.into(),
        data.relation.into(),
        key.key().into(),
      ],
    ))
    .one(db)
    .await?
    .map(|row| row.id)
    .ok_or_else(|| SumidaError::Unknown(500, "the tuple was written and cannot be read back".into()))
  }

  /// Revokes one tuple, authority-checked against `<resource_type>#grant`.
  ///
  /// The lookup is **not** scoped by `relation_tuple#view`. Seeing the grant register and being
  /// able to revoke a grant are different capabilities and the rules say so separately: an
  /// organization manager holds `organization#grant` over their own subtree without holding
  /// `service:access_policy#op_read` over the whole table. Scoping the lookup would make them a
  /// `404` on a row they are entitled to revoke — and, worse, would make the refusal indistinguish-
  /// able from "no such tuple".
  pub async fn delete(&self, id: i64) -> Result<(), SumidaError> {
    let db = self.state.db();

    let row = Identity::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      "SELECT resource_type, resource_id, relation, subject_key AS subject
         FROM relation_tuples WHERE id = $1",
      [id.into()],
    ))
    .one(db)
    .await?
    .ok_or_else(|| SumidaError::NoData("RelationTuple".to_owned(), id.to_string()))?;

    assert_grantable(&row.resource_type)?;

    let filter = TupleFilter::object(&row.resource_type, &row.resource_id)
      .relations(&[row.relation.as_str()])
      .subject(SubjectKey::parse(&row.subject).map_err(malformed)?);

    Authz::revoke(filter, &self.subject(), db).await?;
    Ok(())
  }
}

/// Refuses a resource type on which the rules declare no way to grant anything.
///
/// Without it the request reaches `try_check(<type>, …, "grant", …)`, which answers
/// `UnknownPermission` and leaves as a `500`. A definition with no `grant` permission is not a
/// server fault — it is the rules saying that nothing on this type is grantable by anyone, which is
/// a `403`, the same answer a caller who simply lacks the authority gets. Deliberately the same:
/// "you may not" and "nobody may" are not a distinction worth handing out.
fn assert_grantable(resource_type: &str) -> Result<(), SumidaError> {
  let definition = Authz::registry()?
    .get(resource_type)
    .ok_or_else(|| SumidaError::BadRequest(format!("no definition `{resource_type}`")))?;

  if definition.permission("grant").is_none() {
    return Err(SumidaError::NoAccessPermission);
  }

  Ok(())
}

/// A rule-side refusal the **request** caused: an undeclared relation, a derived relation, a
/// subject type the relation does not accept, a subject key that is not `<type>:<id>[#<relation>]`.
/// All four are `400`; everything else keeps `AuthzError`'s own `500`, because it means the rule set
/// itself did not load.
fn malformed(error: AuthzError) -> SumidaError {
  match &error {
    AuthzError::UnknownDefinition(_)
    | AuthzError::UnknownRelation { .. }
    | AuthzError::Schema(_)
    | AuthzError::NotCompilable(_) => SumidaError::BadRequest(error.to_string()),
    _ => error.into(),
  }
}

#[derive(FromQueryResult)]
struct Id {
  id: i64,
}

#[derive(FromQueryResult)]
struct Identity {
  resource_type: String,
  resource_id:   String,
  relation:      String,
  subject:       String,
}

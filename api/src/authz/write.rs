//! Tuple writes and reverse expansion.
//!
//! Everything else in this module answers "may this subject do that". This file is the other half:
//! the grants themselves — writing one, revoking one, and reading back **who holds what on one
//! object**. That last direction is the one the sharing screens read, and it is the one shape that
//! cannot be a predicate over a caller's rows: it enumerates subjects, so it is its own statement.
//!
//! Three properties are load-bearing:
//!
//! * **A write is checked against the rule set before it reaches the table.** A relation that is
//!   derived from a domain column cannot be granted as a row, and a subject type the relation never
//!   declared is refused at the API instead of being stored and silently never matching. That is
//!   the defect a reversed `admin:user` row is: a grant nobody could ever have used, sitting
//!   in the table looking like authority.
//! * **A write is authority-checked against `<definition>#grant`.** The granter is the caller's own
//!   subject set and the rule states what granting that object requires, so nobody can hand out
//!   what they do not hold themselves.
//! * **A revoke names a tuple exactly.** `uq_relation_tuples__tuple` makes a duplicate impossible,
//!   so a revocation cannot leave a second copy behind. A non-unique index in that position is how
//!   a revoked grant keeps working.

use error::SumidaError;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::{ConnectionTrait, FromQueryResult, Statement};

use super::ir::{RelationSource, SubjectSpec};
use super::subject::SubjectSet;
use super::{Authz, AuthzError, ServiceGrant};

/// One subject, in Zanzibar's triple form.
///
/// `relation` empty is a terminal subject (`user:7`, `apikey:ab12…`); non-empty makes it a
/// **set** (`role:1#member` — "every member of role 1"), which is what the old `role:1` string
/// meant without ever saying so.
///
/// `Hash` is derived because a caller that rewrites a whole grant grid deduplicates by subject
/// before it writes — a sharing screen can submit the same person twice at two levels, and
/// resolving that in a `HashMap<SubjectKey, _>` is what stops the second write from being an
/// `ON CONFLICT` that silently keeps whichever row lost.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SubjectKey {
  pub ty:       String,
  pub id:       String,
  pub relation: String,
}

impl SubjectKey {
  pub fn user(id: i32) -> Self {
    Self {
      ty:       "user".to_owned(),
      id:       id.to_string(),
      relation: String::new(),
    }
  }

  pub fn apikey(hash: &str) -> Self {
    Self {
      ty:       "apikey".to_owned(),
      id:       hash.to_owned(),
      relation: String::new(),
    }
  }

  /// A userset — `role:1#member`, `organization:100#member`.
  pub fn userset(ty: &str, id: &str, relation: &str) -> Self {
    Self {
      ty:       ty.to_owned(),
      id:       id.to_owned(),
      relation: relation.to_owned(),
    }
  }

  /// Parses the canonical key form `<type>:<id>[#<relation>]`.
  ///
  /// The id may itself contain colons (`delegation_scope:svc:alpha`), so the split is on the
  /// **first** one — the same rule `subject_key`'s generated column concatenates by.
  pub fn parse(key: &str) -> Result<Self, AuthzError> {
    let (head, relation) = match key.split_once('#') {
      Some((head, relation)) => (head, relation),
      None => (key, ""),
    };
    let (ty, id) = head
      .split_once(':')
      .ok_or_else(|| AuthzError::Schema(format!("`{key}` is not a subject key (`<type>:<id>`)")))?;
    if ty.is_empty() || id.is_empty() {
      return Err(AuthzError::Schema(format!("`{key}` is not a subject key (`<type>:<id>`)")));
    }
    Ok(Self {
      ty:       ty.to_owned(),
      id:       id.to_owned(),
      relation: relation.to_owned(),
    })
  }

  /// The canonical key — exactly what `relation_tuples.subject_key` generates.
  pub fn key(&self) -> String {
    if self.relation.is_empty() {
      format!("{}:{}", self.ty, self.id)
    } else {
      format!("{}:{}#{}", self.ty, self.id, self.relation)
    }
  }

  /// The integer user id, for a terminal `user:` subject and nothing else.
  ///
  /// `role:1#member` is a set of users and must **not** answer here: a caller that treated it as
  /// one would report a role as a person, which is precisely how a share list would grow a
  /// phantom member.
  pub fn as_user_id(&self) -> Option<i32> {
    if self.ty == "user" && self.relation.is_empty() { self.id.parse().ok() } else { None }
  }
}

impl std::fmt::Display for SubjectKey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.key())
  }
}

/// One tuple to write.
#[derive(Clone, Debug)]
pub struct TupleWrite {
  pub resource_type: String,
  pub resource_id:   String,
  pub relation:      String,
  pub subject:       SubjectKey,
  /// Expiry is the only condition a tuple can carry — there is no time-of-day, no weekday, no
  /// request-origin clause. `None` is a grant that does not expire.
  pub expires_at:    Option<DateTimeWithTimeZone>,
}

impl TupleWrite {
  pub fn new(resource_type: &str, resource_id: &str, relation: &str, subject: SubjectKey) -> Self {
    Self {
      resource_type: resource_type.to_owned(),
      resource_id: resource_id.to_owned(),
      relation: relation.to_owned(),
      subject,
      expires_at: None,
    }
  }

  pub fn expiring(mut self, at: DateTimeWithTimeZone) -> Self {
    self.expires_at = Some(at);
    self
  }
}

/// Which tuples a revocation names.
///
/// `resource_type` is always required. Everything else narrows, and the widest form
/// (`TupleFilter::object`) is "every grant on this object", which is what a resource delete needs.
#[derive(Clone, Debug)]
pub struct TupleFilter {
  resource_type: String,
  resource_id:   Option<String>,
  relations:     Vec<String>,
  subject:       Option<SubjectKey>,
}

impl TupleFilter {
  /// Every tuple on one object. The delete-cascade shape.
  pub fn object(resource_type: &str, resource_id: &str) -> Self {
    Self {
      resource_type: resource_type.to_owned(),
      resource_id:   Some(resource_id.to_owned()),
      relations:     Vec::new(),
      subject:       None,
    }
  }

  pub fn relations(mut self, relations: &[&str]) -> Self {
    self.relations = relations.iter().map(|r| (*r).to_owned()).collect();
    self
  }

  pub fn subject(mut self, subject: SubjectKey) -> Self {
    self.subject = Some(subject);
    self
  }
}

/// One grant read back off an object — who, and at which relation.
///
/// The design's `subjects_with` returns bare subject keys. The relation comes back with them
/// because every caller of this — a sharing screen's "who holds what" read — has to say
/// *at which level* the subject holds the object, and deriving that afterwards would mean one
/// query per relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectGrant {
  pub subject:  SubjectKey,
  pub relation: String,
}

/// Which existing grants `replace_object_grants` may delete.
///
/// It exists because two products already have this exact operation and implement it with
/// different preservation rules: one screen preserves out-of-subtree, `role:`/`apikey:` and denied
/// rows, another rewrites only the one user being re-levelled. Carrying the policy here lets both
/// call one function instead of each hand-rolling a delete.
#[derive(Clone, Debug)]
pub enum SubjectScope {
  /// Only this subject's grants are replaced; every other subject on the object is left alone.
  SingleSubject(SubjectKey),
  /// Every subject's grants on the object are replaced. The sharing screen that shows the whole
  /// grid and submits it whole.
  All,
}

#[derive(FromQueryResult)]
struct GrantRow {
  relation:         String,
  subject_type:     String,
  subject_id:       String,
  subject_relation: String,
}

impl Authz {
  /// Writes one tuple, authority-checked against `<definition>#grant`.
  ///
  /// Idempotent: `uq_relation_tuples__tuple` makes the same grant one row, and a re-grant refreshes
  /// its expiry rather than adding a second copy.
  pub async fn write<C: ConnectionTrait>(
    write: TupleWrite,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<(), SumidaError> {
    validate_write(&write)?;
    // `resource_id` is `text` in the tuple table and the definition's own key column may not be —
    // `organization` is `organizations.id integer`. Handing the raw string to the point check is
    // `operator does not exist: integer = text`, not a slow query.
    let object = Self::object_key(&write.resource_type, &write.resource_id)?;
    Self::try_check(&write.resource_type, object, "grant", subject, db).await?;

    let granter = subject.keys().first().cloned().unwrap_or_else(|| "system".to_owned());
    insert_tuple(&write, &granter, db).await?;
    Ok(())
  }

  /// Writes one tuple **without** an authority check, stamping `created_by` with `granter`.
  ///
  /// For the seeding paths only, and the distinction is not a convenience: at creation time there
  /// is no grantor to check, because the resource did not exist a moment ago and therefore nobody
  /// can hold `grant` on it yet. A create path's owner seed is exactly that — the authorization
  /// already happened, at the `create` permission that let the row exist.
  ///
  /// Still rule-checked. Skipping the authority is a decision about *who*; writing a relation
  /// nobody declared would be a decision about *what*, and no path gets to make that one.
  pub async fn write_unchecked<C: ConnectionTrait>(
    write: TupleWrite,
    granter: &str,
    db: &C,
  ) -> Result<(), AuthzError> {
    validate_write(&write)?;
    insert_tuple(&write, granter, db).await
  }

  /// Revokes every tuple the filter names, authority-checked against `<definition>#grant`.
  ///
  /// Returns how many rows went. A revocation of something nobody held is `0` and not an error —
  /// the sharing screens submit a level for a user who may or may not already have one.
  pub async fn revoke<C: ConnectionTrait>(
    filter: TupleFilter,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<u64, SumidaError> {
    let Some(id) = filter.resource_id.clone() else {
      return Err(SumidaError::BadRequest("a revocation must name the object it revokes on".to_owned()));
    };
    let object = Self::object_key(&filter.resource_type, &id)?;
    Self::try_check(&filter.resource_type, object, "grant", subject, db).await?;
    Ok(Self::revoke_unchecked(filter, db).await?)
  }

  /// Revokes without an authority check.
  ///
  /// The counterpart of [`Authz::write_unchecked`], and needed for the same reason in reverse: a
  /// resource being deleted takes its grants with it, and by the time the cascade runs the caller
  /// has already been authorized to delete the resource itself. Asking again would ask about an
  /// object that is on its way out.
  pub async fn revoke_unchecked<C: ConnectionTrait>(
    filter: TupleFilter,
    db: &C,
  ) -> Result<u64, AuthzError> {
    let mut sql = String::from("DELETE FROM relation_tuples WHERE resource_type = $1");
    let mut values: Vec<sea_orm::Value> = vec![filter.resource_type.clone().into()];

    if let Some(id) = &filter.resource_id {
      values.push(id.clone().into());
      sql.push_str(&format!(" AND resource_id = ${}", values.len()));
    }
    if !filter.relations.is_empty() {
      values.push(filter.relations.clone().into());
      sql.push_str(&format!(" AND relation = ANY(${})", values.len()));
    }
    if let Some(subject) = &filter.subject {
      values.push(subject.key().into());
      sql.push_str(&format!(" AND subject_key = ${}", values.len()));
    }

    let result = db
      .execute(Statement::from_sql_and_values(db.get_database_backend(), &sql, values))
      .await?;
    Ok(result.rows_affected())
  }

  /// Who holds `relations` on one object — the reverse expansion.
  ///
  /// Expired grants are excluded, because a screen that lists them shows authority nobody has.
  pub async fn subjects_with<C: ConnectionTrait>(
    resource_type: &str,
    resource_id: &str,
    relations: &[&str],
    db: &C,
  ) -> Result<Vec<ObjectGrant>, AuthzError> {
    let relations: Vec<String> = relations.iter().map(|r| (*r).to_owned()).collect();

    let rows = GrantRow::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      // `(resource_type, resource_id, relation)` is `idx_rt__object`, whose INCLUDE carries
      // `subject_key` — this is the direction that index exists for.
      r#"
      SELECT relation, subject_type, subject_id, subject_relation
        FROM relation_tuples
       WHERE resource_type = $1 AND resource_id = $2 AND relation = ANY($3)
         AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
       ORDER BY subject_key, relation
      "#,
      [resource_type.to_owned().into(), resource_id.to_owned().into(), relations.into()],
    ))
    .all(db)
    .await?;

    Ok(
      rows
        .into_iter()
        .map(|row| ObjectGrant {
          subject:  SubjectKey {
            ty:       row.subject_type,
            id:       row.subject_id,
            relation: row.subject_relation,
          },
          relation: row.relation,
        })
        .collect(),
    )
  }

  /// Replaces the grant set on one object, within `scope`, in the caller's transaction.
  ///
  /// Two statements — a scoped delete and the inserts — so it **must** be handed a transaction, or
  /// a failure halfway leaves the subject with no grant at all rather than the one it had. Every
  /// call site in this tree passes one.
  ///
  /// The authority check runs once, against `<definition>#grant`, before anything is touched. A
  /// caller who may not grant does not get to delete either, which is a rule a hand-rolled
  /// version is easy to get wrong.
  pub async fn replace_object_grants<C: ConnectionTrait>(
    resource_type: &str,
    resource_id: &str,
    relations: &[&str],
    entries: &[(SubjectKey, &str)],
    scope: SubjectScope,
    subject: &SubjectSet,
    db: &C,
  ) -> Result<(), SumidaError> {
    let object = Self::object_key(resource_type, resource_id)?;
    Self::try_check(resource_type, object, "grant", subject, db).await?;

    let mut filter = TupleFilter::object(resource_type, resource_id).relations(relations);
    if let SubjectScope::SingleSubject(only) = &scope {
      filter = filter.subject(only.clone());
    }
    Self::revoke_unchecked(filter, db).await?;

    let granter = subject.keys().first().cloned().unwrap_or_else(|| "system".to_owned());
    for (target, relation) in entries {
      let write = TupleWrite::new(resource_type, resource_id, relation, target.clone());
      validate_write(&write)?;
      insert_tuple(&write, &granter, db).await?;
    }

    Ok(())
  }

  /// Replaces every **service-tier** grant one subject holds, in the caller's transaction.
  ///
  /// This is the write half of the role form and the api-key form — the two screens that post
  /// `accessPolicies: [{namespace, object: ["service"], action, effect}]` and read the same shape
  /// back from [`Authz::service_grants`].
  ///
  /// Three properties, and the first two are the reason it is one function rather than a delete and
  /// a loop at each call site:
  ///
  /// * **It touches nothing outside `resource_type = 'service'`.** A per-object grant or a
  ///   delegation scope held by the same role survives a submit of the permission grid, without the
  ///   caller having to enumerate what to preserve. A delete keyed by subject alone would take the
  ///   object-tier rows with it.
  /// * **Authority is `service:<namespace>#grant`**, checked for every namespace named, before
  ///   anything is deleted. A check that only refuses wildcards is not enough: it lets a non-admin
  ///   hand a role any concrete grant at all, including one they do not hold themselves.
  /// * **`action: ["*"]` is expanded, not stored.** `*` is not a relation; on a `service` object it
  ///   stands for the four route-gate actions and nothing more.
  pub async fn replace_service_grants<C: ConnectionTrait>(
    holder: &SubjectKey,
    grants: &[ServiceGrant],
    subject: &SubjectSet,
    db: &C,
  ) -> Result<(), SumidaError> {
    let mut writes = Vec::new();
    for grant in grants {
      for relation in expand_service_actions(&grant.action) {
        let write = TupleWrite::new("service", &grant.namespace, &relation, holder.clone());
        // A relation the `service` definition never declared is the client's mistake, not the
        // server's: answering it as the `500` an `AuthzError` becomes would tell a form that
        // posted `action: ["manage"]` to retry.
        validate_write(&write).map_err(|error| SumidaError::BadRequest(error.to_string()))?;
        writes.push(write);
      }
    }

    // Every namespace, before anything is deleted — a caller who may not grant one of them must
    // not get to clear the others on the way to being refused.
    let mut checked: Vec<&str> = Vec::new();
    for grant in grants {
      if checked.contains(&grant.namespace.as_str()) {
        continue;
      }
      checked.push(&grant.namespace);
      Self::try_check("service", grant.namespace.as_str(), "grant", subject, db).await?;
    }

    db.execute(Statement::from_sql_and_values(
      db.get_database_backend(),
      "DELETE FROM relation_tuples WHERE resource_type = 'service' AND subject_key = $1",
      [holder.key().into()],
    ))
    .await
    .map_err(AuthzError::from)?;

    let granter = subject.keys().first().cloned().unwrap_or_else(|| "system".to_owned());
    for write in &writes {
      insert_tuple(write, &granter, db).await?;
    }

    Ok(())
  }

  /// Every tuple this subject holds, on anything, revoked.
  ///
  /// [`TupleFilter`] cannot express it: `resource_type` is mandatory there, and "every grant this
  /// role holds" names no resource type. It is served by `idx_rt__lookup`'s leading column.
  ///
  /// Deliberately **unchecked**, and the reason is structural rather than convenient: it is called
  /// from inside the delete of the subject itself — a role, an api key — where the authority check
  /// already happened on that row. Requiring `#grant` on every object the subject happens to hold
  /// would make deleting a role impossible for anyone but a superuser, and would leave the deleted
  /// role's grants behind when it was refused.
  pub async fn revoke_all_of<C: ConnectionTrait>(holder: &SubjectKey, db: &C) -> Result<u64, AuthzError> {
    let result = db
      .execute(Statement::from_sql_and_values(
        db.get_database_backend(),
        "DELETE FROM relation_tuples WHERE subject_key = $1",
        [holder.key().into()],
      ))
      .await?;
    Ok(result.rows_affected())
  }

  /// The user ids a set of subject keys resolves to.
  ///
  /// **Every subject type the relations accept is expanded here, api keys included**, in one
  /// statement. A hand-parser that recognises only the prefixes its author had in mind drops the
  /// rest without saying so, and a subject silently dropped from this expansion is a holder who
  /// never receives what the grant exists to deliver.
  ///
  /// A userset (`role:1#member`, `organization:100#member`) expands to its members; a terminal
  /// `user:` subject is itself. Anything else contributes nobody rather than being guessed at.
  pub async fn expand_to_user_ids<C: ConnectionTrait>(
    subjects: &[SubjectKey],
    db: &C,
  ) -> Result<Vec<i32>, AuthzError> {
    let (mut users, mut roles, mut orgs, mut keys) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for subject in subjects {
      match (subject.ty.as_str(), subject.relation.as_str()) {
        ("user", "") => users.extend(subject.id.parse::<i32>()),
        ("role", "member") => roles.extend(subject.id.parse::<i32>()),
        ("organization", "member") => orgs.extend(subject.id.parse::<i32>()),
        ("apikey", "") => keys.push(subject.id.clone()),
        _ => {}
      }
    }

    if users.is_empty() && roles.is_empty() && orgs.is_empty() && keys.is_empty() {
      return Ok(Vec::new());
    }

    #[derive(FromQueryResult)]
    struct UserId {
      id: i32,
    }

    let rows = UserId::find_by_statement(Statement::from_sql_and_values(
      db.get_database_backend(),
      r#"
      SELECT DISTINCT u.id
        FROM users u
       WHERE u.id = ANY($1)
          OR u.role_id = ANY($2)
          OR u.organization_id = ANY($3)
          OR u.id IN (SELECT k.user_id FROM api_keys k WHERE k.key = ANY($4))
       ORDER BY u.id
      "#,
      [users.into(), roles.into(), orgs.into(), keys.into()],
    ))
    .all(db)
    .await?;

    Ok(rows.into_iter().map(|row| row.id).collect())
  }
}

/// The action vocabulary of a `service` object, as relations.
///
/// `*` is the only value that is not already a relation name. On a `service` object it means "every
/// route action on this namespace", which is the four gate relations exactly — the object is
/// `service`, so no object-wide power is in it. Expanding it here rather than storing it keeps `*`
/// out of the table, where it would be a relation the rules never declared.
fn expand_service_actions(actions: &[String]) -> Vec<String> {
  let mut out = Vec::new();
  for action in actions {
    if action == "*" {
      out.extend(["create", "read", "update", "delete"].map(str::to_owned));
    } else {
      out.push(action.clone());
    }
  }
  out.sort();
  out.dedup();
  out
}

async fn insert_tuple<C: ConnectionTrait>(
  write: &TupleWrite,
  granter: &str,
  db: &C,
) -> Result<(), AuthzError> {
  db.execute(Statement::from_sql_and_values(
    db.get_database_backend(),
    r#"
    INSERT INTO relation_tuples
      (resource_type, resource_id, relation, subject_type, subject_id, subject_relation,
       expires_at, created_by)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    ON CONFLICT (resource_type, resource_id, relation, subject_type, subject_id, subject_relation)
    DO UPDATE SET expires_at = EXCLUDED.expires_at, created_by = EXCLUDED.created_by
    "#,
    [
      write.resource_type.clone().into(),
      write.resource_id.clone().into(),
      write.relation.clone().into(),
      write.subject.ty.clone().into(),
      write.subject.id.clone().into(),
      write.subject.relation.clone().into(),
      write.expires_at.into(),
      granter.to_owned().into(),
    ],
  ))
  .await?;
  Ok(())
}

/// The rule-side check every write goes through, authority-checked or not.
///
/// Three refusals, each of which the old model had no vocabulary for:
///
/// 1. **A relation nobody declared.** The foreign key catches this in the database too; catching it
///    here is what turns a constraint violation into a sentence.
/// 2. **A derived relation.** `board_group#parent` is `board_groups.parent_id`. Granting it
///    as a row would store authority the evaluator never reads, because the rule reads the column.
/// 3. **A subject type the relation never declared.** `service:board_post#read@board:7` is stored
///    happily by `access_policies` and matches nothing, forever.
///
/// Crate-visible so `/relation-tuples` can run it **before** `Authz::write` and answer the three
/// refusals as `400` rather than as the `500` an `AuthzError` becomes on its way out of `write`.
/// All three are a malformed request, not a server fault, and a client that cannot tell them apart
/// from an outage retries. It is pure, so running it twice costs nothing and changes nothing.
pub(crate) fn validate_write(write: &TupleWrite) -> Result<(), AuthzError> {
  let registry = Authz::registry()?;
  let definition = registry.require(&write.resource_type)?;

  let relation = definition.relation(&write.relation).ok_or_else(|| AuthzError::UnknownRelation {
    definition: write.resource_type.clone(),
    relation:   write.relation.clone(),
  })?;

  let RelationSource::Stored {
    subjects,
    allows_wildcard,
  } = &relation.source
  else {
    return Err(AuthzError::NotCompilable(format!(
      "`{}#{}` is derived from a domain column, so it cannot be granted as a tuple",
      write.resource_type, write.relation
    )));
  };

  let subject = &write.subject;
  let accepted = subjects.iter().any(|spec| match spec {
    SubjectSpec::Direct(ty) => *ty == subject.ty && subject.relation.is_empty() && subject.id != "*",
    SubjectSpec::Userset { ty, relation } => *ty == subject.ty && *relation == subject.relation,
    SubjectSpec::Wildcard(ty) => *allows_wildcard && *ty == subject.ty && subject.id == "*",
  });

  if !accepted {
    let declared: Vec<String> = subjects.iter().map(ToString::to_string).collect();
    return Err(AuthzError::Schema(format!(
      "`{}#{}` does not accept `{}`; it declares {}",
      write.resource_type,
      write.relation,
      subject.key(),
      declared.join(" | ")
    )));
  }

  Ok(())
}

#[cfg(test)]
mod write_validation_tests {
  use super::*;

  fn write(relation: &str, subject: SubjectKey) -> TupleWrite {
    TupleWrite::new("board_group", "7", relation, subject)
  }

  /// The shape a create path's owner seed writes.
  #[test]
  fn a_declared_subject_on_a_stored_relation_is_accepted() {
    validate_write(&write("owner", SubjectKey::user(1))).expect("a user may own a board group");
    validate_write(&write("viewer", SubjectKey::userset("organization", "100", "member")))
      .expect("an organization's members may view a board group");
  }

  /// `parent` is `board_groups.parent_id`. A tuple on it would be authority the evaluator never
  /// looks at, which is worse than a refusal: it reads as a grant.
  #[test]
  fn a_derived_relation_cannot_be_granted_as_a_tuple() {
    let error = validate_write(&write("parent", SubjectKey::user(1))).unwrap_err();
    assert!(matches!(&error, AuthzError::NotCompilable(m) if m.contains("derived")), "{error:?}");
  }

  /// The `admin:user` defect, refused at the API this time.
  #[test]
  fn a_subject_type_the_relation_never_declared_is_refused() {
    let error = validate_write(&write("owner", SubjectKey::parse("board:7").unwrap())).unwrap_err();
    assert!(matches!(&error, AuthzError::Schema(m) if m.contains("does not accept")), "{error:?}");
  }

  /// A relation nobody declared, named rather than met as a foreign-key violation.
  #[test]
  fn an_undeclared_relation_is_named() {
    let error = validate_write(&write("administrator", SubjectKey::user(1))).unwrap_err();
    assert!(matches!(error, AuthzError::UnknownRelation { .. }), "{error:?}");
  }

  /// `action: ["*"]` on a `service` object meant the four route actions, and must not survive as a
  /// relation nobody declared. Anything else passes through untouched, so a form that posts
  /// `op_read` still grants object-wide power rather than being silently downgraded.
  #[test]
  fn the_legacy_action_wildcard_expands_to_the_four_gate_relations() {
    assert_eq!(expand_service_actions(&["*".to_owned()]), vec!["create", "delete", "read", "update"]);
    assert_eq!(
      expand_service_actions(&["read".to_owned(), "*".to_owned()]),
      vec!["create", "delete", "read", "update"],
      "a wildcard beside a concrete action must not produce a duplicate `read`"
    );
    assert_eq!(expand_service_actions(&["operator".to_owned()]), vec!["operator"]);
  }

  /// Every relation the expansion can produce is one the `service` definition declares — otherwise
  /// the form's own vocabulary would be refused at write time rather than at review time.
  #[test]
  fn the_expanded_actions_are_relations_the_service_definition_declares() {
    for relation in expand_service_actions(&["*".to_owned()]) {
      validate_write(&TupleWrite::new("service", "user", &relation, SubjectKey::user(1)))
        .unwrap_or_else(|e| panic!("`service#{relation}` is not writable: {e}"));
    }
  }

  /// The subject key round-trips, colons in the id included — `delegation_scope:svc:alpha` is a
  /// real object id and splitting on the last colon would mangle it.
  #[test]
  fn subject_keys_round_trip() {
    for key in ["user:7", "role:1#member", "organization:100#member", "apikey:ab12cd"] {
      assert_eq!(SubjectKey::parse(key).unwrap().key(), key);
    }
    assert_eq!(SubjectKey::user(7).as_user_id(), Some(7));
    assert_eq!(SubjectKey::userset("role", "1", "member").as_user_id(), None);
  }
}

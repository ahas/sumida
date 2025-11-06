use std::sync::Arc;

use api::util::ap::{self, ApForUser, ApGuard, ApUser};
use chrono::Utc;
use e2e::prelude::*;
use entity::access_policies::{Column as ApCol, Entity as ApEntity};
use entity::prelude::*;
use infra::session::{UserEnvironment, UserProfile, UserSession, UserState};
use sea_orm::sea_query::Expr;
use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tower_sessions::{MemoryStore, Session};

async fn setup(namespace: &str) -> (Mock, DatabaseConnection) {
  let mock = Mock::new().await;
  let db = infra::db::connect().await;

  ApEntity::delete_many()
    .filter(ApCol::Namespace.eq(namespace))
    .exec(&db)
    .await
    .expect("failed to clean namespace");

  (mock, db)
}

async fn insert_policy(
  db: &impl ConnectionTrait,
  namespace: &str,
  object: &str,
  actions: Vec<String>,
  user: ApUser,
  effect: &str,
) {
  ap::edit()
    .namespace(namespace)
    .object(object)
    .action(actions)
    .user(user)
    .effect(effect)
    .create(db)
    .await
    .expect("failed to insert policy");
}

fn build_session(
  user: &entity::users::Model,
  org: &entity::organizations::Model,
  role: &entity::roles::Model,
  api_key: Option<&str>,
  is_admin: bool,
) -> UserSession {
  let store = Arc::new(MemoryStore::default());
  let session = Session::new(None, store, None);
  let now = Utc::now().into();

  let profile = UserProfile {
    id: user.id,
    username: user.username.clone(),
    full_name: user.full_name.clone().unwrap_or_default(),
    organization: infra::session::UserOrganization {
      id:   org.id,
      name: org.name.clone(),
    },
    role: infra::session::UserRole {
      id:   role.id,
      name: role.name.clone(),
    },
    is_admin,
    is_authorized: true,
    api_key: api_key.map(str::to_string),
    first_seen_at: now,
    last_seen_at: now,
    expiry: None,
  };

  UserSession {
    inner: session,
    profile,
    env: UserEnvironment::default(),
    state: UserState::default(),
  }
}

#[tokio::test]
async fn ap_condition_allows_and_blocks_with_effects() {
  let (_mock, db) = setup("ap.tests.allow").await;

  insert_policy(&db, "ap.tests.allow", "resource-a", vec!["read".into()], ApUser::User(1), "allow").await;
  insert_policy(&db, "ap.tests.allow", "resource-a", vec!["read".into()], ApUser::User(1), "deny").await;

  let guard = ap::ns("ap.tests.allow").object("resource-a").action("read").user(ApUser::User(1));

  assert!(!guard.clone().check_allowed(&db).await);
  let query = ApEntity::find().filter(ApCol::Object.eq("resource-a"));
  assert!(!guard.clone().can_access(query.clone(), &db).await);

  let allow_guard = ap::ns("ap.tests.allow").object("resource-a").action("read").user(ApUser::User(2));
  assert!(!allow_guard.check_allowed(&db).await);

  db.close().await.expect("close database");
}

#[tokio::test]
async fn ap_condition_supports_wildcards_and_exprs() {
  let (_mock, db) = setup("ap.tests.wildcards").await;

  insert_policy(&db, "ap.tests.wildcards", "*", vec!["*".into()], ApUser::User(33), "allow").await;

  let wildcard_guard =
    ap::ns("ap.tests.wildcards").object("anything").action("delete").user(ApUser::User(33));
  assert!(wildcard_guard.check_allowed(&db).await);

  let expr_guard = ap::ns("ap.tests.wildcards")
    .expr(Expr::val("dynamic").cast_as("varchar"))
    .action("inspect")
    .user(ApUser::User(33));
  insert_policy(&db, "ap.tests.wildcards", "dynamic", vec!["inspect".into()], ApUser::User(33), "allow")
    .await;
  assert!(expr_guard.check_allowed(&db).await);

  db.close().await.expect("close database");
}

#[tokio::test]
async fn ap_condition_who_expands_session_roles() {
  let (_mock, db) = setup("ap.tests.who").await;
  let user = TestUser::create(&db).await.expect("user");
  let session = build_session(
    &user.model,
    &Organizations::find_by_id(user.model.organization_id).one(&db).await.unwrap().unwrap(),
    &Roles::find_by_id(user.model.role_id).one(&db).await.unwrap().unwrap(),
    None,
    false,
  );

  insert_policy(
    &db,
    "ap.tests.who",
    "by-role",
    vec!["manage".into()],
    ApUser::Role(user.model.role_id),
    "allow",
  )
  .await;
  insert_policy(
    &db,
    "ap.tests.who",
    "by-org",
    vec!["view".into()],
    ApUser::Org(user.model.organization_id),
    "allow",
  )
  .await;

  let role_guard = ap::ns("ap.tests.who").object("by-role").action("manage").who(&session);
  assert!(role_guard.check_allowed(&db).await);

  let org_guard = ap::ns("ap.tests.who").object("by-org").action("view").who(&session);
  assert!(org_guard.check_allowed(&db).await);

  let api_session = build_session(
    &user.model,
    &Organizations::find_by_id(user.model.organization_id).one(&db).await.unwrap().unwrap(),
    &Roles::find_by_id(user.model.role_id).one(&db).await.unwrap().unwrap(),
    Some("api-key-1"),
    false,
  );
  insert_policy(
    &db,
    "ap.tests.who",
    "by-key",
    vec!["read".into()],
    ApUser::ApiKey("api-key-1".into()),
    "allow",
  )
  .await;
  let key_guard = ap::ns("ap.tests.who").object("by-key").action("read").who(&api_session);
  assert!(key_guard.check_allowed(&db).await);

  db.close().await.expect("close database");
}

#[tokio::test]
async fn ap_condition_column_and_join_filter_results() {
  let (_mock, db) = setup("ap.tests.columns").await;
  let allowed_user = TestUser::create(&db).await.expect("user");
  let banned_user = TestUser::create(&db).await.expect("user");

  insert_policy(
    &db,
    "ap.tests.columns",
    &allowed_user.model.id.to_string(),
    vec!["read".into()],
    ApUser::User(allowed_user.model.id),
    "allow",
  )
  .await;

  let guard = ap::ns("ap.tests.columns")
    .column(entity::users::Column::Id)
    .join(Users, entity::users::Column::Id)
    .action("read")
    .user(ApUser::User(allowed_user.model.id));

  let allowed = guard
    .clone()
    .apply(Users::find().filter(entity::users::Column::Id.eq(allowed_user.model.id)))
    .all(&db)
    .await
    .expect("query");
  assert_eq!(allowed.len(), 1);

  let blocked = guard
    .apply(Users::find().filter(entity::users::Column::Id.eq(banned_user.model.id)))
    .all(&db)
    .await
    .expect("query");
  assert!(blocked.is_empty());

  db.close().await.expect("close database");
}

#[tokio::test]
async fn ap_chain_requires_each_condition() {
  let (_mock, db) = setup("ap.tests.chain").await;

  insert_policy(&db, "ap.tests.chain", "a", vec!["read".into()], ApUser::User(500), "allow").await;
  insert_policy(&db, "ap.tests.chain", "b", vec!["update".into()], ApUser::User(500), "allow").await;

  let chain = ap::chain()
    .push(ap::ns("ap.tests.chain").object("a").action("read").user(ApUser::User(500)))
    .push(ap::ns("ap.tests.chain").object("b").action("update").user(ApUser::User(500)));

  assert!(chain.clone().check_allowed(&db).await);

  ap::edit()
    .namespace("ap.tests.chain")
    .object("b")
    .action(vec!["update".into()])
    .user(ApUser::User(500))
    .update("deny", &db)
    .await
    .expect("set effect");

  assert!(!chain.check_allowed(&db).await);

  db.close().await.expect("close database");
}

#[tokio::test]
async fn find_ap_for_users_handles_save_update_clear() {
  let (_mock, db) = setup("ap.tests.for_users").await;
  let user = ApUser::User(600);

  ap::for_users()
    .user(user.clone())
    .save(
      vec![ApForUser {
        namespace: "ap.tests.for_users".into(),
        object:    vec!["doc-a".into(), "doc-b".into()],
        action:    vec!["read".into(), "write".into()],
        effect:    "allow".into(),
      }],
      &db,
    )
    .await
    .expect("save policies");

  let stored = ap::for_users().user(user.clone()).find_all(&db).await.expect("find");
  assert_eq!(stored.len(), 2);

  ap::for_users()
    .user(user.clone())
    .update(
      vec![ApForUser {
        namespace: "ap.tests.for_users".into(),
        object:    vec!["doc-a".into()],
        action:    vec!["read".into()],
        effect:    "deny".into(),
      }],
      &db,
    )
    .await
    .expect("update policies");

  let updated = ap::for_users().user(user.clone()).find_all(&db).await.expect("find");
  assert_eq!(updated[0].effect, "deny");

  let deleted = ap::for_users().user(user.clone()).clear(&db).await.expect("clear");
  assert!(deleted.rows_affected > 0);

  let empty = ap::for_users().user(user).find_all(&db).await.expect("find");
  assert!(empty.is_empty());

  db.close().await.expect("close database");
}

#[tokio::test]
async fn find_users_of_ap_lists_all_principals() {
  let (_mock, db) = setup("ap.tests.find_users").await;

  insert_policy(&db, "ap.tests.find_users", "shared", vec!["read".into()], ApUser::Admin, "allow").await;
  insert_policy(&db, "ap.tests.find_users", "shared", vec!["read".into()], ApUser::Org(1), "allow").await;
  insert_policy(&db, "ap.tests.find_users", "shared", vec!["read".into()], ApUser::Role(2), "allow").await;
  insert_policy(&db, "ap.tests.find_users", "shared", vec!["read".into()], ApUser::User(3), "allow").await;
  insert_policy(
    &db,
    "ap.tests.find_users",
    "shared",
    vec!["read".into()],
    ApUser::ApiKey("key-find".into()),
    "allow",
  )
  .await;

  let mut principals = ap::find_users("ap.tests.find_users")
    .object("shared")
    .action(vec!["read".into()])
    .all(&db)
    .await;
  principals.sort_by_key(|p| format!("{p:?}"));

  assert!(principals.contains(&ApUser::Admin));
  assert!(principals.contains(&ApUser::Org(1)));
  assert!(principals.contains(&ApUser::Role(2)));
  assert!(principals.contains(&ApUser::User(3)));
  assert!(principals.contains(&ApUser::ApiKey("key-find".into())));

  db.close().await.expect("close database");
}

#[tokio::test]
async fn edit_ap_set_effect_updates_existing_records() {
  let (_mock, db) = setup("ap.tests.edit").await;

  insert_policy(&db, "ap.tests.edit", "shared", vec!["edit".into()], ApUser::User(700), "allow").await;

  ap::edit()
    .namespace("ap.tests.edit")
    .object("shared")
    .action(vec!["edit".into()])
    .user(ApUser::User(700))
    .update("deny", &db)
    .await
    .expect("update effect");

  let guard = ap::ns("ap.tests.edit").object("shared").action("edit").user(ApUser::User(700));
  assert!(!guard.check_allowed(&db).await);

  db.close().await.expect("close database");
}

use axum::Router;

use crate::SumidaState;
use crate::layer::{ServiceApLayer, UserGuardLayer};
use crate::util::filter::{FilterSchema, filter_descriptor_route};

pub mod access_log;
pub mod api_key;
pub mod auth;
pub mod authz;
pub mod ban;
#[cfg(feature = "board")]
pub mod bps;
pub mod dashboard;
pub mod dashboard_overview;
#[cfg(feature = "file")]
pub mod fs;
#[cfg(feature = "mail-form")]
pub mod mail_form;
pub mod me;
#[cfg(feature = "membership")]
pub mod membership;
pub mod organization;
pub mod relation_tuple;
pub mod role;
pub mod user;
pub mod user_invitation;
pub mod user_verification;

pub mod prelude;

pub trait ApiController<StateT>
where
  StateT: Clone + Send + Sync + 'static,
{
  fn router(self, state: &StateT) -> axum::Router<StateT>;

  fn mount(self, prefix: &str, state: &StateT, root: axum::Router<StateT>) -> axum::Router<StateT>
  where
    Self: Sized,
  {
    root.nest(prefix, self.router(state))
  }
}

/// The per-action route split, held by one object instead of by every author copying the shape.
///
/// The namespace is stated once, at construction. Each action's routes receive that action's gate
/// and no other, the descriptor route reuses the same namespace the read routes gate on, and the
/// session guard goes over the whole bundle exactly once. There is no way to hand two actions to
/// one gate, which is the property [`ServiceApLayer`] depends on: a bundle that mixes actions is
/// checked against the weakest action in it.
///
/// A controller supplies only the routes it owns. Which paths, which methods and which handlers
/// belong to an action stay with the controller; the wiring that must not vary stays here.
pub struct ControllerRouter<'a, StateT: SumidaState> {
  state:     &'a StateT,
  namespace: &'a str,
  read:      Router<StateT>,
  create:    Router<StateT>,
  update:    Router<StateT>,
  delete:    Router<StateT>,
  ungated:   Router<StateT>,
  /// Routes gated by a namespace that is not this bundle's.
  foreign:   Router<StateT>,
}

impl<'a, StateT> ControllerRouter<'a, StateT>
where
  StateT: SumidaState,
{
  /// Start a bundle for one namespace — the string every gate below is built from.
  pub fn new(state: &'a StateT, namespace: &'a str) -> Self {
    Self {
      state,
      namespace,
      read: Router::new(),
      create: Router::new(),
      update: Router::new(),
      delete: Router::new(),
      ungated: Router::new(),
      foreign: Router::new(),
    }
  }

  /// Routes that only read. Called more than once, the routers accumulate under the one gate.
  pub fn read(mut self, router: Router<StateT>) -> Self {
    self.read = self.read.merge(router);
    self
  }

  pub fn create(mut self, router: Router<StateT>) -> Self {
    self.create = self.create.merge(router);
    self
  }

  pub fn update(mut self, router: Router<StateT>) -> Self {
    self.update = self.update.merge(router);
    self
  }

  pub fn delete(mut self, router: Router<StateT>) -> Self {
    self.delete = self.delete.merge(router);
    self
  }

  /// `GET <resource>/filters` for the resource's declared filter schema, joined to the read routes
  /// so it stands behind the very gate the list stands behind — the namespace cannot be spelled
  /// differently here because it is not spelled here at all.
  ///
  /// # Panics
  ///
  /// As [`filter_descriptor_route`], and for the same reason.
  pub fn descriptor<F>(self) -> Self
  where
    F: FilterSchema + Send + Sync + 'static,
  {
    self.read(filter_descriptor_route::<StateT, F>())
  }

  /// Routes that take the session guard but no route gate, for the few endpoints whose
  /// authorization is settled inside the handler rather than by the namespace. Naming the method is
  /// the declaration: an endpoint arrives here on purpose, not by being left out of an action.
  pub fn ungated(mut self, router: Router<StateT>) -> Self {
    self.ungated = self.ungated.merge(router);
    self
  }

  /// Routes whose gate is another namespace's.
  ///
  /// A route can act on this resource while the capability it needs belongs elsewhere — launching a
  /// job produces a task, so it is the task namespace that decides who may do it. Such a route cannot
  /// go in an action bundle, because those all derive their gate from the one namespace this bundle
  /// was opened with, and it must not go in [`Self::ungated`], because it is gated. It joins here so
  /// it carries its own gate and still takes the session guard once with everything else.
  pub fn gated_by(mut self, namespace: &str, action: &str, router: Router<StateT>) -> Self {
    self.foreign =
      self
        .foreign
        .merge(router.route_layer(ServiceApLayer::new(self.state.clone(), namespace, action)));
    self
  }

  /// Apply each action's gate, merge, and put the session guard over the result.
  ///
  /// An action the controller never named holds no routes, and axum refuses a layer that has
  /// nothing under it, so such an action contributes nothing instead of a gate guarding no route.
  pub fn build(self) -> Router<StateT> {
    let Self {
      state,
      namespace,
      read,
      create,
      update,
      delete,
      ungated,
      foreign,
    } = self;
    let gate = |router: Router<StateT>, action: &str| match router.has_routes() {
      true => router.route_layer(ServiceApLayer::new(state.clone(), namespace, action)),
      false => router,
    };

    Router::new()
      .merge(gate(read, "read"))
      .merge(gate(create, "create"))
      .merge(gate(update, "update"))
      .merge(gate(delete, "delete"))
      .merge(ungated)
      .merge(foreign)
      .route_layer(UserGuardLayer::new(state.clone()))
  }
}

#[cfg(test)]
mod tests {
  use super::ApiController;
  use crate::SumidaState;
  use crate::controller::access_log::AccessLogController;
  use crate::controller::api_key::ApiKeyController;
  use crate::controller::ban::BanController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board::BoardController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_file::BoardFileController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_group::BoardGroupController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post::BoardPostController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post_reaction::BoardPostReactionController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_post_report::BoardPostReportController;
  #[cfg(feature = "board")]
  use crate::controller::bps::board_topic::BoardTopicController;
  #[cfg(feature = "file")]
  use crate::controller::fs::file::FileController;
  #[cfg(feature = "mail-form")]
  use crate::controller::mail_form::MailFormController;
  #[cfg(feature = "membership")]
  use crate::controller::membership::coupon::CouponController;
  #[cfg(feature = "membership")]
  use crate::controller::membership::plan::PlanController;
  use crate::controller::organization::OrganizationController;
  use crate::controller::relation_tuple::RelationTupleController;
  use crate::controller::role::RoleController;
  use crate::controller::user::UserController;
  use crate::controller::user_invitation::UserInvitationController;

  #[derive(Clone)]
  struct DummyState;

  impl SumidaState for DummyState {
    fn db(&self) -> &sea_orm::DatabaseConnection {
      unreachable!("assembling a router never reaches the database")
    }
  }

  /// Assembly is where a duplicated path and an invalid filter declaration both panic, and it is
  /// the one part of a controller that runs before any request does.
  #[test]
  fn converted_controllers_assemble() {
    let state = DummyState;

    assert!(RoleController::new().router(&state).has_routes());
    assert!(UserController::new().router(&state).has_routes());
    assert!(OrganizationController::new().router(&state).has_routes());
    assert!(ApiKeyController::new().router(&state).has_routes());
    assert!(AccessLogController::new().router(&state).has_routes());
    assert!(BanController::new().router(&state).has_routes());
    assert!(RelationTupleController::new().router(&state).has_routes());
    assert!(UserInvitationController::new().router(&state).has_routes());

    #[cfg(feature = "mail-form")]
    assert!(MailFormController::new().router(&state).has_routes());

    #[cfg(feature = "board")]
    {
      assert!(BoardController::new().router(&state).has_routes());
      assert!(BoardFileController::new().router(&state).has_routes());
      assert!(BoardGroupController::new().router(&state).has_routes());
      assert!(BoardPostController::new().router(&state).has_routes());
      assert!(BoardPostReactionController::new().router(&state).has_routes());
      assert!(BoardPostReportController::new().router(&state).has_routes());
      assert!(BoardTopicController::new().router(&state).has_routes());
    }

    #[cfg(feature = "file")]
    assert!(FileController::new().router(&state).has_routes());

    #[cfg(feature = "membership")]
    {
      assert!(CouponController::new().router(&state).has_routes());
      assert!(PlanController::new().router(&state).has_routes());
    }
  }
}

/// What an instant looks like in what this surface writes, and what keeps it that way.
///
/// A timestamp column is `timestamptz`, and the two chrono types that can hold one differ in what
/// they remember. `DateTime<FixedOffset>` keeps the offset the value arrived in — for a row read
/// from the database that is the connection session's offset, which is deployment configuration
/// and nothing the caller asked for, so the same code answers `+09:00` on one deployment and
/// `+00:00` on another. `DateTime<Utc>` has no room for an offset at all: the instant is the same
/// and the string is always the one written with a trailing `Z`.
///
/// So the spelling is not a step anything runs on the way out — it is the type, and the type is
/// the only one the workspace names. A timestamp field written like the ones beside it is right
/// with nothing for its author to remember, and the walk below refuses the offset-carrying
/// spelling everywhere except this file, which is where the difference is stated.
#[cfg(test)]
mod instant_tests {
  use std::fs;
  use std::path::{Path, PathBuf};

  use chrono::{DateTime, FixedOffset, TimeZone, Utc};
  use sea_orm::prelude::DateTimeUtc;

  /// One second into 2026, the instant every case below is the same moment of.
  fn instant() -> DateTimeUtc {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).single().expect("an unambiguous moment")
  }

  /// What a value is written as, without the quotes JSON puts around a string.
  fn written<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).expect("an instant is JSON").as_str().expect("a string").to_owned()
  }

  /// The offsets a database session may be set to, spanning both signs and the far ends.
  const SESSION_OFFSETS: [i32; 6] = [-11, -5, 0, 5, 9, 14];

  /// Read through any session offset, one instant is written as one string.
  ///
  /// This answers for the type rather than for a field of it, so every timestamp the surface
  /// carries today and every one added later is covered by this one case.
  #[test]
  fn an_instant_is_written_as_utc_whatever_session_it_was_read_through() {
    for hours in SESSION_OFFSETS {
      let session = FixedOffset::east_opt(hours * 3600).expect("an offset a session can be set to");
      let as_read: DateTimeUtc = instant().with_timezone(&session).into();

      assert_eq!(written(&as_read), "2026-01-01T00:00:01Z", "read through {hours:+}");
    }
  }

  /// The type is what does it. Kept in the one that has room for an offset, the same instant is
  /// written with the session's offset instead — which is the failure this whole arrangement is
  /// against, and it is reproduced here so the case above cannot pass by accident.
  #[test]
  fn the_offset_carrying_type_writes_the_session_instead() {
    let session = FixedOffset::east_opt(9 * 3600).expect("an offset a session can be set to");
    let carried: DateTime<FixedOffset> = instant().with_timezone(&session);

    assert_eq!(written(&carried), "2026-01-01T09:00:01+09:00");
  }

  /// A fraction of a second survives, and takes the same ending.
  #[test]
  fn a_fraction_of_a_second_is_written_with_the_same_ending() {
    let precise = instant() + chrono::Duration::milliseconds(500);

    assert_eq!(written(&precise), "2026-01-01T00:00:01.500Z");
  }

  /// Reading is unchanged: an offset is still accepted, and it names the instant it names.
  ///
  /// What this change settles is the spelling of an answer. A caller that has always sent its own
  /// offset goes on being understood, and refusing it would be a new rejection of something the
  /// surface takes today.
  #[test]
  fn an_instant_is_read_whatever_offset_it_arrives_in() {
    for arriving in ["2026-01-01T00:00:01Z", "2026-01-01T09:00:01+09:00", "2025-12-31T19:00:01-05:00"] {
      let read: DateTimeUtc =
        serde_json::from_value(serde_json::Value::String(arriving.to_owned())).expect("readable");

      assert_eq!(read, instant(), "`{arriving}` was read as another moment");
    }
  }

  /// A range parameter reads the same way, so a filter is spelled however the caller spells it.
  #[test]
  fn a_range_parameter_is_read_whatever_offset_it_arrives_in() {
    let asked: sea_orm_crud::Bounds<DateTimeUtc> =
      serde_urlencoded::from_str::<std::collections::HashMap<String, sea_orm_crud::Bounds<DateTimeUtc>>>(
        "at=2026-01-01T09%3A00%3A01%2B09%3A00..2026-01-01T09%3A00%3A01%2B09%3A00",
      )
      .expect("a readable range")
      .remove("at")
      .expect("the parameter that was sent");

    assert_eq!(asked, sea_orm_crud::Bounds::Between(instant(), instant()));
  }

  /// Every `.rs` file in the workspace, `target` left out.
  fn sources(from: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(from).expect("a directory that is there") {
      let path = entry.expect("an entry that can be read").path();

      if path.is_dir() {
        if !matches!(path.file_name().and_then(|name| name.to_str()), Some("target" | ".git")) {
          sources(&path, into);
        }
      } else if path.extension().is_some_and(|extension| extension == "rs") {
        into.push(path);
      }
    }
  }

  /// Nothing outside this file names the type that carries an offset.
  ///
  /// A field cannot opt out of the spelling by forgetting something, only by naming the other type
  /// on purpose — so that is what is watched for, and it is watched for across the workspace
  /// rather than in the response types alone. An instant read into the offset-carrying type
  /// anywhere reaches a response body eventually, and by then which field it came from tells
  /// nobody anything.
  #[test]
  fn no_source_but_this_one_names_the_offset_carrying_instant() {
    // Assembled rather than written out, so the file this rule is stated in is not the file that
    // breaks it. It is skipped below in any case, and this keeps the two from depending on that.
    let refused = [format!("Fixed{}", "Offset"), format!("DateTime{}", "WithTimeZone")];

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("a workspace above this crate");
    let mut found = Vec::new();
    sources(root, &mut found);

    assert!(found.len() > 100, "the walk read {} files, which is not the workspace", found.len());

    let mut named: Vec<String> = Vec::new();
    for path in found {
      if path.ends_with(file!()) {
        continue;
      }

      let source = fs::read_to_string(&path).expect("a source file that can be read");
      for spelling in &refused {
        if source.contains(spelling.as_str()) {
          named.push(format!("{} names `{spelling}`", path.display()));
        }
      }
    }

    assert!(named.is_empty(), "an instant has one spelling here: {named:#?}");
  }
}

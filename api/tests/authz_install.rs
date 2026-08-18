//! `Registry::install` — the seam a consumer hands its rules through, and the ordering it enforces.
//!
//! A binary of its own because the thing under test is **process-wide state**. Which rules were
//! installed, and whether the registry has been read yet, are two `OnceLock`s; a case that asserts
//! "installing after the first read is refused" cannot share a process with a case that expects the
//! registry to be un-read. Rust's test order inside one binary is not something to depend on, so
//! the isolation is a separate binary rather than a convention.
//!
//! No database and no server: everything here is parse and validate.

use api::authz::Authz;
use api::authz::registry::{Registry, RuleFile};

/// A consumer's rule file. `installed_widget` is declared nowhere else, so its presence in the
/// registry is proof the installed file was really loaded rather than merely accepted.
const CONSUMER: &[RuleFile] = &[RuleFile {
  name: "consumer.sz",
  text: "extend organization { scope widget }

         definition installed_widget on widgets.slug {
           relation org: organization = column(widgets.organization_id -> organization
                                               where widgets.organization_id IS NOT NULL)
           relation viewer: user | role#member | organization#member | apikey
           permission view = viewer | org->view_widget | service:demo:widget->operate_read
         }

         namespace demo:widget service = installed_widget",
}];

/// Byte-identical to `CONSUMER`, as a *different* slice. `install` has to accept this — the router
/// and the boot sequence each declare the same rules — while refusing a slice that says something
/// else.
const CONSUMER_AGAIN: &[RuleFile] = &[RuleFile {
  name: "consumer.sz",
  text: "extend organization { scope widget }

         definition installed_widget on widgets.slug {
           relation org: organization = column(widgets.organization_id -> organization
                                               where widgets.organization_id IS NOT NULL)
           relation viewer: user | role#member | organization#member | apikey
           permission view = viewer | org->view_widget | service:demo:widget->operate_read
         }

         namespace demo:widget service = installed_widget",
}];

const OTHER: &[RuleFile] = &[RuleFile {
  name: "other.sz",
  text: "definition installed_other on others.id int {
           relation viewer: user
           permission view = viewer
         }",
}];

/// The whole ordering contract, in the order a process meets it.
///
/// One test rather than four, because the state it walks through is the process's and the steps are
/// only meaningful in sequence: repeat-install is only interesting *before* the first read, and
/// late-install is only interesting *after* it.
#[test]
fn install_is_idempotent_before_the_first_read_and_refused_after_it() {
  Authz::install_rules(CONSUMER).expect("the first install");
  Authz::install_rules(CONSUMER_AGAIN).expect("the same declaration, from a second call site");

  let replaced = Authz::install_rules(OTHER).expect_err("a different declaration must be refused");
  assert!(
    replaced.to_string().contains("already installed"),
    "the failure must say what is already installed, so the second caller can be found: {replaced}"
  );

  // Reading freezes it — and proves the installed file is really in the rule set rather than
  // merely accepted.
  let registry = Authz::registry().expect("sumida's rules plus the consumer's must validate");
  assert!(
    registry.get("installed_widget").is_some(),
    "the installed file did not reach the registry"
  );
  assert_eq!(registry.source_of("installed_widget"), Some("consumer.sz"));
  assert!(
    registry.routes_service_gate("demo:widget"),
    "the installed file's namespace inventory did not reach the registry"
  );

  // `extend organization` reached sumida's own definition, and left it sumida's.
  let organization = registry.get("organization").expect("organization is declared");
  assert!(
    organization.permission("view_widget").is_some(),
    "the consumer's `scope widget` did not reach `organization`"
  );
  assert_eq!(registry.source_of("organization"), Some("schema.sz"));

  // A repeat of the *installed* declaration is still fine after the read — it changes nothing.
  Authz::install_rules(CONSUMER).expect("the same declaration, after the read");

  let late = Authz::install_rules(OTHER).expect_err("a second declaration must still be refused");
  assert!(
    late.to_string().contains("already installed"),
    "the failure must name what is already installed: {late}"
  );

  // The refused install changed nothing.
  assert!(
    Registry::global().expect("still valid").get("installed_other").is_none(),
    "a refused install reached the registry anyway"
  );
}

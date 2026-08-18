//! The ordering trap: rules installed **after** the rule set has already been read.
//!
//! A binary of its own, and a separate one from `authz_install.rs`, because the only way to reach
//! this state is to read the registry before installing anything — and once a process has installed
//! something it can never get back to "nothing installed". The two cases cannot share a process, so
//! they do not share a binary.
//!
//! This is the trap a product's `main` walks into by writing `boot_or_abort` above the line that
//! registers its rules: boot then validates sumida's rules alone, reports every definition healthy,
//! and the product's own rules are never checked against the database at all. It has to be an error
//! rather than a warning — a process that answered one request with one rule set and the next with
//! another is two permission models wearing one deployment.

use api::authz::Authz;
use api::authz::registry::RuleFile;

const CONSUMER: &[RuleFile] = &[RuleFile {
  name: "late.sz",
  text: "definition late_thing on things.id int {
           relation viewer: user
           permission view = viewer
         }",
}];

#[test]
fn installing_rules_after_the_first_read_is_refused_and_declaring_none_is_not() {
  // The read that freezes it. In a product this is `boot_or_abort`, or anything else that asks the
  // evaluator a question.
  let registry = Authz::registry().expect("sumida's own rules must validate");
  assert!(
    registry.get("late_thing").is_none(),
    "nothing is installed yet, so the registry is sumida's rules alone"
  );

  let late = Authz::install_rules(CONSUMER).expect_err("installing after the read must be refused");
  assert!(
    late.to_string().contains("already been read"),
    "the failure must say the rule set is already frozen, and say where the install belongs: {late}"
  );

  // And the refusal did not half-apply.
  assert!(
    Authz::registry().expect("still valid").get("late_thing").is_none(),
    "a refused install reached the registry anyway"
  );

  // Declaring **no** rules after the read is not the same thing and is not refused: the rule set
  // that was read is sumida's alone, which is precisely what an empty slice says. A product with no
  // rules of its own has to be able to say so, wherever in its boot sequence it says it.
  Authz::install_rules(&[]).expect("declaring no rules of one's own is never a disagreement");
}

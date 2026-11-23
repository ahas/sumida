//! Attempts counted in this process, per key, over a fixed window.
//!
//! What it is for is the routes that take a secret anyone may guess at without signing in — a
//! verification code above all — where the only thing keeping the secret out of reach is how few
//! guesses one caller gets. It counts in memory and nowhere else: a restart forgets every count, and
//! several instances each keep their own. The limits it is given are chosen with both in mind.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many keys are kept before the ones whose window has run out are swept. A sweep walks every
/// key, so it is done in bulk rather than on each call.
const SWEEP_ABOVE: usize = 10_000;

#[derive(Debug)]
struct Window {
  started: Instant,
  span:    Duration,
  count:   u32,
}

impl Window {
  fn elapsed(&self, now: Instant) -> bool {
    now.duration_since(self.started) >= self.span
  }
}

/// Counts per key over a fixed window.
#[derive(Debug, Default)]
pub struct Throttle {
  windows: Mutex<HashMap<String, Window>>,
}

impl Throttle {
  pub fn new() -> Self {
    Self::default()
  }

  /// Counts one more attempt under `key` and answers how many this window now holds, this one
  /// included. A window that has run out starts again from this attempt.
  ///
  /// The count is taken before the caller looks at what the attempt carries, so two attempts that
  /// arrive together are two attempts — neither can be judged against a count the other has not yet
  /// added to.
  pub fn take(&self, key: &str, span: Duration) -> u32 {
    self.take_at(key, span, Instant::now())
  }

  /// Whether one more attempt under `key` stays within `limit` for this window. The attempt is
  /// counted either way.
  pub fn admit(&self, key: &str, limit: u32, span: Duration) -> bool {
    self.take(key, span) <= limit
  }

  /// Forgets `key`, so its next attempt starts a new window.
  pub fn forget(&self, key: &str) {
    self.windows.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
  }

  fn take_at(&self, key: &str, span: Duration, now: Instant) -> u32 {
    let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());

    if windows.len() > SWEEP_ABOVE {
      windows.retain(|_, window| !window.elapsed(now));
    }

    let window = windows.entry(key.to_owned()).or_insert(Window {
      started: now,
      span,
      count: 0,
    });
    if window.elapsed(now) {
      *window = Window {
        started: now,
        span,
        count: 0,
      };
    }

    window.count = window.count.saturating_add(1);
    window.count
  }
}

#[cfg(test)]
mod tests {
  use std::time::{Duration, Instant};

  use super::Throttle;

  const HOUR: Duration = Duration::from_secs(3600);

  #[test]
  fn attempts_within_the_limit_are_admitted_and_the_next_is_not() {
    let throttle = Throttle::new();

    for _ in 0..5 {
      assert!(throttle.admit("issue:user:1", 5, HOUR));
    }
    assert!(
      !throttle.admit("issue:user:1", 5, HOUR),
      "the sixth attempt in the window is over the limit"
    );
    assert!(!throttle.admit("issue:user:1", 5, HOUR), "and it stays over for the rest of the window");
  }

  #[test]
  fn keys_are_counted_apart() {
    let throttle = Throttle::new();

    for _ in 0..5 {
      throttle.take("issue:user:1", HOUR);
    }
    assert!(throttle.admit("issue:user:2", 5, HOUR), "another account's count is its own");
  }

  #[test]
  fn a_window_that_has_run_out_starts_again() {
    let throttle = Throttle::new();
    let start = Instant::now();

    assert_eq!(throttle.take_at("k", HOUR, start), 1);
    assert_eq!(throttle.take_at("k", HOUR, start + Duration::from_secs(10)), 2);
    assert_eq!(
      throttle.take_at("k", HOUR, start + HOUR),
      1,
      "the window passed, so the count starts over"
    );
  }

  #[test]
  fn a_forgotten_key_starts_again() {
    let throttle = Throttle::new();

    throttle.take("k", HOUR);
    throttle.take("k", HOUR);
    throttle.forget("k");

    assert_eq!(throttle.take("k", HOUR), 1);
  }
}

//! Retry timing.
//!
//! The wait and nothing else. What is worth retrying, how many attempts it gets, and what counts
//! as a failure are policy, they differ per caller, and they stay with the caller — a helper that
//! decided any of them would be deciding it for every product at once.

use std::time::Duration;

/// The wait before the next attempt, doubling `base` once per attempt already spent.
///
/// `attempt` counts the attempt that just failed, from one: the first failure waits `base`, the
/// second `2 × base`, the third `4 × base`. Zero reads as one, so a caller that counts its
/// attempts from zero gets the base wait rather than a surprise.
///
/// The doubling saturates at `Duration::MAX`. Wrapping would turn an absurd attempt number into a
/// near-zero wait, which is the one outcome worse than waiting forever: it retries a failing
/// dependency as fast as the loop can go.
pub fn backoff_delay(base: Duration, attempt: u32) -> Duration {
  match 1u32.checked_shl(attempt.saturating_sub(1)) {
    Some(factor) => base.checked_mul(factor).unwrap_or(Duration::MAX),
    None => Duration::MAX,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_wait_doubles_once_per_spent_attempt() {
    let base = Duration::from_secs(2);

    assert_eq!(backoff_delay(base, 1), Duration::from_secs(2));
    assert_eq!(backoff_delay(base, 2), Duration::from_secs(4));
    assert_eq!(backoff_delay(base, 3), Duration::from_secs(8));
  }

  /// Counting attempts from zero is a caller's choice, and it must not fall off the bottom.
  #[test]
  fn a_zeroth_attempt_waits_the_base() {
    assert_eq!(backoff_delay(Duration::from_secs(2), 0), Duration::from_secs(2));
  }

  /// An attempt number past the width of the shift must saturate, never wrap into a short wait.
  #[test]
  fn the_doubling_saturates_instead_of_wrapping() {
    assert_eq!(backoff_delay(Duration::from_secs(2), 33), Duration::MAX);
    assert_eq!(backoff_delay(Duration::from_secs(2), u32::MAX), Duration::MAX);
  }
}

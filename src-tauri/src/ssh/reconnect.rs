//! Exponential-backoff helper (spec 03 §3).
//!
//! The reconnect LOOP itself lives in the `engine.rs` supervisor (F21 — a
//! single long-lived task loops across attempts); this module is only the pure,
//! unit-testable delay calculator.

use std::time::Duration;

/// Backoff delay for reconnect `attempt` (0-based): `delay_sec * 2^attempt`,
/// clamped to `[1, 60]` seconds (spec 03 §3). For `delay_sec = 5` the sequence
/// is 5, 10, 20, 40, 60, 60, … and never drops below 1s.
///
/// `2u64.pow(attempt)` is guarded: attempts large enough to overflow are
/// saturated so the clamp still yields 60s rather than panicking.
pub fn backoff(delay_sec: u32, attempt: u32) -> Duration {
    let factor = 2u64.checked_pow(attempt).unwrap_or(u64::MAX);
    let secs = (delay_sec as u64).saturating_mul(factor).clamp(1, 60);
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_sequence_for_delay_5() {
        // spec 03 §3 acceptance: 5, 10, 20, 40, 60 (clamped), 60, …
        assert_eq!(backoff(5, 0), Duration::from_secs(5));
        assert_eq!(backoff(5, 1), Duration::from_secs(10));
        assert_eq!(backoff(5, 2), Duration::from_secs(20));
        assert_eq!(backoff(5, 3), Duration::from_secs(40));
        assert_eq!(backoff(5, 4), Duration::from_secs(60)); // clamped from 80
        assert_eq!(backoff(5, 5), Duration::from_secs(60)); // clamped from 160
    }

    #[test]
    fn backoff_never_below_1s() {
        // delay 0 would compute 0 → clamped up to the 1s floor.
        assert_eq!(backoff(0, 0), Duration::from_secs(1));
        assert_eq!(backoff(0, 10), Duration::from_secs(1));
    }

    #[test]
    fn backoff_never_above_60s() {
        for attempt in 0..64 {
            assert!(backoff(1, attempt) <= Duration::from_secs(60));
            assert!(backoff(60, attempt) <= Duration::from_secs(60));
        }
    }

    #[test]
    fn backoff_large_attempt_does_not_panic() {
        // 2^attempt would overflow u64; saturation keeps it clamped at 60.
        assert_eq!(backoff(5, 1000), Duration::from_secs(60));
    }
}

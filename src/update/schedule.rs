//! When to check automatically: about a minute after launch if the last check
//! was more than a day ago, then once a day while the app runs. The timer wakes
//! at least hourly and compares wall-clock times, so a Mac that slept through
//! the day still checks soon after it wakes.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const AFTER_LAUNCH: Duration = Duration::from_secs(60);
const WAKE: Duration = Duration::from_secs(60 * 60);

/// How long until the next check is due: zero when it's due now (never checked,
/// a day or more ago, or a last check in the future because the clock changed).
pub fn next_check(last: Option<SystemTime>, now: SystemTime) -> Duration {
    match last.map(|l| now.duration_since(l)) {
        Some(Ok(since)) if since < INTERVAL => INTERVAL - since,
        _ => Duration::ZERO,
    }
}

/// The first wait after launch.
pub fn first_wait(last: Option<SystemTime>, now: SystemTime) -> Duration {
    next_check(last, now).max(AFTER_LAUNCH)
}

/// Bumped by every `start`, so timers from an earlier start stop.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Run `check` whenever one is due and `enabled()` says yes. Call again after the
/// setting changes to start over (with the after-launch wait). Main thread.
pub fn start(enabled: fn() -> bool, last: fn() -> Option<SystemTime>, check: fn()) {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    arm(first_wait(last(), SystemTime::now()).min(WAKE), generation, enabled, last, check);
}

fn arm(wait: Duration, generation: u64, enabled: fn() -> bool, last: fn() -> Option<SystemTime>, check: fn()) {
    super::main_after(wait, move || {
        if GENERATION.load(Ordering::Relaxed) != generation {
            return;
        }
        let mut next = next_check(last(), SystemTime::now());
        if enabled() && next.is_zero() {
            check();
            // `last` changes when the check answers. Until then (or if it fails), wait an hour.
            next = WAKE;
        }
        arm(next.clamp(AFTER_LAUNCH, WAKE), generation, enabled, last, check);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn checks_a_minute_after_launch_when_a_day_has_passed() {
        let now = SystemTime::now();
        assert_eq!(first_wait(None, now), AFTER_LAUNCH);
        assert_eq!(first_wait(Some(now - 25 * HOUR), now), AFTER_LAUNCH);
        assert_eq!(first_wait(Some(now - 24 * HOUR), now), AFTER_LAUNCH);
        // Checked an hour ago: the next one is 23 hours away.
        assert_eq!(first_wait(Some(now - HOUR), now), 23 * HOUR);
        // Due in 10 seconds: still waits the minute after launch.
        assert_eq!(first_wait(Some(now - INTERVAL + Duration::from_secs(10)), now), AFTER_LAUNCH);
    }

    #[test]
    fn checks_once_a_day() {
        let now = SystemTime::now();
        assert_eq!(next_check(Some(now), now), INTERVAL);
        assert_eq!(next_check(Some(now - 12 * HOUR), now), 12 * HOUR);
        assert!(next_check(Some(now - 24 * HOUR), now).is_zero());
        assert!(next_check(Some(now - 400 * HOUR), now).is_zero());
        assert!(next_check(None, now).is_zero());
        // The clock went back: check rather than wait for a time that may be days away.
        assert!(next_check(Some(now + 48 * HOUR), now).is_zero());
    }
}

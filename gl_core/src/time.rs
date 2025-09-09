// ABOUTME: Utilities for working with times and timestamps.
// ABOUTME: Provides RFC3339 formatting and monotonic timers.
use ::time::{format_description::well_known::Rfc3339, OffsetDateTime};
use std::time::{Duration, Instant, SystemTime};

/// Get the current system time (note: not necessarily UTC)
///
/// # Examples
///
/// ```
/// use gl_core::utc_now;
/// let now = utc_now();
/// assert!(now.duration_since(std::time::UNIX_EPOCH).is_ok());
/// ```
pub fn utc_now() -> SystemTime {
    SystemTime::now()
}

/// Convert a SystemTime to an RFC3339 timestamp string
///
/// # Examples
///
/// ```
/// use gl_core::to_rfc3339;
/// use std::time::{SystemTime, UNIX_EPOCH, Duration};
///
/// let time = UNIX_EPOCH + Duration::from_secs(1_609_459_200); // 2021-01-01
/// let timestamp = to_rfc3339(time);
/// assert_eq!(timestamp, "2021-01-01T00:00:00Z");
/// ```
pub fn to_rfc3339(time: SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// Get current time as an RFC3339 formatted string
///
/// # Examples
///
/// ```
/// use gl_core::time::now_iso8601;
/// let timestamp = now_iso8601();
/// assert!(!timestamp.is_empty());
/// ```
pub fn now_iso8601() -> String {
    to_rfc3339(utc_now())
}

/// Create a monotonic duration measurer
///
/// # Examples
///
/// ```
/// use gl_core::MonotonicTimer;
/// use std::thread;
/// use std::time::Duration;
///
/// let timer = MonotonicTimer::new();
/// thread::sleep(Duration::from_millis(1));
/// let elapsed = timer.elapsed();
/// assert!(elapsed >= Duration::from_millis(1));
/// ```
pub struct MonotonicTimer {
    start: Instant,
}

impl MonotonicTimer {
    /// Create a new timer starting now
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed time since creation
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Reset the timer to now
    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
}

impl Default for MonotonicTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::UNIX_EPOCH;

    #[test]
    fn test_utc_now() {
        let now = utc_now();
        let duration_since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
        // Should be a reasonable time (after 2020)
        assert!(duration_since_epoch.as_secs() > 1_577_836_800); // 2020-01-01
    }

    #[test]
    fn test_to_rfc3339() {
        let time = UNIX_EPOCH + Duration::from_secs(1_609_459_200); // 2021-01-01
        let timestamp = to_rfc3339(time);
        assert_eq!(timestamp, "2021-01-01T00:00:00Z");
    }

    #[test]
    fn test_monotonic_timer() {
        let timer = MonotonicTimer::new();
        thread::sleep(Duration::from_millis(1));
        let elapsed = timer.elapsed();
        // Timer should show some elapsed time, but be reasonable
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn test_monotonic_timer_reset() {
        let mut timer = MonotonicTimer::new();
        thread::sleep(Duration::from_millis(1));
        let first_elapsed = timer.elapsed();
        timer.reset();
        let second_elapsed = timer.elapsed();
        // After reset, elapsed time should be less than before
        assert!(second_elapsed < first_elapsed);
    }
}

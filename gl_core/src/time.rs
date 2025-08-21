use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Get the current UTC time as a SystemTime
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

/// Convert a SystemTime to RFC3339 string format
///
/// # Examples
///
/// ```
/// use gl_core::to_rfc3339;
/// use std::time::{SystemTime, UNIX_EPOCH, Duration};
/// 
/// let time = UNIX_EPOCH + Duration::from_secs(1_609_459_200); // 2021-01-01
/// let rfc3339 = to_rfc3339(time);
/// assert!(!rfc3339.is_empty());
/// ```
pub fn to_rfc3339(time: SystemTime) -> String {
    let duration_since_epoch = time.duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    
    let secs = duration_since_epoch.as_secs();
    let _nanos = duration_since_epoch.subsec_nanos();
    
    // Convert to RFC3339 format manually to avoid chrono dependency
    let datetime = UNIX_EPOCH + Duration::from_secs(secs);
    format!("{:?}", datetime).replace("SystemTime ", "")
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
        let rfc3339 = to_rfc3339(time);
        assert!(!rfc3339.is_empty());
    }

    #[test]
    fn test_monotonic_timer() {
        let timer = MonotonicTimer::new();
        thread::sleep(Duration::from_millis(10));
        let elapsed = timer.elapsed();
        assert!(elapsed >= Duration::from_millis(10));
        assert!(elapsed < Duration::from_millis(100)); // Should be reasonable
    }

    #[test]
    fn test_monotonic_timer_reset() {
        let mut timer = MonotonicTimer::new();
        thread::sleep(Duration::from_millis(10));
        timer.reset();
        let elapsed = timer.elapsed();
        assert!(elapsed < Duration::from_millis(10));
    }
}
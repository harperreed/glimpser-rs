//! ABOUTME: Circuit breaker pattern for database operations
//! ABOUTME: Prevents cascade failures by temporarily disabling database access during persistent errors

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Circuit open due to failures
    HalfOpen, // Testing if service is recovered
}

impl CircuitState {
    /// Convert to metric value (0=closed, 1=open, 2=half-open)
    pub fn to_metric_value(self) -> i64 {
        match self {
            CircuitState::Closed => 0,
            CircuitState::Open => 1,
            CircuitState::HalfOpen => 2,
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening circuit
    pub failure_threshold: u64,
    /// Number of consecutive successes needed to close circuit from half-open
    pub success_threshold: u64,
    /// Duration to wait before attempting half-open state
    pub timeout_duration: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout_duration: Duration::from_secs(60),
        }
    }
}

/// Circuit breaker for database operations
#[derive(Debug)]
pub struct DatabaseCircuitBreaker {
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: std::sync::Mutex<Option<SystemTime>>,
    is_open: AtomicBool,
    config: CircuitBreakerConfig,
}

impl DatabaseCircuitBreaker {
    /// Create a new circuit breaker with given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: std::sync::Mutex::new(None),
            is_open: AtomicBool::new(false),
            config,
        }
    }

    /// Create a new circuit breaker with default configuration
    pub fn default_config() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        if !self.is_open.load(Ordering::Relaxed) {
            return CircuitState::Closed;
        }

        // Check if enough time has passed to try half-open
        if let Ok(last_failure) = self.last_failure_time.lock() {
            if let Some(last_time) = *last_failure {
                if let Ok(elapsed) = last_time.elapsed() {
                    if elapsed > self.config.timeout_duration {
                        // Transition to half-open: keep circuit marked as "open" but allow
                        // requests through. Will fully close after enough successes.
                        return CircuitState::HalfOpen;
                    }
                }
            }
        }

        CircuitState::Open
    }

    /// Check if circuit is open (should reject operations)
    pub fn is_open(&self) -> bool {
        let state = self.state();

        // In half-open state, allow requests through to test recovery
        // The state will transition back to closed after enough successes
        if state == CircuitState::HalfOpen {
            return false;
        }

        state == CircuitState::Open
    }

    /// Record a successful operation
    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        let successes = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;

        let was_open = self.is_open.load(Ordering::Relaxed);

        // If we have enough successes, close the circuit
        if successes >= self.config.success_threshold {
            self.is_open.store(false, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);

            if was_open {
                info!("Database circuit breaker closed after successful recovery");
            }
        } else if was_open {
            debug!(
                successes = successes,
                threshold = self.config.success_threshold,
                "Database circuit breaker recovering"
            );
        }
    }

    /// Record a failed operation
    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;

        if failures >= self.config.failure_threshold {
            let was_open = self.is_open.load(Ordering::Relaxed);
            self.is_open.store(true, Ordering::Relaxed);

            if let Ok(mut last_failure) = self.last_failure_time.lock() {
                *last_failure = Some(SystemTime::now());
            }

            if !was_open {
                warn!(
                    failures = failures,
                    timeout_secs = self.config.timeout_duration.as_secs(),
                    "Database circuit breaker opened due to consecutive failures"
                );
            }
        } else {
            debug!(
                failures = failures,
                threshold = self.config.failure_threshold,
                "Database operation failure recorded"
            );
        }
    }

    /// Get current failure count
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Relaxed)
    }

    /// Get current success count
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout_duration: Duration::from_secs(60),
        };
        let breaker = DatabaseCircuitBreaker::new(config);

        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(!breaker.is_open());

        // Record failures
        breaker.record_failure();
        assert!(!breaker.is_open());

        breaker.record_failure();
        assert!(!breaker.is_open());

        breaker.record_failure();
        assert!(breaker.is_open());
        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_closes_after_successes() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout_duration: Duration::from_millis(10),
        };
        let breaker = DatabaseCircuitBreaker::new(config);

        // Open the circuit
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.is_open());

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(20));

        // Now in half-open state
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // is_open() should return false in half-open to allow testing
        assert!(!breaker.is_open());

        // Record successes
        breaker.record_success();
        breaker.record_success();

        assert!(!breaker.is_open());
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_success_resets_failure_count() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout_duration: Duration::from_secs(60),
        };
        let breaker = DatabaseCircuitBreaker::new(config);

        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);

        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
        assert!(!breaker.is_open());
    }
}

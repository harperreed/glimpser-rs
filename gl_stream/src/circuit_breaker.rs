//! ABOUTME: Circuit breaker pattern implementation for stream health management
//! ABOUTME: Prevents continuous retries on failing streams with exponential backoff

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CircuitState {
    /// Normal operation - requests are allowed
    Closed,
    /// Failure threshold exceeded - requests are blocked
    Open,
    /// Testing if service has recovered - limited requests allowed
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Stream health status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamHealth {
    /// Current circuit breaker state
    pub state: CircuitState,
    /// Consecutive failure count
    pub consecutive_failures: u64,
    /// Total failure count
    pub total_failures: u64,
    /// Total success count
    pub total_successes: u64,
    /// Last failure time
    pub last_failure: Option<String>,
    /// Last success time
    pub last_success: Option<String>,
    /// Current backoff duration in seconds
    pub current_backoff_secs: u64,
    /// Time when circuit will transition to half-open (if in open state)
    pub next_retry_at: Option<String>,
}

/// Configuration for circuit breaker behavior
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening circuit
    pub failure_threshold: u64,
    /// Number of consecutive successes in half-open before closing circuit
    pub success_threshold: u64,
    /// Initial backoff duration when circuit opens
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Timeout duration for circuit to stay open before trying half-open
    pub open_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,       // Open after 5 consecutive failures
            success_threshold: 2,        // Close after 2 consecutive successes in half-open
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            open_timeout: Duration::from_secs(30), // Try half-open after 30s
        }
    }
}

/// Internal state for circuit breaker
struct CircuitBreakerState {
    state: CircuitState,
    consecutive_failures: u64,
    consecutive_successes: u64,
    last_failure_time: Option<Instant>,
    last_success_time: Option<Instant>,
    current_backoff: Duration,
    opened_at: Option<Instant>,
}

/// Circuit breaker for stream health management
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
    total_failures: Arc<AtomicU64>,
    total_successes: Arc<AtomicU64>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default config
    pub fn new() -> Self {
        Self::with_config(CircuitBreakerConfig::default())
    }

    /// Create a new circuit breaker with custom config
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitBreakerState {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                consecutive_successes: 0,
                last_failure_time: None,
                last_success_time: None,
                current_backoff: config.initial_backoff,
                opened_at: None,
            })),
            config,
            total_failures: Arc::new(AtomicU64::new(0)),
            total_successes: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Check if a request should be allowed through
    pub async fn should_allow_request(&self) -> bool {
        let mut state = self.state.write().await;

        match state.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if we should transition to half-open
                if let Some(opened_at) = state.opened_at {
                    if opened_at.elapsed() >= self.config.open_timeout {
                        info!("Circuit breaker transitioning from Open to HalfOpen");
                        state.state = CircuitState::HalfOpen;
                        state.consecutive_successes = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                // In half-open, we allow limited requests to test recovery
                true
            }
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self) {
        let mut state = self.state.write().await;
        self.total_successes.fetch_add(1, Ordering::Relaxed);
        state.last_success_time = Some(Instant::now());
        state.consecutive_failures = 0;

        match state.state {
            CircuitState::Closed => {
                // Already closed, just reset backoff
                state.current_backoff = self.config.initial_backoff;
            }
            CircuitState::HalfOpen => {
                state.consecutive_successes += 1;
                debug!(
                    consecutive_successes = state.consecutive_successes,
                    threshold = self.config.success_threshold,
                    "Circuit breaker recording success in HalfOpen state"
                );

                if state.consecutive_successes >= self.config.success_threshold {
                    info!("Circuit breaker transitioning from HalfOpen to Closed");
                    state.state = CircuitState::Closed;
                    state.current_backoff = self.config.initial_backoff;
                    state.opened_at = None;
                }
            }
            CircuitState::Open => {
                // Should not happen, but handle gracefully
                warn!("Recorded success while circuit is Open - this should not happen");
            }
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        let mut state = self.state.write().await;
        self.total_failures.fetch_add(1, Ordering::Relaxed);
        state.last_failure_time = Some(Instant::now());
        state.consecutive_failures += 1;
        state.consecutive_successes = 0;

        match state.state {
            CircuitState::Closed => {
                debug!(
                    consecutive_failures = state.consecutive_failures,
                    threshold = self.config.failure_threshold,
                    "Circuit breaker recording failure in Closed state"
                );

                if state.consecutive_failures >= self.config.failure_threshold {
                    warn!(
                        consecutive_failures = state.consecutive_failures,
                        "Circuit breaker transitioning from Closed to Open"
                    );
                    state.state = CircuitState::Open;
                    state.opened_at = Some(Instant::now());

                    // Calculate exponential backoff
                    let backoff_multiplier = self.config.backoff_multiplier
                        .powi((state.consecutive_failures - self.config.failure_threshold) as i32);
                    let new_backoff = state.current_backoff.mul_f64(backoff_multiplier);
                    state.current_backoff = new_backoff.min(self.config.max_backoff);

                    info!(
                        backoff_secs = state.current_backoff.as_secs(),
                        "Circuit opened with backoff"
                    );
                }
            }
            CircuitState::HalfOpen => {
                warn!(
                    "Circuit breaker transitioning from HalfOpen to Open after failure"
                );
                state.state = CircuitState::Open;
                state.opened_at = Some(Instant::now());

                // Increase backoff on half-open failure
                let new_backoff = state.current_backoff.mul_f64(self.config.backoff_multiplier);
                state.current_backoff = new_backoff.min(self.config.max_backoff);
            }
            CircuitState::Open => {
                // Already open, just update backoff if needed
                debug!(
                    consecutive_failures = state.consecutive_failures,
                    current_backoff_secs = state.current_backoff.as_secs(),
                    "Additional failure while circuit is Open"
                );
            }
        }
    }

    /// Get the current backoff duration
    pub async fn get_backoff_duration(&self) -> Duration {
        let state = self.state.read().await;
        if state.state == CircuitState::Open {
            state.current_backoff
        } else {
            Duration::from_millis(500) // Default retry delay when not in open state
        }
    }

    /// Get current health status
    pub async fn health(&self) -> StreamHealth {
        let state = self.state.read().await;

        let next_retry_at = if state.state == CircuitState::Open {
            state.opened_at.map(|opened| {
                let next_retry = opened + self.config.open_timeout;
                let duration = next_retry.duration_since(Instant::now());
                let future_time = std::time::SystemTime::now() + duration;
                humantime::format_rfc3339(future_time).to_string()
            })
        } else {
            None
        };

        StreamHealth {
            state: state.state,
            consecutive_failures: state.consecutive_failures,
            total_failures: self.total_failures.load(Ordering::Relaxed),
            total_successes: self.total_successes.load(Ordering::Relaxed),
            last_failure: state.last_failure_time.map(|_| {
                humantime::format_rfc3339(std::time::SystemTime::now()).to_string()
            }),
            last_success: state.last_success_time.map(|_| {
                humantime::format_rfc3339(std::time::SystemTime::now()).to_string()
            }),
            current_backoff_secs: state.current_backoff.as_secs(),
            next_retry_at,
        }
    }

    /// Force reset the circuit breaker to closed state
    pub async fn reset(&self) {
        let mut state = self.state.write().await;
        info!("Circuit breaker manually reset to Closed state");
        state.state = CircuitState::Closed;
        state.consecutive_failures = 0;
        state.consecutive_successes = 0;
        state.current_backoff = self.config.initial_backoff;
        state.opened_at = None;
    }

    /// Get current circuit state
    pub async fn get_state(&self) -> CircuitState {
        let state = self.state.read().await;
        state.state
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new();
        assert_eq!(cb.get_state().await, CircuitState::Closed);
        assert!(cb.should_allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Record failures
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
        assert!(!cb.should_allow_request().await);
    }

    #[tokio::test]
    async fn test_circuit_transitions_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            open_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Should transition to half-open when checked
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_closes_after_successes_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            open_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        // Wait and transition to half-open
        sleep(Duration::from_millis(150)).await;
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        // Record successes
        cb.record_success().await;
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        cb.record_success().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_reopens_on_failure_in_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            open_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;

        // Wait and transition to half-open
        sleep(Duration::from_millis(150)).await;
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        // Failure in half-open reopens circuit
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            open_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Open the circuit with 2 failures
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        // Initial backoff should be 1 second
        let backoff = cb.get_backoff_duration().await;
        assert_eq!(backoff, Duration::from_secs(1));

        // Wait for circuit to try half-open
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(cb.should_allow_request().await);
        assert_eq!(cb.get_state().await, CircuitState::HalfOpen);

        // Failure in half-open reopens circuit with increased backoff
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        let backoff = cb.get_backoff_duration().await;
        assert_eq!(backoff, Duration::from_secs(2));
    }

    #[tokio::test]
    async fn test_health_reporting() {
        let cb = CircuitBreaker::new();

        // Initial health
        let health = cb.health().await;
        assert_eq!(health.state, CircuitState::Closed);
        assert_eq!(health.consecutive_failures, 0);
        assert_eq!(health.total_failures, 0);
        assert_eq!(health.total_successes, 0);

        // After failure
        cb.record_failure().await;
        let health = cb.health().await;
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.total_failures, 1);

        // After success
        cb.record_success().await;
        let health = cb.health().await;
        assert_eq!(health.consecutive_failures, 0);
        assert_eq!(health.total_successes, 1);
    }

    #[tokio::test]
    async fn test_manual_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Open);

        // Manual reset
        cb.reset().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);
        assert!(cb.should_allow_request().await);
    }

    #[tokio::test]
    async fn test_success_resets_consecutive_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::with_config(config);

        // Two failures
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);

        // Success resets counter
        cb.record_success().await;

        let health = cb.health().await;
        assert_eq!(health.consecutive_failures, 0);

        // Would need 3 more failures to open now
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.get_state().await, CircuitState::Closed);
    }
}

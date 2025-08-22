//! ABOUTME: Circuit breaker pattern for notification adapters
//! ABOUTME: Prevents cascade failures by temporarily disabling failing adapters

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

use crate::{Notification, NotificationError, Notifier, Result};

/// Simple circuit breaker state
#[derive(Debug)]
enum CircuitState {
    Closed,   // Normal operation
    Open,     // Circuit open due to failures
    HalfOpen, // Testing if service is recovered
}

/// Simple circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u64,
    pub success_threshold: u64,
    pub timeout_seconds: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout_seconds: 60,
        }
    }
}

/// Simple circuit breaker implementation
#[derive(Debug)]
pub struct SimpleCircuitBreaker {
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: std::sync::Mutex<Option<SystemTime>>,
    is_open: AtomicBool,
    config: CircuitBreakerConfig,
}

impl SimpleCircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: std::sync::Mutex::new(None),
            is_open: AtomicBool::new(false),
            config,
        }
    }

    pub fn is_open(&self) -> bool {
        if !self.is_open.load(Ordering::Relaxed) {
            return false;
        }

        // Check if enough time has passed to try half-open
        if let Ok(last_failure) = self.last_failure_time.lock() {
            if let Some(last_time) = *last_failure {
                if let Ok(elapsed) = last_time.elapsed() {
                    if elapsed > Duration::from_secs(self.config.timeout_seconds) {
                        // Move to half-open state
                        self.is_open.store(false, Ordering::Relaxed);
                        self.success_count.store(0, Ordering::Relaxed);
                        return false;
                    }
                }
            }
        }

        true
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Relaxed);
        let successes = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;

        // If we have enough successes in half-open state, close the circuit
        if successes >= self.config.success_threshold {
            self.is_open.store(false, Ordering::Relaxed);
            self.success_count.store(0, Ordering::Relaxed);
        }
    }

    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;

        if failures >= self.config.failure_threshold {
            self.is_open.store(true, Ordering::Relaxed);
            if let Ok(mut last_failure) = self.last_failure_time.lock() {
                *last_failure = Some(SystemTime::now());
            }
        }
    }
}

/// Circuit breaker wrapper for notification adapters
#[derive(Debug)]
pub struct CircuitBreakerWrapper<T: Notifier> {
    inner: T,
    circuit_breaker: Arc<SimpleCircuitBreaker>,
}

impl<T: Notifier> CircuitBreakerWrapper<T> {
    /// Create a new circuit breaker wrapper with default configuration
    pub fn new(inner: T) -> Self {
        Self::with_config(inner, CircuitBreakerConfig::default())
    }

    /// Create circuit breaker wrapper with custom configuration
    pub fn with_config(inner: T, config: CircuitBreakerConfig) -> Self {
        let circuit_breaker = Arc::new(SimpleCircuitBreaker::new(config));

        Self {
            inner,
            circuit_breaker,
        }
    }

    /// Get the current circuit breaker state
    pub fn is_open(&self) -> bool {
        self.circuit_breaker.is_open()
    }

    /// Get the underlying adapter
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait::async_trait]
impl<T: Notifier> Notifier for CircuitBreakerWrapper<T> {
    async fn send(&self, msg: &Notification) -> Result<()> {
        // Check if circuit breaker is open
        if self.circuit_breaker.is_open() {
            warn!(
                notification_id = %msg.id,
                adapter = self.inner.name(),
                "Circuit breaker is open, skipping notification"
            );
            return Err(NotificationError::CircuitBreakerOpen(
                self.inner.name().to_string(),
            ));
        }

        // Attempt to send the notification
        match self.inner.send(msg).await {
            Ok(()) => {
                // Record success
                self.circuit_breaker.record_success();

                debug!(
                    notification_id = %msg.id,
                    adapter = self.inner.name(),
                    "Notification sent successfully"
                );

                Ok(())
            }
            Err(e) => {
                // Determine if this error should count as a failure for the circuit breaker
                let should_record_failure = match &e {
                    NotificationError::HttpError(http_err) => {
                        // Record failures for server errors and network issues
                        http_err.is_timeout()
                            || http_err.is_connect()
                            || http_err
                                .status()
                                .map(|s| s.is_server_error())
                                .unwrap_or(true)
                    }
                    NotificationError::CircuitBreakerOpen(_) => false, // Don't compound circuit breaker errors
                    NotificationError::RetryExhausted(_) => true, // This indicates persistent failures
                    _ => false, // Don't fail circuit for client errors like bad config
                };

                if should_record_failure {
                    self.circuit_breaker.record_failure();

                    if self.circuit_breaker.is_open() {
                        info!(
                            adapter = self.inner.name(),
                            "Circuit breaker opened due to failures"
                        );
                    }
                }

                warn!(
                    notification_id = %msg.id,
                    adapter = self.inner.name(),
                    error = %e,
                    circuit_open = self.circuit_breaker.is_open(),
                    "Notification failed"
                );

                Err(e)
            }
        }
    }

    async fn health_check(&self) -> Result<()> {
        // Circuit breaker doesn't apply to health checks
        self.inner.health_check().await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

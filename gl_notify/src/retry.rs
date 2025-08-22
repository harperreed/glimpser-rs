//! ABOUTME: Retry logic with exponential backoff and jitter for notifications
//! ABOUTME: Provides configurable retry behavior for failed notification attempts

use std::time::Duration;
use tracing::{debug, warn};

use crate::{Notification, NotificationError, Notifier, Result};

/// Simple retry configuration  
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay_ms: 100,
            max_delay_ms: 30000, // 30 seconds
            multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for a given attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms = (self.initial_delay_ms as f64 * self.multiplier.powi(attempt as i32)) as u64;
        let capped_delay = delay_ms.min(self.max_delay_ms);
        Duration::from_millis(capped_delay)
    }
}

/// Retry wrapper for notification adapters
#[derive(Debug)]
pub struct RetryWrapper<T: Notifier> {
    inner: T,
    config: RetryConfig,
}

impl<T: Notifier> RetryWrapper<T> {
    /// Create a new retry wrapper with default configuration
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            config: RetryConfig::default(),
        }
    }

    /// Create retry wrapper with custom configuration
    pub fn with_config(inner: T, config: RetryConfig) -> Self {
        Self {
            inner,
            config,
        }
    }

    /// Get the underlying adapter
    pub fn inner(&self) -> &T {
        &self.inner
    }
}

#[async_trait::async_trait]
impl<T: Notifier> Notifier for RetryWrapper<T> {
    async fn send(&self, msg: &Notification) -> Result<()> {
        let mut attempt = 0;

        loop {
            match self.inner.send(msg).await {
                Ok(()) => {
                    if attempt > 0 {
                        debug!(
                            notification_id = %msg.id,
                            adapter = self.inner.name(),
                            attempt = attempt + 1,
                            "Notification sent successfully after retry"
                        );
                    }
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    
                    if attempt >= self.config.max_retries {
                        warn!(
                            notification_id = %msg.id,
                            adapter = self.inner.name(),
                            attempts = attempt,
                            error = %e,
                            "Notification failed after all retry attempts"
                        );
                        return Err(NotificationError::RetryExhausted(format!(
                            "{} failed after {} attempts: {}",
                            self.inner.name(),
                            attempt,
                            e
                        )));
                    }

                    // Determine if error is retryable
                    let should_retry = match &e {
                        NotificationError::HttpError(http_err) => {
                            // Retry on network errors, 5xx status codes, and timeouts
                            http_err.is_timeout() || 
                            http_err.is_connect() ||
                            http_err.status().map(|s| s.is_server_error()).unwrap_or(false)
                        }
                        NotificationError::CircuitBreakerOpen(_) => false, // Don't retry if circuit breaker is open
                        _ => true, // Retry other errors like serialization, etc.
                    };

                    if !should_retry {
                        warn!(
                            notification_id = %msg.id,
                            adapter = self.inner.name(),
                            error = %e,
                            "Non-retryable error, giving up"
                        );
                        return Err(e);
                    }

                    let delay = self.config.delay_for_attempt(attempt - 1);
                    debug!(
                        notification_id = %msg.id,
                        adapter = self.inner.name(),
                        attempt = attempt,
                        delay_ms = delay.as_millis(),
                        error = %e,
                        "Notification failed, retrying after delay"
                    );
                    
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn health_check(&self) -> Result<()> {
        // Don't retry health checks - they should be fast
        self.inner.health_check().await
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}
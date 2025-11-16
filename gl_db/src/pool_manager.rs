//! ABOUTME: Connection pool manager with timeout and retry logic
//! ABOUTME: Provides resilient connection acquisition with exponential backoff

use std::sync::Arc;
use std::time::Duration;

use gl_core::{Error, Result};
use sqlx::{SqliteConnection, SqlitePool};
use tracing::{debug, warn};

use crate::circuit_breaker::DatabaseCircuitBreaker;
use crate::metrics::PoolMetrics;

/// Configuration for connection pool management
#[derive(Debug, Clone)]
pub struct PoolManagerConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_retry_delay: Duration,
    /// Maximum delay between retries
    pub max_retry_delay: Duration,
    /// Multiplier for exponential backoff
    pub retry_multiplier: f64,
    /// Timeout for acquiring a connection
    pub acquire_timeout: Duration,
}

impl Default for PoolManagerConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_retry_delay: Duration::from_millis(100),
            max_retry_delay: Duration::from_secs(5),
            retry_multiplier: 2.0,
            acquire_timeout: Duration::from_secs(10),
        }
    }
}

impl PoolManagerConfig {
    /// Calculate delay for a given retry attempt
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms =
            (self.initial_retry_delay.as_millis() as f64 * self.retry_multiplier.powi(attempt as i32))
                as u64;
        let delay = Duration::from_millis(delay_ms);
        delay.min(self.max_retry_delay)
    }
}

/// Pool manager that handles connection acquisition with resilience features
#[derive(Debug, Clone)]
pub struct PoolManager {
    pool: SqlitePool,
    config: PoolManagerConfig,
    circuit_breaker: Arc<DatabaseCircuitBreaker>,
    metrics: Arc<PoolMetrics>,
}

impl PoolManager {
    /// Create a new pool manager with default configuration
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_config(pool, PoolManagerConfig::default())
    }

    /// Create a new pool manager with custom configuration
    pub fn with_config(pool: SqlitePool, config: PoolManagerConfig) -> Self {
        Self {
            pool,
            config,
            circuit_breaker: Arc::new(DatabaseCircuitBreaker::default_config()),
            metrics: Arc::new(PoolMetrics::new()),
        }
    }

    /// Create a new pool manager with custom config, circuit breaker, and metrics
    pub fn with_components(
        pool: SqlitePool,
        config: PoolManagerConfig,
        circuit_breaker: Arc<DatabaseCircuitBreaker>,
        metrics: Arc<PoolMetrics>,
    ) -> Self {
        Self {
            pool,
            config,
            circuit_breaker,
            metrics,
        }
    }

    /// Get the underlying pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get pool metrics
    pub fn metrics(&self) -> &Arc<PoolMetrics> {
        &self.metrics
    }

    /// Get circuit breaker
    pub fn circuit_breaker(&self) -> &Arc<DatabaseCircuitBreaker> {
        &self.circuit_breaker
    }

    /// Update pool metrics based on current pool state
    pub fn update_pool_metrics(&self) {
        // SQLite pool doesn't expose these metrics directly, but we can track
        // connections through acquire/release patterns
        let idle = self.pool.num_idle() as i64;
        let size = self.pool.size() as i64;
        let active = size - idle;

        self.metrics.set_idle(idle);
        self.metrics.set_active(active);

        // Update circuit breaker state
        let state = self.circuit_breaker.state();
        self.metrics.set_circuit_breaker_state(state.to_metric_value());
    }

    /// Acquire a connection with timeout and retry logic
    pub async fn acquire(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
        // Check circuit breaker
        if self.circuit_breaker.is_open() {
            self.metrics.record_failed();
            return Err(Error::Database(
                "Database circuit breaker is open, refusing connection attempts".to_string(),
            ));
        }

        let mut attempt = 0;

        loop {
            // Check circuit breaker before each attempt (it may have opened during retries)
            if attempt > 0 && self.circuit_breaker.is_open() {
                self.metrics.record_failed();
                return Err(Error::Database(
                    "Database circuit breaker opened during retry attempts".to_string(),
                ));
            }

            // Update metrics before acquisition attempt
            self.update_pool_metrics();

            // Try to acquire connection with timeout
            match self.try_acquire_with_timeout().await {
                Ok(conn) => {
                    // Success!
                    self.circuit_breaker.record_success();
                    self.metrics.record_acquired();

                    if attempt > 0 {
                        debug!(
                            attempt = attempt + 1,
                            "Connection acquired successfully after retry"
                        );
                    }

                    return Ok(conn);
                }
                Err(e) => {
                    attempt += 1;

                    // Check if we should retry
                    if attempt >= self.config.max_retries {
                        // Max retries exceeded
                        self.circuit_breaker.record_failure();
                        self.metrics.record_failed();

                        warn!(
                            attempts = attempt,
                            error = %e,
                            "Failed to acquire database connection after all retry attempts"
                        );

                        return Err(Error::Database(format!(
                            "Failed to acquire connection after {} attempts: {}",
                            attempt, e
                        )));
                    }

                    // Determine if error is retryable
                    let should_retry = Self::is_retryable_error(&e);

                    if !should_retry {
                        self.circuit_breaker.record_failure();
                        self.metrics.record_failed();

                        warn!(
                            error = %e,
                            "Non-retryable database error, giving up"
                        );

                        return Err(e);
                    }

                    // Calculate retry delay
                    let delay = self.config.delay_for_attempt(attempt - 1);
                    self.metrics.record_retry();

                    debug!(
                        attempt = attempt,
                        delay_ms = delay.as_millis(),
                        error = %e,
                        "Database connection acquisition failed, retrying after delay"
                    );

                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Try to acquire connection with timeout
    async fn try_acquire_with_timeout(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
        match tokio::time::timeout(self.config.acquire_timeout, self.pool.acquire()).await {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(e)) => {
                // Pool error
                Err(Error::Database(format!("Pool error: {}", e)))
            }
            Err(_) => {
                // Timeout
                self.metrics.record_timeout();
                Err(Error::Database(format!(
                    "Timeout acquiring connection after {:?}",
                    self.config.acquire_timeout
                )))
            }
        }
    }

    /// Determine if an error is retryable
    fn is_retryable_error(error: &Error) -> bool {
        match error {
            Error::Database(msg) => {
                // Retry on timeout and pool exhaustion errors
                msg.contains("Timeout") || msg.contains("Pool error")
            }
            _ => false,
        }
    }

    /// Execute a query with automatic connection management
    pub async fn with_connection<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut SqliteConnection) -> futures_util::future::BoxFuture<'_, Result<T>>,
    {
        let mut conn = self.acquire().await?;
        f(&mut *conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_manager_config_default() {
        let config = PoolManagerConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.acquire_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_pool_manager_config_delay_calculation() {
        let config = PoolManagerConfig::default();

        // First retry: 100ms * 2^0 = 100ms
        assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));

        // Second retry: 100ms * 2^1 = 200ms
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));

        // Third retry: 100ms * 2^2 = 400ms
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));

        // Should cap at max_retry_delay (5 seconds)
        assert_eq!(config.delay_for_attempt(10), Duration::from_secs(5));
    }

    #[test]
    fn test_is_retryable_error() {
        // Timeout errors should be retryable
        let timeout_error = Error::Database("Timeout acquiring connection".to_string());
        assert!(PoolManager::is_retryable_error(&timeout_error));

        // Pool errors should be retryable
        let pool_error = Error::Database("Pool error: connection closed".to_string());
        assert!(PoolManager::is_retryable_error(&pool_error));

        // Other database errors should not be retryable
        let other_error = Error::Database("Syntax error".to_string());
        assert!(!PoolManager::is_retryable_error(&other_error));
    }
}

//! ABOUTME: Resource limiter for snapshot generation to prevent thread pool exhaustion
//! ABOUTME: Implements semaphore-based concurrency control with metrics

use gl_core::Result;
use metrics::{counter, gauge};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{debug, instrument};

/// Configuration for snapshot resource limiting
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotLimiterConfig {
    /// Maximum number of concurrent blocking operations (default: 10)
    /// This prevents exhausting Tokio's blocking thread pool
    pub max_concurrent_operations: usize,
}

impl Default for SnapshotLimiterConfig {
    fn default() -> Self {
        Self {
            // Conservative default to prevent thread pool exhaustion
            // Tokio's default blocking pool size is typically 512 threads
            // We limit to 10 to ensure other blocking operations can proceed
            max_concurrent_operations: 10,
        }
    }
}

/// Resource limiter for snapshot generation operations
/// Uses semaphore to prevent thread pool exhaustion under high load
#[derive(Debug, Clone)]
pub struct SnapshotResourceLimiter {
    semaphore: Arc<Semaphore>,
    active_operations: Arc<AtomicU64>,
    total_operations: Arc<AtomicU64>,
    config: SnapshotLimiterConfig,
}

impl SnapshotResourceLimiter {
    /// Create a new resource limiter with the given configuration
    pub fn new(config: SnapshotLimiterConfig) -> Self {
        // Initialize metrics
        gauge!("snapshot_limiter_max_operations").set(config.max_concurrent_operations as f64);

        Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_operations)),
            active_operations: Arc::new(AtomicU64::new(0)),
            total_operations: Arc::new(AtomicU64::new(0)),
            config,
        }
    }

    /// Acquire a permit to perform a blocking operation
    /// Returns a permit guard that will automatically release the permit when dropped
    #[instrument(skip(self))]
    pub async fn acquire(&self) -> Result<SnapshotPermit> {
        // Record waiting metrics
        let available = self.semaphore.available_permits();
        let waiting = self.config.max_concurrent_operations.saturating_sub(available);

        gauge!("snapshot_limiter_waiting_operations").set(waiting as f64);
        gauge!("snapshot_limiter_available_permits").set(available as f64);

        if waiting > 0 {
            debug!(
                waiting = waiting,
                available = available,
                max = self.config.max_concurrent_operations,
                "Waiting for snapshot operation permit"
            );

            // Increment blocking pool saturation metric when waiting
            counter!("snapshot_limiter_wait_events_total").increment(1);
        }

        // Acquire permit (will wait if none available)
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| gl_core::Error::Config(format!("Failed to acquire semaphore: {}", e)))?;

        // Update active operations counter
        let active = self.active_operations.fetch_add(1, Ordering::SeqCst) + 1;
        let total = self.total_operations.fetch_add(1, Ordering::SeqCst) + 1;

        gauge!("snapshot_limiter_active_operations").set(active as f64);
        counter!("snapshot_limiter_total_operations").absolute(total);

        debug!(
            active = active,
            total = total,
            available = self.semaphore.available_permits(),
            "Acquired snapshot operation permit"
        );

        Ok(SnapshotPermit {
            _permit: permit,
            active_operations: Arc::clone(&self.active_operations),
        })
    }

    /// Get the current number of active operations
    pub fn active_operations(&self) -> u64 {
        self.active_operations.load(Ordering::SeqCst)
    }

    /// Get the total number of operations performed
    pub fn total_operations(&self) -> u64 {
        self.total_operations.load(Ordering::SeqCst)
    }

    /// Get the number of available permits
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Check if the limiter is currently at capacity
    pub fn is_saturated(&self) -> bool {
        self.semaphore.available_permits() == 0
    }
}

impl Default for SnapshotResourceLimiter {
    fn default() -> Self {
        Self::new(SnapshotLimiterConfig::default())
    }
}

/// RAII guard for a snapshot operation permit
/// Automatically releases the permit when dropped
pub struct SnapshotPermit {
    _permit: OwnedSemaphorePermit,
    active_operations: Arc<AtomicU64>,
}

impl Drop for SnapshotPermit {
    fn drop(&mut self) {
        let active = self.active_operations.fetch_sub(1, Ordering::SeqCst) - 1;
        gauge!("snapshot_limiter_active_operations").set(active as f64);

        debug!(
            active = active,
            "Released snapshot operation permit"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_limiter_creation() {
        let config = SnapshotLimiterConfig::default();
        let limiter = SnapshotResourceLimiter::new(config.clone());

        assert_eq!(limiter.active_operations(), 0);
        assert_eq!(limiter.total_operations(), 0);
        assert_eq!(
            limiter.available_permits(),
            config.max_concurrent_operations
        );
        assert!(!limiter.is_saturated());
    }

    #[tokio::test]
    async fn test_acquire_and_release() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 5,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        // Acquire a permit
        let permit = limiter.acquire().await.unwrap();
        assert_eq!(limiter.active_operations(), 1);
        assert_eq!(limiter.total_operations(), 1);
        assert_eq!(limiter.available_permits(), 4);

        // Drop the permit
        drop(permit);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(limiter.active_operations(), 0);
        assert_eq!(limiter.available_permits(), 5);
    }

    #[tokio::test]
    async fn test_multiple_concurrent_operations() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 3,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        // Acquire multiple permits
        let _permit1 = limiter.acquire().await.unwrap();
        let _permit2 = limiter.acquire().await.unwrap();
        let _permit3 = limiter.acquire().await.unwrap();

        assert_eq!(limiter.active_operations(), 3);
        assert_eq!(limiter.total_operations(), 3);
        assert_eq!(limiter.available_permits(), 0);
        assert!(limiter.is_saturated());
    }

    #[tokio::test]
    async fn test_saturation_and_wait() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 2,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        // Acquire all available permits
        let _permit1 = limiter.acquire().await.unwrap();
        let _permit2 = limiter.acquire().await.unwrap();

        assert!(limiter.is_saturated());
        assert_eq!(limiter.available_permits(), 0);

        // Spawn a task that will wait for a permit
        let limiter_clone = limiter.clone();
        let handle = tokio::spawn(async move {
            let _permit = limiter_clone.acquire().await.unwrap();
            // Permit acquired successfully
        });

        // Give the spawned task time to start waiting
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Release one permit - this should allow the waiting task to proceed
        drop(_permit1);

        // Wait for the spawned task to complete
        handle.await.unwrap();

        // After completion, should have one permit available again
        assert_eq!(limiter.available_permits(), 1);
    }

    #[tokio::test]
    async fn test_sequential_operations() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 2,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        // Perform multiple sequential operations
        for i in 0..5 {
            let permit = limiter.acquire().await.unwrap();
            assert_eq!(limiter.active_operations(), 1);
            assert_eq!(limiter.total_operations(), i + 1);
            drop(permit);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(limiter.total_operations(), 5);
        assert_eq!(limiter.active_operations(), 0);
    }

    #[tokio::test]
    async fn test_permit_drop_behavior() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 3,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        {
            let _p1 = limiter.acquire().await.unwrap();
            let _p2 = limiter.acquire().await.unwrap();
            assert_eq!(limiter.active_operations(), 2);
        } // Permits dropped here

        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(limiter.active_operations(), 0);
        assert_eq!(limiter.available_permits(), 3);
    }

    #[tokio::test]
    async fn test_custom_config() {
        let config = SnapshotLimiterConfig {
            max_concurrent_operations: 50,
        };
        let limiter = SnapshotResourceLimiter::new(config);

        assert_eq!(limiter.available_permits(), 50);
    }
}

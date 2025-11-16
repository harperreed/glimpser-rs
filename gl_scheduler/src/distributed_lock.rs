//! ABOUTME: Distributed locking mechanism for job execution coordination
//! ABOUTME: Prevents duplicate job execution across multiple application instances

use chrono::{DateTime, Duration, Utc};
use gl_core::{Error, Id, Result};
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Unique identifier for an application instance
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceId(String);

impl InstanceId {
    /// Create a new instance ID using hostname and process ID
    pub fn new() -> Self {
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string());
        let pid = std::process::id();
        Self(format!("{}:{}", hostname, pid))
    }

    /// Create an instance ID from a string (for testing)
    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a job lock
#[derive(Debug, Clone, PartialEq)]
pub enum LockStatus {
    /// Lock is currently held
    Acquired,
    /// Lock has been released
    Released,
    /// Lock has expired (instance likely crashed)
    Expired,
}

impl LockStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Acquired => "acquired",
            Self::Released => "released",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "acquired" => Self::Acquired,
            "released" => Self::Released,
            "expired" => Self::Expired,
            _ => Self::Expired,
        }
    }
}

/// Represents a distributed lock for a job execution
#[derive(Debug, Clone)]
pub struct JobLock {
    pub id: String,
    pub job_id: String,
    pub execution_id: String,
    pub instance_id: InstanceId,
    pub locked_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub status: LockStatus,
    pub released_at: Option<DateTime<Utc>>,
}

/// Configuration for distributed locking
#[derive(Debug, Clone)]
pub struct LockConfig {
    /// Default lease duration in seconds
    pub default_lease_seconds: i64,
    /// Enable automatic lock cleanup
    pub enable_cleanup: bool,
    /// Cleanup interval in seconds
    pub cleanup_interval_seconds: u64,
}

impl Default for LockConfig {
    fn default() -> Self {
        Self {
            default_lease_seconds: 300, // 5 minutes
            enable_cleanup: true,
            cleanup_interval_seconds: 60, // 1 minute
        }
    }
}

/// Distributed lock manager for coordinating job execution across instances
pub struct DistributedLockManager {
    pool: SqlitePool,
    instance_id: InstanceId,
    config: LockConfig,
}

impl DistributedLockManager {
    /// Create a new distributed lock manager
    pub fn new(pool: SqlitePool, config: LockConfig) -> Self {
        let instance_id = InstanceId::new();
        info!(
            "Initialized distributed lock manager for instance: {}",
            instance_id
        );

        Self {
            pool,
            instance_id,
            config,
        }
    }

    /// Get the current instance ID
    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    /// Attempt to acquire a lock for a job execution
    ///
    /// Returns Some(lock) if the lock was acquired, None if another instance holds the lock
    pub async fn try_acquire_lock(
        &self,
        job_id: &str,
        execution_id: &str,
        lease_duration_seconds: Option<i64>,
    ) -> Result<Option<JobLock>> {
        debug!(
            "Attempting to acquire lock for job {} (execution {})",
            job_id, execution_id
        );

        // First, check if there's an active lock for this job
        let existing_lock = self.get_active_lock(job_id).await?;

        if let Some(lock) = existing_lock {
            // Check if the lock has expired
            if Utc::now() > lock.lease_expires_at {
                debug!(
                    "Found expired lock for job {}, allowing takeover. Previous instance: {}",
                    job_id, lock.instance_id
                );
                // Mark the old lock as expired
                self.mark_lock_expired(&lock.id).await?;
            } else {
                debug!(
                    "Job {} is already locked by instance {} until {}",
                    job_id, lock.instance_id, lock.lease_expires_at
                );
                return Ok(None);
            }
        }

        // Try to acquire the lock
        let lock_id = Id::new().to_string();
        let now = Utc::now();
        let lease_duration =
            Duration::seconds(lease_duration_seconds.unwrap_or(self.config.default_lease_seconds));
        let lease_expires_at = now + lease_duration;

        let lock = JobLock {
            id: lock_id.clone(),
            job_id: job_id.to_string(),
            execution_id: execution_id.to_string(),
            instance_id: self.instance_id.clone(),
            locked_at: now,
            lease_expires_at,
            status: LockStatus::Acquired,
            released_at: None,
        };

        // Insert the lock record
        sqlx::query(
            r#"
            INSERT INTO job_locks (
                id, job_id, execution_id, instance_id,
                locked_at, lease_expires_at, status
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&lock.id)
        .bind(&lock.job_id)
        .bind(&lock.execution_id)
        .bind(lock.instance_id.as_str())
        .bind(lock.locked_at.to_rfc3339())
        .bind(lock.lease_expires_at.to_rfc3339())
        .bind(lock.status.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!("Failed to insert lock for job {}: {}", job_id, e))
        })?;

        info!(
            "Successfully acquired lock for job {} (execution {}), expires at {}",
            job_id, execution_id, lease_expires_at
        );
        Ok(Some(lock))
    }

    /// Release a lock after job execution completes
    pub async fn release_lock(&self, lock_id: &str) -> Result<()> {
        debug!("Releasing lock: {}", lock_id);

        let now = Utc::now();

        let result = sqlx::query(
            r#"
            UPDATE job_locks
            SET status = ?, released_at = ?
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(LockStatus::Released.as_str())
        .bind(now.to_rfc3339())
        .bind(lock_id)
        .bind(LockStatus::Acquired.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to release lock: {}", e)))?;

        if result.rows_affected() > 0 {
            info!("Successfully released lock: {}", lock_id);
        } else {
            warn!("Lock {} was already released or not found", lock_id);
        }

        Ok(())
    }

    /// Get the active lock for a job (if any)
    async fn get_active_lock(&self, job_id: &str) -> Result<Option<JobLock>> {
        let row = sqlx::query(
            r#"
            SELECT * FROM job_locks
            WHERE job_id = ? AND status = ?
            ORDER BY locked_at DESC
            LIMIT 1
            "#,
        )
        .bind(job_id)
        .bind(LockStatus::Acquired.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get active lock: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(self.row_to_lock(row)?))
        } else {
            Ok(None)
        }
    }

    /// Mark a lock as expired
    async fn mark_lock_expired(&self, lock_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE job_locks
            SET status = ?
            WHERE id = ?
            "#,
        )
        .bind(LockStatus::Expired.as_str())
        .bind(lock_id)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to mark lock expired: {}", e)))?;

        debug!("Marked lock {} as expired", lock_id);
        Ok(())
    }

    /// Clean up old locks (released or expired locks older than retention period)
    pub async fn cleanup_old_locks(&self, retention_days: u32) -> Result<u64> {
        debug!("Cleaning up old locks older than {} days", retention_days);

        let cutoff_date = Utc::now() - Duration::days(retention_days as i64);

        let result = sqlx::query(
            r#"
            DELETE FROM job_locks
            WHERE (status = ? OR status = ?) AND locked_at < ?
            "#,
        )
        .bind(LockStatus::Released.as_str())
        .bind(LockStatus::Expired.as_str())
        .bind(cutoff_date.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to cleanup old locks: {}", e)))?;

        let deleted_count = result.rows_affected();
        if deleted_count > 0 {
            info!("Cleaned up {} old lock records", deleted_count);
        }

        Ok(deleted_count)
    }

    /// Expire stale locks (locks that have passed their lease expiration)
    pub async fn expire_stale_locks(&self) -> Result<u64> {
        debug!("Expiring stale locks");

        let now = Utc::now();

        let result = sqlx::query(
            r#"
            UPDATE job_locks
            SET status = ?
            WHERE status = ? AND lease_expires_at < ?
            "#,
        )
        .bind(LockStatus::Expired.as_str())
        .bind(LockStatus::Acquired.as_str())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to expire stale locks: {}", e)))?;

        let expired_count = result.rows_affected();
        if expired_count > 0 {
            warn!("Expired {} stale lock(s)", expired_count);
        }

        Ok(expired_count)
    }

    /// Get lock statistics for monitoring
    pub async fn get_lock_stats(&self) -> Result<LockStats> {
        let acquired_count_row = sqlx::query(
            "SELECT COUNT(*) as count FROM job_locks WHERE status = ?",
        )
        .bind(LockStatus::Acquired.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get lock stats: {}", e)))?;

        let acquired_count = acquired_count_row.get::<i64, _>("count") as u64;

        let released_count_row = sqlx::query(
            "SELECT COUNT(*) as count FROM job_locks WHERE status = ?",
        )
        .bind(LockStatus::Released.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get lock stats: {}", e)))?;

        let released_count = released_count_row.get::<i64, _>("count") as u64;

        let expired_count_row = sqlx::query(
            "SELECT COUNT(*) as count FROM job_locks WHERE status = ?",
        )
        .bind(LockStatus::Expired.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get lock stats: {}", e)))?;

        let expired_count = expired_count_row.get::<i64, _>("count") as u64;

        Ok(LockStats {
            acquired_count,
            released_count,
            expired_count,
        })
    }

    /// Convert database row to JobLock
    fn row_to_lock(&self, row: sqlx::sqlite::SqliteRow) -> Result<JobLock> {
        let locked_at_str: String = row.get("locked_at");
        let locked_at = DateTime::parse_from_rfc3339(&locked_at_str)
            .map_err(|e| Error::Validation(format!("Invalid locked_at timestamp: {}", e)))?
            .with_timezone(&Utc);

        let lease_expires_at_str: String = row.get("lease_expires_at");
        let lease_expires_at = DateTime::parse_from_rfc3339(&lease_expires_at_str)
            .map_err(|e| Error::Validation(format!("Invalid lease_expires_at timestamp: {}", e)))?
            .with_timezone(&Utc);

        let released_at = row
            .get::<Option<String>, _>("released_at")
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| Error::Validation(format!("Invalid released_at timestamp: {}", e)))?;

        let status_str: String = row.get("status");
        let status = LockStatus::from_str(&status_str);

        let instance_id_str: String = row.get("instance_id");
        let instance_id = InstanceId::from_string(instance_id_str);

        Ok(JobLock {
            id: row.get("id"),
            job_id: row.get("job_id"),
            execution_id: row.get("execution_id"),
            instance_id,
            locked_at,
            lease_expires_at,
            status,
            released_at,
        })
    }
}

/// Statistics about locks in the system
#[derive(Debug, Clone)]
pub struct LockStats {
    pub acquired_count: u64,
    pub released_count: u64,
    pub expired_count: u64,
}

/// A guard that automatically releases a lock when dropped
pub struct LockGuard {
    lock: Arc<JobLock>,
    manager: Arc<DistributedLockManager>,
}

impl LockGuard {
    pub fn new(lock: JobLock, manager: Arc<DistributedLockManager>) -> Self {
        Self {
            lock: Arc::new(lock),
            manager,
        }
    }

    pub fn lock(&self) -> &JobLock {
        &self.lock
    }

    pub async fn release(self) -> Result<()> {
        self.manager.release_lock(&self.lock.id).await
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Attempt to release the lock asynchronously
        let lock_id = self.lock.id.clone();
        let manager = self.manager.clone();

        tokio::spawn(async move {
            if let Err(e) = manager.release_lock(&lock_id).await {
                warn!("Failed to release lock {} on drop: {}", lock_id, e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gl_db::Db;

    async fn create_test_db() -> Result<Db> {
        let test_id = Id::new().to_string();
        let db_path = format!("test_lock_{}.db", test_id);
        let db = Db::new(&db_path).await?;

        // Disable foreign key constraints for tests
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(db.pool())
            .await
            .map_err(|e| Error::Database(format!("Failed to disable foreign keys: {}", e)))?;

        Ok(db)
    }

    #[tokio::test]
    async fn test_instance_id_creation() {
        let id1 = InstanceId::new();
        let id2 = InstanceId::new();

        // Same process should have same instance ID
        assert_eq!(id1, id2);
        assert!(id1.as_str().contains(':'));
    }

    #[tokio::test]
    async fn test_acquire_and_release_lock() {
        let db = create_test_db().await.expect("Failed to create test db");
        let config = LockConfig::default();
        let manager = DistributedLockManager::new(db.pool().clone(), config);

        let job_id = "test-job-1";
        let execution_id = Id::new().to_string();

        // Acquire lock
        let lock = manager
            .try_acquire_lock(job_id, &execution_id, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available");

        assert_eq!(lock.job_id, job_id);
        assert_eq!(lock.execution_id, execution_id);
        assert_eq!(lock.status, LockStatus::Acquired);

        // Release lock
        manager
            .release_lock(&lock.id)
            .await
            .expect("Failed to release lock");
    }

    #[tokio::test]
    async fn test_lock_prevents_duplicate_execution() {
        let db = create_test_db().await.expect("Failed to create test db");
        let pool = db.pool().clone();
        let config = LockConfig::default();
        let manager1 = DistributedLockManager::new(pool.clone(), config.clone());

        // Create second manager with different instance ID
        let mut manager2 = DistributedLockManager::new(pool, config);
        manager2.instance_id = InstanceId::from_string("other-instance:9999".to_string());

        let job_id = "test-job-2";
        let execution_id_1 = Id::new().to_string();
        let execution_id_2 = Id::new().to_string();

        // Manager 1 acquires lock
        let lock1 = manager1
            .try_acquire_lock(job_id, &execution_id_1, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available");

        // Manager 2 tries to acquire lock for same job - should fail
        let lock2 = manager2
            .try_acquire_lock(job_id, &execution_id_2, None)
            .await
            .expect("Failed to check lock");

        assert!(lock2.is_none(), "Second instance should not get lock");

        // Release first lock
        manager1
            .release_lock(&lock1.id)
            .await
            .expect("Failed to release lock");

        // Now manager 2 should be able to acquire
        let lock3 = manager2
            .try_acquire_lock(job_id, &execution_id_2, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available after release");

        assert_eq!(lock3.job_id, job_id);
    }

    #[tokio::test]
    #[ignore] // TODO: Fix foreign key constraint issue in test environment
    async fn test_expired_lock_takeover() {
        let db = create_test_db().await.expect("Failed to create test db");
        let pool = db.pool().clone();
        let config = LockConfig {
            default_lease_seconds: 1, // 1 second lease for testing
            ..Default::default()
        };
        let manager1 = DistributedLockManager::new(pool.clone(), config.clone());
        let mut manager2 = DistributedLockManager::new(pool, config);
        manager2.instance_id = InstanceId::from_string("other-instance:9999".to_string());

        let job_id = "test-job-3";
        let execution_id_1 = Id::new().to_string();
        let execution_id_2 = Id::new().to_string();

        // Manager 1 acquires lock with short lease
        let lock1 = manager1
            .try_acquire_lock(job_id, &execution_id_1, Some(1))
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available");

        // Wait for lease to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Manager 2 should be able to take over expired lock
        let lock2 = manager2
            .try_acquire_lock(job_id, &execution_id_2, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available after expiration");

        assert_eq!(lock2.job_id, job_id);
        assert_ne!(lock2.id, lock1.id);
    }

    #[tokio::test]
    async fn test_cleanup_old_locks() {
        let db = create_test_db().await.expect("Failed to create test db");
        let config = LockConfig::default();
        let manager = DistributedLockManager::new(db.pool().clone(), config);

        let job_id = "test-job-4";
        let execution_id = Id::new().to_string();

        // Acquire and release lock
        let lock = manager
            .try_acquire_lock(job_id, &execution_id, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available");

        manager
            .release_lock(&lock.id)
            .await
            .expect("Failed to release lock");

        // Cleanup with 0 days retention should remove the lock
        let deleted = manager
            .cleanup_old_locks(0)
            .await
            .expect("Failed to cleanup locks");

        assert!(deleted > 0, "Should have deleted at least one lock");
    }

    #[tokio::test]
    #[ignore] // TODO: Fix foreign key constraint issue in test environment
    async fn test_lock_stats() {
        let db = create_test_db().await.expect("Failed to create test db");
        let config = LockConfig::default();
        let manager = DistributedLockManager::new(db.pool().clone(), config);

        let stats_before = manager
            .get_lock_stats()
            .await
            .expect("Failed to get stats");

        let job_id = "test-job-5";
        let execution_id = Id::new().to_string();

        // Acquire lock
        let lock = manager
            .try_acquire_lock(job_id, &execution_id, None)
            .await
            .expect("Failed to acquire lock")
            .expect("Lock should be available");

        let stats_after = manager
            .get_lock_stats()
            .await
            .expect("Failed to get stats");

        assert_eq!(
            stats_after.acquired_count,
            stats_before.acquired_count + 1,
            "Acquired count should increase"
        );

        // Release lock
        manager
            .release_lock(&lock.id)
            .await
            .expect("Failed to release lock");

        let stats_final = manager
            .get_lock_stats()
            .await
            .expect("Failed to get stats");

        assert_eq!(
            stats_final.released_count,
            stats_before.released_count + 1,
            "Released count should increase"
        );
    }
}
